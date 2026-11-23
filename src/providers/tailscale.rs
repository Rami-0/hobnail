use super::*;
use crate::{
    logs::frame::parse_rfc3339_parts,
    util::{fmt_utc, unix_now},
};
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

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
            description: "Up to twelve devices with their online state, Tailscale address, operating system and when offline ones were last seen.",
            recommended: true,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"laptop","secondary":"100.64.0.1 · macOS","value":"Online","status":"up"},{"title":"nas","secondary":"100.64.0.2 · linux","value":"Online","status":"up"},{"title":"phone","secondary":"100.64.0.3 · iOS","value":"2 h ago","status":"down"}]}"#,
            fields: &[PUBLIC_REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "summary",
            name: "Tailnet summary",
            description: "Devices online and offline, keys expiring within 30 days and clients with an update available.",
            recommended: false,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Online","value":"7","glyph":"check","tone":"up"},{"label":"Offline","value":"2","glyph":"server"},{"label":"Keys expiring","value":"1","glyph":"clock","tone":"warn"},{"label":"Updates","value":"3","glyph":"download","tone":"warn"}]}"#,
            fields: &[PUBLIC_REFRESH],
            fetch: summary,
            validate: no_widget_options,
        },
        Widget {
            id: "expiry",
            name: "Expiring device keys",
            description: "Up to ten device keys expiring in the next 30 days, soonest first.",
            recommended: false,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"rows","rows":[{"title":"nas","secondary":"Key expires 2026-12-05 09:00 UTC","value":"in 12 days","status":"warn"},{"title":"build-box","secondary":"Key expires 2026-12-18 14:30 UTC","value":"in 25 days","status":"warn"}]}"#,
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
    #[serde(default)]
    addresses: Vec<String>,
    os: Option<String>,
    #[serde(default)]
    update_available: bool,
}

/// "5 min ago", "3 h ago", "2 days ago".
fn ago(now: u64, at: u64) -> String {
    let secs = now.saturating_sub(at);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
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
        let now = unix_now();
        let mut devices = devices(conn).await?;
        if devices.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No devices in this tailnet.".into(),
            });
        }
        // Online first, then by name.
        devices.sort_by(|a, b| {
            b.connected_to_control
                .cmp(&a.connected_to_control)
                .then_with(|| a.hostname.cmp(&b.hostname))
        });
        let total = devices.len();
        let mut rows: Vec<BodyRow> = devices
            .into_iter()
            .take(12)
            .map(|device| {
                let mut about = Vec::new();
                about.extend(device.addresses.into_iter().next());
                about.extend(device.os);
                let value = if device.connected_to_control {
                    "Online".into()
                } else {
                    timestamp(device.last_seen.as_deref())
                        .map_or_else(|| "Offline".into(), |at| ago(now, at))
                };
                BodyRow {
                    title: display_text(conn, device.hostname),
                    secondary: (!about.is_empty()).then(|| display_text(conn, about.join(" · "))),
                    value: Some(value),
                    status: Some(if device.connected_to_control {
                        Health::Up
                    } else {
                        Health::Down
                    }),
                    ..Default::default()
                }
            })
            .collect();
        if total > rows.len() {
            rows.push(BodyRow {
                title: format!("{} more devices", total - rows.len()),
                ..Default::default()
            });
        }
        Ok(StructuredBody::Rows { rows })
    })
}

fn summary<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let now = unix_now();
        let devices = devices(conn).await?;
        let online = devices.iter().filter(|d| d.connected_to_control).count() as u64;
        let offline = devices.len() as u64 - online;
        let expiring = devices
            .iter()
            .filter(|d| !d.key_expiry_disabled)
            .filter_map(|d| timestamp(d.expires.as_deref()))
            .filter(|at| (now..=now + EXPIRY_WINDOW).contains(at))
            .count() as u64;
        let updates = devices.iter().filter(|d| d.update_available).count() as u64;
        let tile = |label: &str, n: u64, glyph, tone: Option<Health>| StatTile {
            label: label.into(),
            value: n.to_string(),
            glyph: Some(glyph),
            delta: None,
            tone: tone.filter(|_| n > 0),
            series: Vec::new(),
        };
        Ok(StructuredBody::Tiles {
            tiles: vec![
                tile("Online", online, Glyph::Check, Some(Health::Up)),
                tile("Offline", offline, Glyph::Server, None),
                tile("Keys expiring", expiring, Glyph::Clock, Some(Health::Warn)),
                tile("Updates", updates, Glyph::Download, Some(Health::Warn)),
            ],
        })
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
        let mut rows: Vec<_> = expiring
            .into_iter()
            .take(10)
            .map(|(at, name)| {
                let days = (at - now) / 86_400;
                BodyRow {
                    title: display_text(conn, name),
                    secondary: Some(format!("Key expires {}", fmt_utc(at))),
                    value: Some(match days {
                        0 => "today".into(),
                        1 => "in 1 day".into(),
                        n => format!("in {n} days"),
                    }),
                    status: Some(if days < 7 { Health::Down } else { Health::Warn }),
                    ..Default::default()
                }
            })
            .collect();
        if incomplete > 0 {
            rows.push(BodyRow {
                title: format!("Expiry data unavailable for {incomplete} device(s)"),
                ..Default::default()
            });
        }
        Ok(StructuredBody::Rows { rows })
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
        let seen = fmt_utc(unix_now() - 3 * 3600 - 60);
        let snapshot = json!({"devices":[
            {"hostname":"a","connectedToControl":true,"lastSeen":null,"keyExpiryDisabled":false,"expires":expiry,"addresses":["100.64.0.1","fd7a:115c:a1e0::1"],"os":"linux","updateAvailable":true},
            {"hostname":"b","connectedToControl":false,"lastSeen":seen,"keyExpiryDisabled":true,"expires":expiry,"os":"iOS"},
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
                assert_eq!(body["rows"], json!([
                    {"title":"a","secondary":"100.64.0.1 · linux","value":"Online","status":"up"},
                    {"title":"b","secondary":"iOS","value":"3 h ago","status":"down"},
                    {"title":"c","value":"Offline","status":"down"},
                ]));
                let summary = fetch(1).await;
                assert_eq!(summary["tiles"], json!([
                    {"label":"Online","value":"1","glyph":"check","tone":"up"},
                    {"label":"Offline","value":"2","glyph":"server"},
                    {"label":"Keys expiring","value":"1","glyph":"clock","tone":"warn"},
                    {"label":"Updates","value":"1","glyph":"download","tone":"warn"},
                ]));
                let keys = fetch(2).await;
                assert_eq!(keys["rows"].as_array().unwrap().len(), 2);
                assert_eq!(keys["rows"][0]["title"], "a");
                assert_eq!(keys["rows"][0]["value"], "today");
                assert_eq!(keys["rows"][0]["status"], "down");
                assert_eq!(
                    keys["rows"][1]["title"],
                    "Expiry data unavailable for 1 device(s)"
                );
                assert_test_fault(conn, &TAILSCALE, Class::Auth).await;
                assert_test_fault(conn, &TAILSCALE, Class::Protocol).await;
            },
        )
        .await;
    }
}
