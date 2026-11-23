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
    color: "#6b7280",
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
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"item_list","items":[{"title":"laptop","secondary":"100.64.0.1 · online","link":null,"time":null},{"title":"nas","secondary":"100.64.0.2 · online","link":null,"time":null},{"title":"phone","secondary":"100.64.0.3 · last seen 2 h ago","link":null,"time":null}]}"#,
            fields: &[PUBLIC_REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "expiry",
            name: "Expiring device keys",
            description: "Up to ten device keys expiring in the next 30 days.",
            recommended: false,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"stat_grid","rows":[["Expiring in 30 days","1"],["nas","in 12 days"]]}"#,
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
            let status = if device.connected_to_control {
                "Connected now".into()
            } else {
                timestamp(device.last_seen.as_deref()).map_or_else(
                    || "Offline · Last seen unavailable".into(),
                    |at| format!("Offline · Last seen {}", fmt_utc(at)),
                )
            };
            (display_text(conn, device.hostname), status)
        }));
        Ok(StructuredBody::StatGrid { rows })
    })
}

fn expirations<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let now = unix_now();
        let mut expiring = Vec::new();
        let mut incomplete = 0;
        for device in devices(conn)
            .await?
            .into_iter()
            .filter(|device| !device.key_expiry_disabled)
        {
            match timestamp(device.expires.as_deref()) {
                Some(at) if (now..=now + EXPIRY_WINDOW).contains(&at) => {
                    expiring.push((at, device.hostname))
                }
                None => incomplete += 1,
                _ => {}
            }
        }
        expiring.sort_unstable();
        if expiring.is_empty() && incomplete == 0 {
            return Ok(StructuredBody::Stat {
                value: "No device keys expiring in the next 30 days".into(),
            });
        }
        let mut items: Vec<_> = expiring
            .into_iter()
            .take(10)
            .map(|(at, name)| BodyItem {
                icon: None,
                title: display_text(conn, name),
                secondary: Some(format!("Expires {}", fmt_utc(at))),
                link: None,
                time: None,
            })
            .collect();
        if incomplete > 0 {
            items.push(BodyItem {
                title: format!("Expiry data unavailable for {incomplete} device(s)"),
                icon: None,
                secondary: None,
                link: None,
                time: None,
            });
        }
        Ok(StructuredBody::ItemList { items })
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
            {"hostname":"a","connectedToControl":true,"lastSeen":null,"keyExpiryDisabled":false,"expires":expiry},
            {"hostname":"b","connectedToControl":false,"lastSeen":"2026-11-10T01:02:03Z","keyExpiryDisabled":true,"expires":expiry},
            {"hostname":"c","connectedToControl":false,"keyExpiryDisabled":false}
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
                assert_eq!(body["rows"][0], json!(["Online", "1 / 3"]));
                assert_eq!(body["rows"][1][1], "Connected now");
                assert!(body["rows"][2][1].as_str().unwrap().contains("2026-11-10"));
                let keys = fetch(1).await;
                assert_eq!(keys["items"].as_array().unwrap().len(), 2);
                assert_eq!(keys["items"][0]["title"], "a");
                assert_eq!(
                    keys["items"][1]["title"],
                    "Expiry data unavailable for 1 device(s)"
                );
                assert_test_fault(conn, &TAILSCALE, Class::Auth).await;
                assert_test_fault(conn, &TAILSCALE, Class::Protocol).await;
            },
        )
        .await;
    }
}
