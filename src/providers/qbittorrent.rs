use super::*;
use crate::util::human_bytes;
use widgets::StructuredBody;

const USERNAME: &str = "username";
const PASSWORD: &str = "password";
const CSRF_HEADERS: &[TargetHeader] = &[TargetHeader::Origin, TargetHeader::Referer];

pub const QBITTORRENT: Provider = Provider {
    id: "qbittorrent",
    name: "qBittorrent",
    category: "Downloads",
    icon: "qbittorrent",
    description: "Show transfer speeds, torrent counts and free disk space.",
    docs: "https://github.com/qbittorrent/qBittorrent/wiki/WebUI-API-(qBittorrent-5.0)",
    public_internet: false,
    contacts: "Your configured qBittorrent WebUI. Public addresses require internet access.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(USERNAME, "WebUI username", Kind::Secret, "Usually admin.")
        },
        Field {
            required: true,
            max_length: Some(4096),
            ..Field::new(PASSWORD, "WebUI password", Kind::Secret, "")
        },
        Field {
            max_length: Some(128),
            default: Some("SID"),
            ..Field::new(
                "cookie_name",
                "Session cookie name",
                Kind::Text,
                "Keep SID unless your WebUI uses a custom or port-qualified cookie name.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "transfers",
        name: "Torrent activity",
        description:
            "Download and upload speeds, active, seeding and queued torrents, and free space.",
        recommended: true,
        fields: &[REFRESH],
        fetch: transfers,
        validate: no_widget_options,
    }],
};

async fn login(conn: &Conn<'_>) -> Result<Option<Session>, Fault> {
    let cookie_name = conn
        .options()
        .get("cookie_name")
        .and_then(Value::as_str)
        .unwrap_or("SID");
    let response = success(
        conn.request(Step {
            verb: Verb::Post,
            body: Some(Body::Form(&[
                ("username", Input::Credential(USERNAME)),
                ("password", Input::Credential(PASSWORD)),
            ])),
            response_headers: &["set-cookie"],
            target_headers: CSRF_HEADERS,
            ..Step::get("api/v2/auth/login")
        })
        .await?,
    )?;
    match response.body.as_ref() {
        b"Ok." => Ok(Some(conn.session_cookie(&response, cookie_name)?)),
        b"Fails." => Err(Class::Auth.into()),
        _ => Err(Class::Protocol.into()),
    }
}

async fn get<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    session: &Session,
    path: &str,
    query: &[(&str, Input<'_>)],
) -> Result<T, Fault> {
    let response = conn
        .request(Step {
            auth: &[Auth::Cookie(session)],
            query,
            target_headers: CSRF_HEADERS,
            ..Step::get(path)
        })
        .await?;
    // Only authentication failures should renew the SID.
    if response.status == 409 {
        return Err(Class::HttpStatus.into());
    }
    parse_json(response)
}

#[derive(Deserialize)]
struct TransferInfo {
    dl_info_speed: u64,
    up_info_speed: u64,
}

#[derive(Deserialize)]
struct Snapshot {
    server_state: ServerState,
    // Omitted by qBittorrent when there are no torrents.
    #[serde(default)]
    torrents: std::collections::HashMap<String, Torrent>,
}

#[derive(Deserialize)]
struct ServerState {
    free_space_on_disk: i64,
}

#[derive(Deserialize)]
struct Torrent {
    state: String,
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        conn.retry_session(
            || login(conn),
            |session| async move {
                let session = session.ok_or(Class::Auth)?;
                get::<TransferInfo>(conn, &session, "api/v2/transfer/info", &[])
                    .await
                    .map(|_| ())
            },
        )
        .await
    })
}

fn transfers<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        conn.retry_session(
            || login(conn),
            |session| async move {
                let session = session.ok_or(Class::Auth)?;
                let speed: TransferInfo = get(conn, &session, "api/v2/transfer/info", &[]).await?;
                let snapshot: Snapshot = get(
                    conn,
                    &session,
                    "api/v2/sync/maindata",
                    &[("rid", Input::Text("0"))],
                )
                .await?;
                let queued = snapshot
                    .torrents
                    .into_values()
                    .filter(|torrent| matches!(torrent.state.as_str(), "queuedUP" | "queuedDL"))
                    .count();
                let mut rows = vec![
                    (
                        "Download".into(),
                        format!("{}/s", human_bytes(speed.dl_info_speed)),
                    ),
                    (
                        "Upload".into(),
                        format!("{}/s", human_bytes(speed.up_info_speed)),
                    ),
                ];
                for (filter, label) in [("active", "Active"), ("seeding", "Seeding")] {
                    let torrents: Vec<Torrent> = get(
                        conn,
                        &session,
                        "api/v2/torrents/info",
                        &[("filter", Input::Text(filter))],
                    )
                    .await?;
                    rows.push((label.into(), torrents.len().to_string()));
                }
                let free = u64::try_from(snapshot.server_state.free_space_on_disk)
                    .map_or_else(|_| "Unavailable".into(), human_bytes);
                rows.push(("Queued".into(), queued.to_string()));
                rows.push(("Free space".into(), free));
                Ok(StructuredBody::StatGrid { rows })
            },
        )
        .await
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use serde_json::json;

    #[tokio::test]
    async fn sends_bound_csrf_headers_reuses_sid_and_renews_it_once() {
        const LOGIN: &str = "POST /prefix/api/v2/auth/login HTTP/1.1";
        const INFO: &str = "GET /prefix/api/v2/transfer/info HTTP/1.1";
        const SPEED: &str =
            r#"{"dl_info_speed":1048576,"up_info_speed":1024,"connection_status":"connected"}"#;
        const FIRST: &str = "first-fake-sid";
        const SECOND: &str = "second-fake-sid";
        exercise(&QBITTORRENT, &[(USERNAME, "fake-user"), (PASSWORD, "fake+/&pass")], Values::new(), &[
            Exchange { request: LOGIN, status: 200, body: "Ok." },
            Exchange { request: INFO, status: 200, body: SPEED },
            Exchange { request: INFO, status: 403, body: "" },
            Exchange { request: LOGIN, status: 200, body: "Ok." },
            Exchange { request: INFO, status: 200, body: SPEED },
            Exchange { request: "GET /prefix/api/v2/sync/maindata?rid=0 HTTP/1.1", status: 200, body: r#"{"rid":1,"full_update":true,"server_state":{"free_space_on_disk":2147483648},"torrents":{"hash1":{"state":"queuedDL"},"hash2":{"state":"queuedUP"},"hash3":{"state":"uploading"}}}"# },
            Exchange { request: "GET /prefix/api/v2/torrents/info?filter=active HTTP/1.1", status: 200, body: r#"[{"hash":"hash3","state":"uploading"}]"# },
            Exchange { request: "GET /prefix/api/v2/torrents/info?filter=seeding HTTP/1.1", status: 200, body: r#"[{"hash":"hash3","state":"uploading"},{"hash":"hash4","state":"stalledUP"}]"# },
            Exchange { request: INFO, status: 200, body: "{}" },
            Exchange { request: INFO, status: 403, body: "" },
            Exchange { request: LOGIN, status: 200, body: "Ok." },
            Exchange { request: INFO, status: 403, body: "" },
            Exchange { request: LOGIN, status: 200, body: "Fails." },
            Exchange { request: LOGIN, status: 200, body: "Ok." },
        ], |index, wire| {
            let (head, body) = wire.split_once("\r\n\r\n").unwrap();
            let host = head.lines().find_map(|line| line.strip_prefix("host: ")).unwrap();
            assert!(head.lines().any(|line| line == format!("origin: http://{host}")));
            assert!(head.lines().any(|line| line == format!("referer: http://{host}/prefix")));
            assert!(!head.contains("authorization:"));
            assert!(!head.contains("fake-user") && !head.contains("fake+/&pass") && !head.contains("fake%2B"));
            match index {
                0 | 3 | 10 | 12 | 13 => {
                    assert!(!head.contains("cookie:"));
                    assert!(body == "username=fake-user&password=fake%2B%2F%26pass", "unexpected login form");
                    match index {
                        0 => "Set-Cookie: proxy=ignored; Path=/\r\nSet-Cookie: SID=first-fake-sid; HttpOnly; Path=/\r\n",
                        3 | 10 => "Set-Cookie: SID=second-fake-sid; HttpOnly; Path=/\r\n",
                        _ => "",
                    }
                }
                _ => {
                    assert!(body.is_empty());
                    let sid = if index < 3 { FIRST } else { SECOND };
                    assert_header(wire, &format!("cookie: SID={sid}"), sid);
                    ""
                }
            }
        }, async |conn| {
            conn.run((QBITTORRENT.test)(conn)).await.unwrap();
            let body = super::super::fetch(conn, &QBITTORRENT.widgets[0], &Values::new()).await.unwrap();
            assert_eq!(serde_json::to_value(body).unwrap()["rows"], json!([
                ["Download", "1.0 MiB/s"], ["Upload", "1.0 KiB/s"], ["Active", "1"],
                ["Seeding", "2"], ["Queued", "2"], ["Free space", "2.0 GiB"],
            ]));
            assert_test_fault(conn, &QBITTORRENT, Class::Protocol).await;
            assert_test_fault(conn, &QBITTORRENT, Class::Auth).await;
            assert_test_fault(conn, &QBITTORRENT, Class::Auth).await;
            assert_test_fault(conn, &QBITTORRENT, Class::Protocol).await;
        }).await;
    }
}
