use super::*;
use crate::{
    logs::frame::parse_rfc3339_parts,
    util::{fmt_utc, unix_now},
};
use widgets::{BodyItem, StructuredBody};

const TOKEN: &str = "token";
const TAILNET: &str = "tailnet";
const EXPIRY_WINDOW: u64 = 30 * 24 * 60 * 60;

pub const TAILSCALE: Provider = Provider {
    id: "tailscale",
    name: "Tailscale",
    category: "Network",
    icon: "tailscale",
    description: "Show tailnet devices, last seen times and upcoming device key expirations.",
    docs: "https://tailscale.com/docs/reference/tailscale-api",
    public_internet: true,
    contacts: "Tailscale's public API at https://api.tailscale.com (the configured API URL).",
    fields: &[
        Field {
            label: "API URL",
            default: Some("https://api.tailscale.com"),
            help: "Use the default Tailscale API address.",
            ..SERVICE_URL
        },
        Field {
            required: true,
            max_length: Some(253),
            default: Some("-"),
            ..Field::new(
                TAILNET,
                "Tailnet",
                Kind::Text,
                "Use - for the token’s default tailnet.",
            )
        },
        Field {
            required: true,
            max_length: Some(4096),
            ..Field::new(
                TOKEN,
                "API access token",
                Kind::Secret,
                "Use a token that can list devices.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "devices",
            name: "Tailnet devices",
            description: "Online count and last seen for up to ten devices.",
            recommended: true,
            fields: &[PUBLIC_REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "expiry",
            name: "Expiring device keys",
            description: "Up to ten device keys expiring in the next 30 days.",
            recommended: true,
            fields: &[PUBLIC_REFRESH],
            fetch: expirations,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
struct Devices {
    devices: Vec<Device>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    hostname: String,
    connected_to_control: bool,
    last_seen: Option<String>,
    key_expiry_disabled: bool,
    expires: Option<String>,
}

async fn devices(conn: &Conn<'_>) -> Result<Vec<Device>, Fault> {
    let tailnet = conn
        .options()
        .get(TAILNET)
        .and_then(Value::as_str)
        .unwrap_or("-");
    if tailnet.is_empty()
        || matches!(tailnet, "." | "..")
        || !tailnet
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
    {
        return Err(Class::Protocol.into());
    }
    let path = format!("api/v2/tailnet/{tailnet}/devices");
    let result: Devices = parse_json(
        conn.request(Step {
            auth: &[Auth::Bearer(Input::Credential(TOKEN))],
            ..Step::get(&path)
        })
        .await?,
    )?;
    Ok(result.devices)
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { devices(conn).await.map(|_| ()) })
}

fn timestamp(value: Option<&str>) -> Option<u64> {
    let (seconds, _) = parse_rfc3339_parts(value?)?;
    u64::try_from(seconds).ok()
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let mut devices = devices(conn).await?;
        let online = devices
            .iter()
            .filter(|device| device.connected_to_control)
            .count();
        devices.sort_by(|a, b| a.hostname.cmp(&b.hostname));
        let mut rows = vec![("Online".into(), format!("{online} / {}", devices.len()))];
        rows.extend(devices.into_iter().take(10).map(|device| {
            let seen = timestamp(device.last_seen.as_deref()).map_or_else(
                || "Last seen unavailable".into(),
                |at| format!("Last seen {}", fmt_utc(at)),
            );
            let status = if device.connected_to_control {
                "Online"
            } else {
                "Offline"
            };
            (
                display_text(conn, device.hostname),
                format!("{status} · {seen}"),
            )
        }));
        Ok(StructuredBody::StatGrid { rows })
    })
}

fn expirations<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let now = unix_now();
        let mut expiring: Vec<_> = devices(conn)
            .await?
            .into_iter()
            .filter(|device| !device.key_expiry_disabled)
            .filter_map(|device| Some((timestamp(device.expires.as_deref())?, device.hostname)))
            .filter(|(at, _)| (now..=now + EXPIRY_WINDOW).contains(at))
            .collect();
        expiring.sort_unstable();
        if expiring.is_empty() {
            return Ok(StructuredBody::Stat {
                value: "No device keys expiring in the next 30 days".into(),
            });
        }
        Ok(StructuredBody::ItemList {
            items: expiring
                .into_iter()
                .take(10)
                .map(|(at, name)| BodyItem {
                    title: display_text(conn, name),
                    secondary: Some(format!("Expires {}", fmt_utc(at))),
                    link: None,
                    time: None,
                })
                .collect(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn reads_control_status_seen_time_and_enabled_expirations() {
        const PATH: &str = "GET /prefix/api/v2/tailnet/example.com/devices HTTP/1.1";
        let expiry = fmt_utc(unix_now() + 3600);
        let snapshot = json!({"devices":[
            {"hostname":"a","connectedToControl":true,"lastSeen":"2026-11-10T01:02:03Z","keyExpiryDisabled":false,"expires":expiry},
            {"hostname":"b","connectedToControl":false,"lastSeen":null,"keyExpiryDisabled":true,"expires":expiry}
        ]}).to_string();
        exercise(
            &TAILSCALE,
            &[(TOKEN, "fake-access-token")],
            Values::from_iter([(TAILNET.into(), json!("example.com"))]),
            &[
                Exchange {
                    request: PATH,
                    status: 200,
                    body: &snapshot,
                },
                Exchange {
                    request: PATH,
                    status: 200,
                    body: &snapshot,
                },
                Exchange {
                    request: PATH,
                    status: 200,
                    body: &snapshot,
                },
                Exchange {
                    request: PATH,
                    status: 403,
                    body: "",
                },
                Exchange {
                    request: PATH,
                    status: 200,
                    body: "{}",
                },
            ],
            |_, wire| {
                assert_header(
                    wire,
                    "authorization: Bearer fake-access-token",
                    "fake-access-token",
                );
                ""
            },
            async |conn| {
                assert!(super::super::get("tailscale").unwrap().public_internet);
                conn.run(test(conn)).await.unwrap();
                let fetch = async |index: usize| {
                    let body = super::super::fetch(conn, &TAILSCALE.widgets[index], &Values::new())
                        .await
                        .unwrap();
                    serde_json::to_value(body).unwrap()
                };
                let body = fetch(0).await;
                assert_eq!(body["rows"][0], json!(["Online", "1 / 2"]));
                assert!(body["rows"][1][1].as_str().unwrap().contains("2026-11-10"));
                let keys = fetch(1).await;
                assert_eq!(keys["items"].as_array().unwrap().len(), 1);
                assert_eq!(keys["items"][0]["title"], "a");
                assert_test_fault(conn, &TAILSCALE, Class::Auth).await;
                assert_test_fault(conn, &TAILSCALE, Class::Protocol).await;
            },
        )
        .await;
    }
}
