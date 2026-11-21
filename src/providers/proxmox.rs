use super::*;
use crate::util::human_bytes;
use widgets::StructuredBody;

const TOKEN_ID: &str = "token_id";
const SECRET: &str = "token_secret";
const RESOURCES: &str = "api2/json/cluster/resources";

pub const PROXMOX: Provider = Provider {
    id: "proxmox-ve",
    name: "Proxmox VE",
    category: "Infrastructure",
    icon: "proxmox",
    description:
        "Show cluster nodes, running virtual machines and containers, and node CPU and memory.",
    docs: "https://pve.proxmox.com/pve-docs/api-viewer/index.html",
    public_internet: false,
    contacts: "Your configured Proxmox VE API server.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(
                TOKEN_ID,
                "Token ID",
                Kind::Text,
                "Enter the full token ID, such as monitor@pve!hobnail.",
            )
        },
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(
                SECRET,
                "Token secret",
                Kind::Secret,
                "Use a token with read-only audit permissions.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "cluster",
        name: "Cluster overview",
        description: "Online nodes, running guests and CPU and RAM for up to ten nodes.",
        recommended: true,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Deserialize)]
struct Version {
    version: String,
}

#[derive(Deserialize)]
struct Node {
    node: String,
    status: String,
    cpu: Option<f64>,
    mem: Option<u64>,
    maxmem: Option<u64>,
}

#[derive(Deserialize)]
struct Guest {
    #[serde(rename = "type")]
    kind: String,
    status: String,
    #[serde(default)]
    template: u8,
}

fn token_prefix(conn: &Conn<'_>) -> Result<String, Fault> {
    let id = conn
        .options()
        .get(TOKEN_ID)
        .and_then(Value::as_str)
        .ok_or(Class::Auth)?;
    let (account, token) = id.split_once('!').ok_or(Class::Auth)?;
    let (user, realm) = account.rsplit_once('@').ok_or(Class::Auth)?;
    if [user, realm, token].iter().any(|part| {
        part.is_empty()
            || part
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '!' | '=' | ',' | ';'))
    }) {
        return Err(Class::Auth.into());
    }
    Ok(format!("PVEAPIToken={id}="))
}

async fn get<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    path: &str,
    query: &[(&str, Input<'_>)],
) -> Result<T, Fault> {
    let prefix = token_prefix(conn)?;
    let result: Envelope<T> = parse_json(
        conn.request(Step {
            auth: &[Auth::Authorization(&prefix, Input::Credential(SECRET))],
            query,
            ..Step::get(path)
        })
        .await?,
    )?;
    Ok(result.data)
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        let version: Version = get(conn, "api2/json/version", &[]).await?;
        if version.version.is_empty() {
            return Err(Class::Protocol.into());
        }
        Ok(())
    })
}

fn node_metrics(node: &Node) -> String {
    if node.status != "online" {
        return if node.status == "offline" {
            "Offline"
        } else {
            "Unknown"
        }
        .into();
    }
    let cpu = node.cpu.map_or_else(
        || "CPU unavailable".into(),
        |ratio| format!("CPU {:.1}%", ratio * 100.0),
    );
    let ram = match node.mem.zip(node.maxmem) {
        Some((used, total)) if total > 0 => {
            format!("RAM {} / {}", human_bytes(used), human_bytes(total))
        }
        _ => "RAM unavailable".into(),
    };
    format!("{cpu} · {ram}")
}

fn guest_count(guests: &[Guest], kind: &str) -> String {
    let eligible = guests
        .iter()
        .filter(|guest| guest.kind == kind && guest.template == 0);
    let running = eligible
        .clone()
        .filter(|guest| guest.status == "running")
        .count();
    format!("{running} / {}", eligible.count())
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let nodes = within_response_limit(
            get::<Vec<Node>>(conn, RESOURCES, &[("type", Input::Text("node"))]).await,
        )?;
        let guests = within_response_limit(
            get::<Vec<Guest>>(conn, RESOURCES, &[("type", Input::Text("vm"))]).await,
        )?;
        let mut rows = vec![
            (
                "VMs running".into(),
                guests.as_ref().map_or_else(
                    || "List too large".into(),
                    |guests| guest_count(guests, "qemu"),
                ),
            ),
            (
                "Containers running".into(),
                guests.as_ref().map_or_else(
                    || "List too large".into(),
                    |guests| guest_count(guests, "lxc"),
                ),
            ),
        ];
        let online = nodes.as_ref().map_or_else(
            || "List too large".into(),
            |nodes| {
                let online = nodes.iter().filter(|node| node.status == "online").count();
                format!("{online} / {}", nodes.len())
            },
        );
        rows.insert(0, ("Nodes online".into(), online));
        if let Some(mut nodes) = nodes {
            nodes.sort_unstable_by(|a, b| a.node.cmp(&b.node));
            rows.extend(nodes.into_iter().take(10).map(|node| {
                let metrics = node_metrics(&node);
                (display_text(conn, node.node), metrics)
            }));
        }
        Ok(StructuredBody::StatGrid { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn filters_cluster_requests_and_keeps_node_stats_when_vm_list_is_oversized() {
        const VERSION: &str = "GET /prefix/api2/json/version HTTP/1.1";
        const NODES: &str = "GET /prefix/api2/json/cluster/resources?type=node HTTP/1.1";
        const GUESTS: &str = "GET /prefix/api2/json/cluster/resources?type=vm HTTP/1.1";
        const NODE_DATA: &str = r#"{"data":[{"node":"pve","status":"online","cpu":0.125,"mem":1048576,"maxmem":2097152},{"node":"pve2","status":"offline"}]}"#;
        let oversized = "x".repeat(MAX_RESPONSE + 1);
        let exchanges = [
            (VERSION, 200, r#"{"data":{"version":"9.0.1"}}"#),
            (NODES, 200, NODE_DATA),
            (GUESTS, 200, r#"{"data":[{"type":"qemu","status":"running"},{"type":"qemu","status":"stopped","template":1},{"type":"lxc","status":"stopped"}]}"#),
            (NODES, 200, NODE_DATA),
            (GUESTS, 200, &oversized),
            (VERSION, 401, ""),
            (VERSION, 200, "{}"),
        ].map(|(request, status, body)| Exchange { request, status, body });
        exercise(
            &PROXMOX,
            &[(SECRET, "fake-pve-secret")],
            Values::from_iter([(TOKEN_ID.into(), json!("monitor@pve!hobnail"))]),
            &exchanges,
            |_, wire| {
                assert_header(
                    wire,
                    "authorization: PVEAPIToken=monitor@pve!hobnail=fake-pve-secret",
                    "fake-pve-secret",
                );
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let fetch = async || {
                    serde_json::to_value(
                        super::super::fetch(conn, &PROXMOX.widgets[0], &Values::new())
                            .await
                            .unwrap(),
                    )
                    .unwrap()
                };
                assert_eq!(
                    fetch().await["rows"],
                    json!([
                        ["Nodes online", "1 / 2"],
                        ["VMs running", "1 / 1"],
                        ["Containers running", "0 / 1"],
                        ["pve", "CPU 12.5% · RAM 1.0 MiB / 2.0 MiB"],
                        ["pve2", "Offline"]
                    ])
                );
                let degraded = fetch().await;
                assert_eq!(degraded["rows"][0], json!(["Nodes online", "1 / 2"]));
                assert_eq!(
                    degraded["rows"][1],
                    json!(["VMs running", "List too large"])
                );
                assert_test_fault(conn, &PROXMOX, Class::Auth).await;
                assert_test_fault(conn, &PROXMOX, Class::Protocol).await;
            },
        )
        .await;
    }
}
