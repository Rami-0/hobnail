use super::*;
use crate::util::human_bytes;
use widgets::StructuredBody;

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
    widgets: &[Widget {
        id: "peers",
        name: "WireGuard peers",
        description: "Peer count and up to ten peers’ last handshake and transfer totals.",
        recommended: true,
        default_size: size(4, 2),
        min_size: size(3, 2),
        sample: r#"{"kind":"stat_grid","rows":[["Peers","6"],["Connected","2"],["Received","1.2 GB"],["Sent","340 MB"]]}"#,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
};

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
    latest_handshake_at: Option<String>,
    transfer_rx: Option<u64>,
    transfer_tx: Option<u64>,
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
        let peers = peers(conn).await?;
        let mut rows = vec![("Peers".into(), peers.len().to_string())];
        rows.extend(peers.into_iter().take(10).map(|peer| {
            let seen = peer
                .latest_handshake_at
                .map_or_else(|| "No handshake".into(), |time| display_text(conn, time));
            let rx = peer
                .transfer_rx
                .map_or_else(|| "Unavailable".into(), human_bytes);
            let tx = peer
                .transfer_tx
                .map_or_else(|| "Unavailable".into(), human_bytes);
            (
                display_text(conn, peer.name),
                format!("{seen} · ↓ {rx} · ↑ {tx}"),
            )
        }));
        Ok(StructuredBody::StatGrid { rows })
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
            exercise(&WG_EASY, &secrets, Values::new(), &[
                Exchange { request: LOGIN, status: 200, body: login_body },
                Exchange { request: SESSION_GET, status: 200, body: session_body },
                Exchange { request: peers_path, status: 200, body: r#"[{"name":"phone","latestHandshakeAt":null,"transferRx":1024,"transferTx":123}]"# },
                Exchange { request: SESSION_GET, status: 401, body: "" },
                Exchange { request: LOGIN, status: 401, body: "" },
                Exchange { request: LOGIN, status: 200, body: "{}" },
            ], |index, wire| {
                let (head, body) = wire.split_once("\r\n\r\n").unwrap();
                assert!(!head.contains("fake-wg-pass") && !head.contains("fake-wg-user"));
                if [0, 4, 5].contains(&index) {
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
                    ["Peers", "1"], ["phone", "No handshake · ↓ 1.0 KiB · ↑ 123 B"],
                ]));
                assert_test_fault(conn, &WG_EASY, Class::Auth).await;
                assert_test_fault(conn, &WG_EASY, Class::Protocol).await;
            }).await;
        }
    }
}
