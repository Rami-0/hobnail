use super::*;
use crate::{logs::frame::parse_rfc3339_parts, util::human_bytes, util::unix_now};
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

const USER: &str = "username";
const PASSWORD: &str = "password";
const SESSION: &str = "api/session";
const CSRF_HEADERS: &[TargetHeader] = &[TargetHeader::Origin, TargetHeader::Referer];

pub const WG_EASY: Provider = Provider {
    id: "wg_easy",
    name: "WireGuard (wg-easy)",
    category: "Network",
    icon: "wireguard",
    color: "#b0262b",
    description: "Show WireGuard peers, last handshakes and transfer counters.",
    docs: "https://wg-easy.github.io/wg-easy/",
    public_internet: false,
    contacts: "Your configured wg-easy Web UI.",
    fields: &[
        SERVICE_URL,
        Field {
            max_length: Some(256),
            ..Field::new(
                USER,
                "Username (v15 only)",
                Kind::Secret,
                "Leave blank for the v14 password-only login.",
            )
        },
        Field {
            required: true,
            max_length: Some(4096),
            ..Field::new(PASSWORD, "Password", Kind::Secret, "")
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "peers",
            name: "WireGuard peers",
            description: "Up to ten peers, connected ones first, with their last handshake and data received and sent.",
            recommended: true,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"phone","secondary":"↓ 1.2 GiB · ↑ 210 MiB","value":"1 min ago","status":"up"},{"title":"laptop","secondary":"↓ 340 MiB · ↑ 98 MiB","value":"2 min ago","status":"up"},{"title":"tablet","secondary":"↓ 12 MiB · ↑ 2.1 MiB","value":"3 days ago","status":"down"},{"title":"old-router","value":"Disabled"}]}"#,
            fields: &[REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "summary",
            name: "WireGuard summary",
            description: "How many peers exist and are connected now, and the data they received and sent.",
            recommended: false,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Peers","value":"6","glyph":"users"},{"label":"Connected","value":"2","glyph":"check","tone":"up"},{"label":"Received","value":"1.6 GiB","glyph":"download"},{"label":"Sent","value":"312 MiB","glyph":"upload"}]}"#,
            fields: &[REFRESH],
            fetch: summary,
            validate: no_widget_options,
        },
    ],
};

/// A handshake younger than this counts as connected: WireGuard renews
/// sessions every two minutes while traffic flows.
const CONNECTED_SECS: u64 = 3 * 60;

#[derive(Deserialize)]
struct LoginV14 {
    success: bool,
}

#[derive(Deserialize)]
struct LoginV15 {
    status: String,
}

async fn login(conn: &Conn<'_>) -> Result<Option<Session>, Fault> {
    let v15 = conn.has_credential(USER);
    let fields = if v15 {
        &[
            (USER, Input::Credential(USER)),
            (PASSWORD, Input::Credential(PASSWORD)),
        ][..]
    } else {
        &[(PASSWORD, Input::Credential(PASSWORD))][..]
    };
    let mut body = Values::new();
    if v15 {
        body.insert("remember".into(), Value::Bool(false));
    }
    let response = success(
        conn.request(Step {
            verb: Verb::Post,
            body: Some(Body::Json(body, fields)),
            response_headers: &["set-cookie"],
            target_headers: CSRF_HEADERS,
            ..Step::get(SESSION)
        })
        .await?,
    )?;
    let cookie = if v15 {
        let login: LoginV15 =
            serde_json::from_slice(&response.body).map_err(|_| Class::Protocol)?;
        match login.status.as_str() {
            "success" => "wg-easy",
            "TOTP_REQUIRED" | "INVALID_TOTP_CODE" => return Err(Class::Auth.into()),
            _ => return Err(Class::Protocol.into()),
        }
    } else {
        let login: LoginV14 =
            serde_json::from_slice(&response.body).map_err(|_| Class::Protocol)?;
        if !login.success {
            return Err(Class::Auth.into());
        }
        "connect.sid"
    };
    Ok(Some(conn.session_cookie(&response, cookie)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Peer {
    name: String,
    #[serde(default = "enabled_default")]
    enabled: bool,
    latest_handshake_at: Option<String>,
    transfer_rx: Option<u64>,
    transfer_tx: Option<u64>,
}

fn enabled_default() -> bool {
    true
}

impl Peer {
    fn handshake(&self) -> Option<u64> {
        let (seconds, _) = parse_rfc3339_parts(self.latest_handshake_at.as_deref()?)?;
        u64::try_from(seconds).ok()
    }

    fn connected(&self, now: u64) -> bool {
        self.enabled
            && self
                .handshake()
                .is_some_and(|at| now.saturating_sub(at) < CONNECTED_SECS)
    }
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
fn ago(now: u64, at: u64) -> String {
    let secs = now.saturating_sub(at);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

async fn peers(conn: &Conn<'_>) -> Result<Vec<Peer>, Fault> {
    let path = if conn.has_credential(USER) {
        "api/client"
    } else {
        "api/wireguard/client"
    };
    conn.retry_session(
        || login(conn),
        |session| async move {
            let session = session.ok_or(Class::Auth)?;
            parse_json(
                conn.request(Step {
                    auth: &[Auth::Cookie(&session)],
                    target_headers: CSRF_HEADERS,
                    ..Step::get(path)
                })
                .await?,
            )
        },
    )
    .await
}

#[derive(Deserialize)]
struct SessionV14 {
    authenticated: bool,
}

#[derive(Deserialize)]
struct SessionV15 {
    #[serde(rename = "username")]
    _username: String,
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        let v15 = conn.has_credential(USER);
        conn.retry_session(
            || login(conn),
            |session| async move {
                let session = session.ok_or(Class::Auth)?;
                let response = conn
                    .request(Step {
                        auth: &[Auth::Cookie(&session)],
                        target_headers: CSRF_HEADERS,
                        ..Step::get(SESSION)
                    })
                    .await?;
                let authenticated = if v15 {
                    parse_json::<Option<SessionV15>>(response)?.is_some()
                } else {
                    parse_json::<SessionV14>(response)?.authenticated
                };
                authenticated
                    .then_some(())
                    .ok_or_else(|| Class::Auth.into())
            },
        )
        .await
    })
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let now = unix_now();
        let mut peers = peers(conn).await?;
        if peers.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No WireGuard peers yet.".into(),
            });
        }
        // Connected first, then the most recent handshake, then by name.
        peers.sort_by(|a, b| {
            b.connected(now)
                .cmp(&a.connected(now))
                .then_with(|| b.handshake().cmp(&a.handshake()))
                .then_with(|| a.name.cmp(&b.name))
        });
        let total = peers.len();
        let mut rows: Vec<BodyRow> = peers
            .into_iter()
            .take(10)
            .map(|peer| {
                let transfer = match (peer.transfer_rx, peer.transfer_tx) {
                    (None, None) => None,
                    (rx, tx) => Some(format!(
                        "↓ {} · ↑ {}",
                        rx.map_or_else(|| "—".into(), human_bytes),
                        tx.map_or_else(|| "—".into(), human_bytes)
                    )),
                };
                let (value, status) = if !peer.enabled {
                    ("Disabled".into(), None)
                } else {
                    let status = Some(if peer.connected(now) {
                        Health::Up
                    } else {
                        Health::Down
                    });
                    match peer.handshake() {
                        Some(at) => (ago(now, at), status),
                        None => ("No handshake".into(), status),
                    }
                };
                BodyRow {
                    title: display_text(conn, peer.name),
                    secondary: transfer,
                    value: Some(value),
                    status,
                    ..Default::default()
                }
            })
            .collect();
        if total > rows.len() {
            rows.push(BodyRow {
                title: format!("{} more peers", total - rows.len()),
                ..Default::default()
            });
        }
        Ok(StructuredBody::Rows { rows })
    })
}

fn summary<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let now = unix_now();
        let peers = peers(conn).await?;
        let connected = peers.iter().filter(|peer| peer.connected(now)).count();
        let rx: u64 = peers.iter().filter_map(|peer| peer.transfer_rx).sum();
        let tx: u64 = peers.iter().filter_map(|peer| peer.transfer_tx).sum();
        let tile = |label: &str, value: String, glyph| StatTile {
            label: label.into(),
            value,
            glyph: Some(glyph),
            delta: None,
            tone: None,
            series: Vec::new(),
        };
        Ok(StructuredBody::Tiles {
            tiles: vec![
                tile("Peers", peers.len().to_string(), Glyph::Users),
                StatTile {
                    tone: (connected > 0).then_some(Health::Up),
                    ..tile("Connected", connected.to_string(), Glyph::Check)
                },
                tile("Received", human_bytes(rx), Glyph::Download),
                tile("Sent", human_bytes(tx), Glyph::Upload),
            ],
        })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn uses_the_version_specific_login_cookie_and_client_route() {
        const LOGIN: &str = "POST /prefix/api/session HTTP/1.1";
        const SESSION_GET: &str = "GET /prefix/api/session HTTP/1.1";
        for v15 in [false, true] {
            let mut secrets = vec![(PASSWORD, "fake-wg-pass")];
            let (login_body, session_body, peers_path, cookie_name, set_cookie) = if v15 {
                secrets.push((USER, "fake-wg-user"));
                (
                    r#"{"status":"success"}"#,
                    r#"{"username":"fake-wg-user"}"#,
                    "GET /prefix/api/client HTTP/1.1",
                    "wg-easy",
                    "Set-Cookie: wg-easy=fake-wg-cookie; Path=/\r\n",
                )
            } else {
                (
                    r#"{"success":true}"#,
                    r#"{"authenticated":true}"#,
                    "GET /prefix/api/wireguard/client HTTP/1.1",
                    "connect.sid",
                    "Set-Cookie: connect.sid=fake-wg-cookie; Path=/\r\n",
                )
            };
            let recent = crate::util::fmt_utc(unix_now() - 30);
            let list = json!([
                {"name":"phone","enabled":true,"latestHandshakeAt":null,"transferRx":1024,"transferTx":123},
                {"name":"laptop","enabled":true,"latestHandshakeAt":recent,"transferRx":2048,"transferTx":null},
                {"name":"old","enabled":false,"latestHandshakeAt":"2026-01-01T00:00:00.000Z","transferRx":null,"transferTx":null},
            ])
            .to_string();
            exercise(&WG_EASY, &secrets, Values::new(), &[
                Exchange { request: LOGIN, status: 200, body: login_body },
                Exchange { request: SESSION_GET, status: 200, body: session_body },
                Exchange { request: peers_path, status: 200, body: &list },
                Exchange { request: peers_path, status: 200, body: &list },
                Exchange { request: SESSION_GET, status: 401, body: "" },
                Exchange { request: LOGIN, status: 401, body: "" },
                Exchange { request: LOGIN, status: 200, body: "{}" },
            ], |index, wire| {
                let (head, body) = wire.split_once("\r\n\r\n").unwrap();
                assert!(!head.contains("fake-wg-pass") && !head.contains("fake-wg-user"));
                if [0, 5, 6].contains(&index) {
                    let body: Value = serde_json::from_str(body).unwrap();
                    assert_eq!(body[PASSWORD], "fake-wg-pass");
                    assert_eq!(body[USER].as_str(), v15.then_some("fake-wg-user"));
                    assert!(!head.contains("cookie:"));
                    set_cookie
                } else {
                    assert!(body.is_empty());
                    assert_header(wire, &format!("cookie: {cookie_name}=fake-wg-cookie"), "fake-wg-cookie");
                    ""
                }
            }, async |conn| {
                conn.run(test(conn)).await.unwrap();
                let body = super::super::fetch(conn, &WG_EASY.widgets[0], &Values::new()).await.unwrap();
                assert_eq!(serde_json::to_value(body).unwrap()["rows"], json!([
                    {"title":"laptop","secondary":"↓ 2.0 KiB · ↑ —","value":"just now","status":"up"},
                    {"title":"old","value":"Disabled"},
                    {"title":"phone","secondary":"↓ 1.0 KiB · ↑ 123 B","value":"No handshake","status":"down"},
                ]));
                let summary = super::super::fetch(conn, &WG_EASY.widgets[1], &Values::new()).await.unwrap();
                assert_eq!(serde_json::to_value(summary).unwrap()["tiles"], json!([
                    {"label":"Peers","value":"3","glyph":"users"},
                    {"label":"Connected","value":"1","glyph":"check","tone":"up"},
                    {"label":"Received","value":"3.0 KiB","glyph":"download"},
                    {"label":"Sent","value":"123 B","glyph":"upload"},
                ]));
                assert_test_fault(conn, &WG_EASY, Class::Auth).await;
                assert_test_fault(conn, &WG_EASY, Class::Protocol).await;
            }).await;
        }
    }
}
