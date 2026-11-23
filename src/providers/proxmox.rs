use super::*;
use crate::util::{human_bytes, human_duration};
use widgets::{BodyRow, Glyph, Health, MeterRow, StatTile, StructuredBody};

const TOKEN_ID: &str = "token_id";
const SECRET: &str = "token_secret";
const RESOURCES: &str = "api2/json/cluster/resources";

pub const PROXMOX: Provider = Provider {
    id: "proxmox-ve",
    name: "Proxmox VE",
    category: "Infrastructure",
    icon: "proxmox",
    color: "#e57000",
    description:
        "Show cluster nodes, node CPU, memory and disk, guests and their state, and storage use.",
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
    widgets: &[
        Widget {
            id: "cluster",
            name: "Cluster overview",
            description: "Online nodes and running virtual machines and containers.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Nodes online","value":"3 / 3","glyph":"server","tone":"up"},{"label":"VMs running","value":"9 / 12","glyph":"cpu"},{"label":"Containers running","value":"5 / 6","glyph":"database"}]}"#,
            fields: &[REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "nodes",
            name: "Node resources",
            description: "CPU, memory and root disk use of up to four nodes.",
            recommended: false,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"meters","rows":[{"label":"pve1 CPU","percent":21,"detail":"21% of 16 CPUs"},{"label":"pve1 memory","percent":58,"detail":"37.1 GiB of 64.0 GiB"},{"label":"pve1 disk","percent":34,"detail":"32.4 GiB of 94.0 GiB"}]}"#,
            fields: &[REFRESH],
            fetch: nodes,
            validate: no_widget_options,
        },
        Widget {
            id: "guests",
            name: "Virtual machines and containers",
            description: "Up to ten guests with their state, node, CPU and memory, running first.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"101 web","secondary":"VM · pve1 · up 3d 4h","value":"4% · 1.2 GiB","status":"up"},{"title":"102 db","secondary":"Container · pve1 · up 12d 2h","value":"11% · 3.8 GiB","status":"up"},{"title":"110 test","secondary":"VM · pve2 · stopped","value":"—"}]}"#,
            fields: &[
                Field {
                    default: Some("all"),
                    choices: &["all", "running"],
                    ..Field::new(
                        "show",
                        "Show",
                        Kind::Select,
                        "All guests, or only running ones.",
                    )
                },
                REFRESH,
            ],
            fetch: guests,
            validate: no_widget_options,
        },
        Widget {
            id: "storage",
            name: "Storage",
            description: "Used space of up to eight storages, fullest first.",
            recommended: false,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"meters","rows":[{"label":"local-lvm (pve1)","percent":71,"detail":"340 GiB of 480 GiB"},{"label":"backups","percent":44,"detail":"1.8 TiB of 4.0 TiB"},{"label":"local (pve1)","percent":23,"detail":"21.6 GiB of 94.0 GiB"}]}"#,
            fields: &[REFRESH],
            fetch: storage,
            validate: no_widget_options,
        },
    ],
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
    maxcpu: Option<u64>,
    mem: Option<u64>,
    maxmem: Option<u64>,
    disk: Option<u64>,
    maxdisk: Option<u64>,
}

#[derive(Deserialize)]
struct Guest {
    #[serde(rename = "type")]
    kind: String,
    status: String,
    #[serde(default)]
    template: u8,
    vmid: Option<u64>,
    name: Option<String>,
    node: Option<String>,
    cpu: Option<f64>,
    mem: Option<u64>,
    uptime: Option<u64>,
}

#[derive(Deserialize)]
struct Storage {
    storage: String,
    node: Option<String>,
    status: Option<String>,
    disk: Option<u64>,
    maxdisk: Option<u64>,
    #[serde(default)]
    shared: u8,
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

async fn resources<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    kind: &str,
) -> Result<Option<Vec<T>>, Fault> {
    within_response_limit(get(conn, RESOURCES, &[("type", Input::Text(kind))]).await)
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

fn tile(label: &str, value: String, glyph: Glyph, tone: Option<Health>) -> StatTile {
    StatTile {
        label: label.into(),
        value,
        glyph: Some(glyph),
        delta: None,
        tone,
        series: Vec::new(),
    }
}

const TOO_LARGE: &str = "List too large";

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let nodes = resources::<Node>(conn, "node").await?;
        let guests = resources::<Guest>(conn, "vm").await?;
        let (online, tone) = nodes.as_ref().map_or((TOO_LARGE.into(), None), |nodes| {
            let online = nodes.iter().filter(|node| node.status == "online").count();
            let tone = if online == nodes.len() {
                Health::Up
            } else if online == 0 {
                Health::Down
            } else {
                Health::Warn
            };
            (format!("{online} / {}", nodes.len()), Some(tone))
        });
        let count = |kind| {
            guests
                .as_ref()
                .map_or_else(|| TOO_LARGE.into(), |guests| guest_count(guests, kind))
        };
        Ok(StructuredBody::Tiles {
            tiles: vec![
                tile("Nodes online", online, Glyph::Server, tone),
                tile("VMs running", count("qemu"), Glyph::Cpu, None),
                tile("Containers running", count("lxc"), Glyph::Database, None),
            ],
        })
    })
}

fn percent(used: u64, total: u64) -> u8 {
    (used.min(total) as f64 * 100.0 / total as f64).round() as u8
}

fn used_of(used: u64, total: u64) -> String {
    format!("{} of {}", human_bytes(used), human_bytes(total))
}

const NODE_METERS: usize = 4;

fn nodes<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(mut nodes) = resources::<Node>(conn, "node").await? else {
            return Ok(StructuredBody::Unavailable {
                message: "Node list too large to show.".into(),
            });
        };
        nodes.sort_unstable_by(|a, b| a.node.cmp(&b.node));
        let single = nodes.len() == 1;
        let mut rows = Vec::new();
        for node in nodes.into_iter().take(NODE_METERS) {
            let name = display_text(conn, node.node);
            let label = |what: &str| {
                if single {
                    let mut what = what.to_owned();
                    what[..1].make_ascii_uppercase();
                    what
                } else {
                    format!("{name} {what}")
                }
            };
            if node.status != "online" {
                rows.push(MeterRow {
                    label: name.clone(),
                    percent: 0,
                    detail: Some(
                        if node.status == "offline" {
                            "Offline"
                        } else {
                            "Unknown"
                        }
                        .into(),
                    ),
                });
                continue;
            }
            if let Some(cpu) = node.cpu.filter(|cpu| cpu.is_finite()) {
                let share = (cpu.clamp(0.0, 1.0) * 100.0).round() as u8;
                rows.push(MeterRow {
                    label: label("CPU"),
                    percent: share,
                    detail: Some(match node.maxcpu {
                        Some(cpus) => format!("{share}% of {cpus} CPUs"),
                        None => format!("{share}%"),
                    }),
                });
            }
            for (what, used, total) in [
                ("memory", node.mem, node.maxmem),
                ("disk", node.disk, node.maxdisk),
            ] {
                if let Some((used, total)) = used.zip(total).filter(|(_, total)| *total > 0) {
                    rows.push(MeterRow {
                        label: label(what),
                        percent: percent(used, total),
                        detail: Some(used_of(used, total)),
                    });
                }
            }
        }
        if rows.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No nodes visible to this token.".into(),
            });
        }
        Ok(StructuredBody::Meters { rows })
    })
}

const GUEST_ROWS: usize = 10;

fn guests<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    let running_only = options.get("show").and_then(Value::as_str) == Some("running");
    Box::pin(async move {
        let Some(mut guests) = resources::<Guest>(conn, "vm").await? else {
            return Ok(StructuredBody::Unavailable {
                message: "Guest list too large to show.".into(),
            });
        };
        guests.retain(|guest| guest.template == 0 && (!running_only || guest.status == "running"));
        guests.sort_unstable_by_key(|guest| (guest.status != "running", guest.vmid));
        if guests.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No guests to show.".into(),
            });
        }
        let rows = guests
            .into_iter()
            .take(GUEST_ROWS)
            .map(|guest| guest_row(conn, guest))
            .collect();
        Ok(StructuredBody::Rows { rows })
    })
}

fn guest_row(conn: &Conn<'_>, guest: Guest) -> BodyRow {
    let name = guest.name.map(|name| display_text(conn, name));
    let title = match (guest.vmid, name) {
        (Some(id), Some(name)) => format!("{id} {name}"),
        (Some(id), None) => id.to_string(),
        (None, Some(name)) => name,
        (None, None) => "Unnamed guest".into(),
    };
    let kind = if guest.kind == "lxc" {
        "Container"
    } else {
        "VM"
    };
    let running = guest.status == "running";
    let state = match guest.status.as_str() {
        "running" => guest.uptime.map_or_else(
            || "running".into(),
            |up| format!("up {}", human_duration(up)),
        ),
        "stopped" => "stopped".into(),
        "paused" | "suspended" | "prelaunch" => guest.status.clone(),
        _ => "unknown".into(),
    };
    let mut secondary = kind.to_owned();
    if let Some(node) = guest.node {
        secondary.push_str(" · ");
        secondary.push_str(&display_text(conn, node));
    }
    secondary.push_str(" · ");
    secondary.push_str(&state);
    let value = match (running, guest.cpu.filter(|cpu| cpu.is_finite()), guest.mem) {
        (true, Some(cpu), Some(mem)) => {
            format!("{:.0}% · {}", cpu.clamp(0.0, 1.0) * 100.0, human_bytes(mem))
        }
        (true, Some(cpu), None) => format!("{:.0}%", cpu.clamp(0.0, 1.0) * 100.0),
        _ => "—".into(),
    };
    BodyRow {
        title,
        secondary: Some(secondary),
        value: Some(value),
        status: match guest.status.as_str() {
            "running" => Some(Health::Up),
            "paused" | "suspended" => Some(Health::Warn),
            _ => None,
        },
        progress: None,
        bars: Vec::new(),
    }
}

const STORAGE_METERS: usize = 8;

fn storage<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(list) = resources::<Storage>(conn, "storage").await? else {
            return Ok(StructuredBody::Unavailable {
                message: "Storage list too large to show.".into(),
            });
        };
        let mut seen = std::collections::HashSet::new();
        let mut rows: Vec<(f64, MeterRow)> = Vec::new();
        for entry in list {
            // A shared storage is listed once per node; show it once.
            if entry.shared != 0 && !seen.insert(entry.storage.clone()) {
                continue;
            }
            if entry
                .status
                .as_deref()
                .is_some_and(|status| status != "available")
            {
                rows.push((
                    f64::INFINITY,
                    MeterRow {
                        label: storage_label(conn, &entry),
                        percent: 0,
                        detail: Some("Unavailable".into()),
                    },
                ));
                continue;
            }
            let Some((used, total)) = entry.disk.zip(entry.maxdisk).filter(|(_, t)| *t > 0) else {
                continue;
            };
            rows.push((
                used as f64 / total as f64,
                MeterRow {
                    label: storage_label(conn, &entry),
                    percent: percent(used, total),
                    detail: Some(used_of(used, total)),
                },
            ));
        }
        if rows.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No storage visible to this token.".into(),
            });
        }
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        Ok(StructuredBody::Meters {
            rows: rows
                .into_iter()
                .take(STORAGE_METERS)
                .map(|(_, row)| row)
                .collect(),
        })
    })
}

fn storage_label(conn: &Conn<'_>, entry: &Storage) -> String {
    let name = display_text(conn, entry.storage.clone());
    match (&entry.node, entry.shared) {
        (Some(node), 0) => format!("{name} ({})", display_text(conn, node.clone())),
        _ => name,
    }
}
#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    const NODES: &str = "GET /prefix/api2/json/cluster/resources?type=node HTTP/1.1";
    const GUESTS: &str = "GET /prefix/api2/json/cluster/resources?type=vm HTTP/1.1";
    const STORAGE: &str = "GET /prefix/api2/json/cluster/resources?type=storage HTTP/1.1";
    const AUTH: &str = "authorization: PVEAPIToken=monitor@pve!hobnail=fake-pve-secret";

    fn token() -> Values {
        Values::from_iter([(TOKEN_ID.into(), json!("monitor@pve!hobnail"))])
    }

    fn check(_: usize, wire: &str) -> &'static str {
        assert_header(wire, AUTH, "fake-pve-secret");
        ""
    }

    async fn fetch(conn: &Conn<'_>, widget: usize, options: Value) -> Result<Value, Fault> {
        let options = options.as_object().cloned().unwrap_or_default();
        super::super::fetch(conn, &PROXMOX.widgets[widget], &options)
            .await
            .map(|body| serde_json::to_value(body).unwrap())
    }

    #[tokio::test]
    async fn filters_cluster_requests_and_keeps_node_stats_when_vm_list_is_oversized() {
        const VERSION: &str = "GET /prefix/api2/json/version HTTP/1.1";
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
            token(),
            &exchanges,
            check,
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let tiles = |body: Value| -> Vec<Value> {
                    body["tiles"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|tile| json!([tile["label"], tile["value"]]))
                        .collect()
                };
                let body = fetch(conn, 0, Value::Null).await.unwrap();
                assert_eq!(body["tiles"][0]["tone"], "warn");
                assert_eq!(
                    tiles(body),
                    [
                        json!(["Nodes online", "1 / 2"]),
                        json!(["VMs running", "1 / 1"]),
                        json!(["Containers running", "0 / 1"]),
                    ]
                );
                let degraded = tiles(fetch(conn, 0, Value::Null).await.unwrap());
                assert_eq!(degraded[0], json!(["Nodes online", "1 / 2"]));
                assert_eq!(degraded[1], json!(["VMs running", "List too large"]));
                assert_test_fault(conn, &PROXMOX, Class::Auth).await;
                assert_test_fault(conn, &PROXMOX, Class::Protocol).await;
            },
        )
        .await;
    }

    #[tokio::test]
    async fn shows_node_meters_guests_and_storage() {
        // Shapes from the cluster/resources schema (pve-docs api-viewer).
        const NODE_DATA: &str = r#"{"data":[{"id":"node/pve2","type":"node","node":"pve2","status":"offline"},{"id":"node/pve1","type":"node","node":"pve1","status":"online","cpu":0.21,"maxcpu":16,"mem":39836160000,"maxmem":68719476736,"disk":34789235712,"maxdisk":100931731456,"uptime":86400,"level":""}]}"#;
        const ONE_NODE: &str = r#"{"data":[{"node":"pve1","status":"online","cpu":0.5,"mem":1073741824,"maxmem":4294967296}]}"#;
        const GUEST_DATA: &str = r#"{"data":[
            {"id":"qemu/110","type":"qemu","vmid":110,"name":"test","node":"pve2","status":"stopped","template":0},
            {"id":"lxc/102","type":"lxc","vmid":102,"name":"db","node":"pve1","status":"running","cpu":0.11,"mem":4080218931,"uptime":1044000},
            {"id":"qemu/9000","type":"qemu","vmid":9000,"name":"template","node":"pve1","status":"stopped","template":1},
            {"id":"qemu/101","type":"qemu","vmid":101,"name":"web","node":"pve1","status":"running","cpu":0.04,"mem":1288490188,"uptime":273600}]}"#;
        const STORAGE_DATA: &str = r#"{"data":[
            {"id":"storage/pve1/local","type":"storage","storage":"local","node":"pve1","status":"available","disk":23192823808,"maxdisk":100931731456,"shared":0,"plugintype":"dir"},
            {"id":"storage/pve1/nfs","type":"storage","storage":"backups","node":"pve1","status":"available","disk":1979120929996,"maxdisk":4398046511104,"shared":1},
            {"id":"storage/pve2/nfs","type":"storage","storage":"backups","node":"pve2","status":"available","disk":1979120929996,"maxdisk":4398046511104,"shared":1},
            {"id":"storage/pve2/local","type":"storage","storage":"local","node":"pve2","status":"unknown","shared":0}]}"#;
        let exchanges = [
            (NODES, 200, NODE_DATA),
            (NODES, 200, ONE_NODE),
            (GUESTS, 200, GUEST_DATA),
            (GUESTS, 200, GUEST_DATA),
            (STORAGE, 200, STORAGE_DATA),
            (STORAGE, 200, r#"{"data":[]}"#),
            (GUESTS, 401, ""),
            (STORAGE, 200, r#"{"data":{"storage":"not a list"}}"#),
        ]
        .map(|(request, status, body)| Exchange {
            request,
            status,
            body,
        });
        exercise(&PROXMOX, &[(SECRET, "fake-pve-secret")], token(), &exchanges, check, async |conn| {
            let nodes = fetch(conn, 1, Value::Null).await.unwrap();
            assert_eq!(nodes["kind"], "meters");
            assert_eq!(nodes["rows"], json!([
                {"label": "pve1 CPU", "percent": 21, "detail": "21% of 16 CPUs"},
                {"label": "pve1 memory", "percent": 58, "detail": "37.1 GiB of 64.0 GiB"},
                {"label": "pve1 disk", "percent": 34, "detail": "32.4 GiB of 94.0 GiB"},
                {"label": "pve2", "percent": 0, "detail": "Offline"},
            ]));
            let single = fetch(conn, 1, Value::Null).await.unwrap();
            assert_eq!(single["rows"][0]["label"], "CPU");
            assert_eq!(single["rows"][1], json!({"label": "Memory", "percent": 25, "detail": "1.0 GiB of 4.0 GiB"}));

            let all = fetch(conn, 2, json!({"show": "all"})).await.unwrap();
            assert_eq!(all["kind"], "rows");
            assert_eq!(all["rows"], json!([
                {"title": "101 web", "secondary": "VM · pve1 · up 3d 4h", "value": "4% · 1.2 GiB", "status": "up"},
                {"title": "102 db", "secondary": "Container · pve1 · up 12d 2h", "value": "11% · 3.8 GiB", "status": "up"},
                {"title": "110 test", "secondary": "VM · pve2 · stopped", "value": "—"},
            ]));
            let running = fetch(conn, 2, json!({"show": "running"})).await.unwrap();
            assert_eq!(running["rows"].as_array().unwrap().len(), 2);

            let storage = fetch(conn, 3, Value::Null).await.unwrap();
            assert_eq!(storage["rows"], json!([
                {"label": "local (pve2)", "percent": 0, "detail": "Unavailable"},
                {"label": "backups", "percent": 45, "detail": "1.8 TiB of 4.0 TiB"},
                {"label": "local (pve1)", "percent": 23, "detail": "21.6 GiB of 94.0 GiB"},
            ]));
            let empty = fetch(conn, 3, Value::Null).await.unwrap();
            assert_eq!(empty["message"], "No storage visible to this token.");
            assert!(matches!(fetch(conn, 2, Value::Null).await.unwrap_err().class, Class::Auth));
            assert!(matches!(fetch(conn, 3, Value::Null).await.unwrap_err().class, Class::Protocol));
        })
        .await;
    }
}
