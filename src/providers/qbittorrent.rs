use super::json_scan::{ArrayLength, Path, Scalar, Scanner, Visit};
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

/// Queued count and free space from the initial `sync/maindata` snapshot,
/// read while it streams: at rid=0 it carries every torrent's full record,
/// which passes 128 KiB at about 100 torrents.
#[derive(Default)]
struct Snapshot {
    queued: u64,
    free: Option<i64>,
    server_state: bool,
    not_object: bool,
}

impl Visit for Snapshot {
    fn value(&mut self, path: &Path<'_>, value: Scalar<'_>) {
        match (path.depth(), path.key(1), value) {
            (0, _, Scalar::Container { object: true }) => {}
            (0, ..) => self.not_object = true,
            (1, Some("server_state"), Scalar::Container { object: true }) => {
                self.server_state = true;
            }
            (
                2,
                Some("server_state"),
                Scalar::Number {
                    text,
                    complete: true,
                },
            ) if path.key(2) == Some("free_space_on_disk") => {
                self.free = text.parse().ok();
            }
            // torrents.<hash>.state; a torrent can be both queued and listed elsewhere.
            (
                3,
                Some("torrents"),
                Scalar::Str {
                    text,
                    complete: true,
                },
            ) if path.key(3) == Some("state") && matches!(text, "queuedUP" | "queuedDL") => {
                self.queued += 1;
            }
            _ => {}
        }
    }
}

/// Stream a JSON response through `visitor` without retaining it.
async fn scan<V: Visit + Send>(
    conn: &Conn<'_>,
    session: &Session,
    path: &str,
    query: &[(&str, Input<'_>)],
    visitor: V,
) -> Result<V, Fault> {
    let mut scanner = Scanner::new(visitor);
    let mut sink = |chunk: &[u8]| scanner.feed(chunk).map_err(drop);
    let response = conn
        .scan(
            Step {
                auth: &[Auth::Cookie(session)],
                query,
                target_headers: CSRF_HEADERS,
                ..Step::get(path)
            },
            &mut sink,
        )
        .await?;
    if response.status == 409 {
        return Err(Class::HttpStatus.into());
    }
    success(response)?;
    scanner.finish().map_err(|_| Class::Protocol)?;
    Ok(scanner.visitor)
}

async fn count(conn: &Conn<'_>, session: &Session, filter: &str) -> Result<u64, Fault> {
    let length = scan(
        conn,
        session,
        "api/v2/torrents/info",
        &[("filter", Input::Text(filter))],
        ArrayLength::default(),
    )
    .await?;
    if length.not_array {
        return Err(Class::Protocol.into());
    }
    Ok(length.count)
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
                    let count = within_response_limit(count(conn, &session, filter).await)?;
                    rows.push((
                        label.into(),
                        count.map_or_else(unavailable, |n| n.to_string()),
                    ));
                }
                let snapshot = within_response_limit(
                    scan(
                        conn,
                        &session,
                        "api/v2/sync/maindata",
                        &[("rid", Input::Text("0"))],
                        Snapshot::default(),
                    )
                    .await,
                )?;
                if snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.not_object || !snapshot.server_state)
                {
                    return Err(Class::Protocol.into());
                }
                let queued = snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.queued.to_string());
                let free = snapshot
                    .and_then(|snapshot| snapshot.free)
                    .and_then(|free| u64::try_from(free).ok());
                rows.push(("Queued".into(), queued.unwrap_or_else(unavailable)));
                rows.push((
                    "Free space".into(),
                    free.map_or_else(unavailable, human_bytes),
                ));
                Ok(StructuredBody::StatGrid { rows })
            },
        )
        .await
    })
}

fn unavailable() -> String {
    "Unavailable".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use serde_json::json;

    /// One torrent as `torrents/info` and `sync/maindata` describe it (about 1 KB).
    fn torrent(index: usize, state: &str) -> String {
        let hash = format!("{index:040x}");
        format!(
            r#"{{"added_on":1700000000,"amount_left":0,"auto_tmm":false,"availability":-1,"category":"linux","completed":1048576000,"completion_on":1700003600,"content_path":"/downloads/debian-{index}.iso","dl_limit":-1,"dlspeed":0,"downloaded":1048576000,"downloaded_session":0,"eta":8640000,"f_l_piece_prio":false,"force_start":false,"hash":"{hash}","infohash_v1":"{hash}","infohash_v2":"","last_activity":1700007200,"magnet_uri":"magnet:?xt=urn:btih:{hash}&dn=debian-{index}.iso","max_ratio":-1,"max_seeding_time":-1,"name":"debian-{index}.iso \u00e9","num_complete":120,"num_incomplete":3,"num_leechs":0,"num_seeds":0,"priority":0,"progress":1,"ratio":1.25,"ratio_limit":-2,"save_path":"/downloads/","seeding_time":86400,"seeding_time_limit":-2,"seen_complete":1700003600,"seq_dl":false,"size":1048576000,"state":"{state}","super_seeding":false,"tags":"","time_active":90000,"total_size":1048576000,"tracker":"https://tracker.example/announce","up_limit":-1,"uploaded":1310720000,"uploaded_session":0,"upspeed":2048}}"#
        )
    }

    #[tokio::test]
    async fn streams_counts_for_libraries_beyond_the_response_cap() {
        let list = |count: usize, state: &str| {
            format!(
                "[{}]",
                (0..count)
                    .map(|i| torrent(i, state))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        let active = list(150, "uploading");
        let seeding = list(300, "stalledUP");
        let states = ["uploading", "queuedUP", "stalledUP", "queuedDL", "pausedUP"];
        let map = (0..300)
            .map(|i| format!(r#""{i:040x}":{}"#, torrent(i, states[i % 5])))
            .collect::<Vec<_>>()
            .join(",");
        let maindata = format!(
            r#"{{"rid":1,"full_update":true,"categories":{{"linux":{{"name":"linux","savePath":""}}}},"server_state":{{"alltime_dl":1,"connection_status":"connected","dl_info_speed":0,"free_space_on_disk":5368709120,"up_info_speed":2048}},"tags":[],"torrents":{{{map}}},"trackers":{{}}}}"#
        );
        for body in [&active, &seeding, &maindata] {
            assert!(body.len() > MAX_RESPONSE);
        }
        exercise(
            &QBITTORRENT,
            &[(USERNAME, "fake-user"), (PASSWORD, "fake-password")],
            Values::new(),
            &[
                Exchange {
                    request: "POST /prefix/api/v2/auth/login HTTP/1.1",
                    status: 200,
                    body: "Ok.",
                },
                Exchange {
                    request: "GET /prefix/api/v2/transfer/info HTTP/1.1",
                    status: 200,
                    body: r#"{"dl_info_speed":1024,"up_info_speed":0}"#,
                },
                Exchange {
                    request: "GET /prefix/api/v2/torrents/info?filter=active HTTP/1.1",
                    status: 200,
                    body: &active,
                },
                Exchange {
                    request: "GET /prefix/api/v2/torrents/info?filter=seeding HTTP/1.1",
                    status: 200,
                    body: &seeding,
                },
                Exchange {
                    request: "GET /prefix/api/v2/sync/maindata?rid=0 HTTP/1.1",
                    status: 200,
                    body: &maindata,
                },
                // A stream that breaks off or is not the expected shape is a protocol fault.
                Exchange {
                    request: "GET /prefix/api/v2/transfer/info HTTP/1.1",
                    status: 200,
                    body: r#"{"dl_info_speed":0,"up_info_speed":0}"#,
                },
                Exchange {
                    request: "GET /prefix/api/v2/torrents/info?filter=active HTTP/1.1",
                    status: 200,
                    body: &active[..active.len() - 1],
                },
                Exchange {
                    request: "GET /prefix/api/v2/transfer/info HTTP/1.1",
                    status: 200,
                    body: r#"{"dl_info_speed":0,"up_info_speed":0}"#,
                },
                Exchange {
                    request: "GET /prefix/api/v2/torrents/info?filter=active HTTP/1.1",
                    status: 200,
                    body: "{}",
                },
            ],
            |index, _| {
                if index == 0 {
                    "Set-Cookie: SID=fake-sid; Path=/\r\n"
                } else {
                    ""
                }
            },
            async |conn| {
                let fetch = async || {
                    super::super::fetch(conn, &QBITTORRENT.widgets[0], &Values::new()).await
                };
                assert_eq!(
                    serde_json::to_value(fetch().await.unwrap()).unwrap()["rows"],
                    json!([
                        ["Download", "1.0 KiB/s"],
                        ["Upload", "0 B/s"],
                        ["Active", "150"],
                        ["Seeding", "300"],
                        ["Queued", "120"],
                        ["Free space", "5.0 GiB"],
                    ])
                );
                for _ in 0..2 {
                    assert!(matches!(fetch().await.unwrap_err().class, Class::Protocol));
                }
            },
        )
        .await;
    }

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
            Exchange { request: "GET /prefix/api/v2/torrents/info?filter=active HTTP/1.1", status: 200, body: r#"[{"hash":"hash3","state":"uploading"}]"# },
            Exchange { request: "GET /prefix/api/v2/torrents/info?filter=seeding HTTP/1.1", status: 200, body: r#"[{"hash":"hash3","state":"uploading"},{"hash":"hash4","state":"stalledUP"}]"# },
            Exchange { request: "GET /prefix/api/v2/sync/maindata?rid=0 HTTP/1.1", status: 200, body: r#"{"rid":1,"full_update":true,"server_state":{"free_space_on_disk":2147483648},"torrents":{"hash1":{"state":"queuedDL"},"hash2":{"state":"queuedUP"},"hash3":{"state":"uploading"}}}"# },
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
