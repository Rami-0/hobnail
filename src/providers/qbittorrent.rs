use super::json_scan::{ArrayLength, Path, Scalar, Scanner, Visit};
use super::*;
use crate::util::{human_bytes, human_duration};
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

const USERNAME: &str = "username";
const PASSWORD: &str = "password";
const CSRF_HEADERS: &[TargetHeader] = &[TargetHeader::Origin, TargetHeader::Referer];

pub const QBITTORRENT: Provider = Provider {
    id: "qbittorrent",
    name: "qBittorrent",
    category: "Downloads",
    icon: "qbittorrent",
    color: "#3a72c4",
    description:
        "Show transfer speeds, torrent counts, free space and torrents with their progress.",
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
    widgets: &[
        Widget {
            id: "transfers",
            name: "Torrent activity",
            description:
                "Download and upload speeds, active, seeding and queued torrents, and free space.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Download","value":"12.4 MiB/s","glyph":"download"},{"label":"Upload","value":"1.1 MiB/s","glyph":"upload"},{"label":"Active","value":"3","glyph":"activity"},{"label":"Seeding","value":"41","glyph":"upload"},{"label":"Queued","value":"2","glyph":"clock"},{"label":"Free space","value":"1.8 TiB","glyph":"disk"}]}"#,
            fields: &[REFRESH],
            fetch: transfers,
            validate: no_widget_options,
        },
        Widget {
            id: "torrents",
            name: "Torrents",
            description:
                "Up to ten torrents with progress, state, speed and time left, fastest first.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"example-distro-24.04.iso","secondary":"Downloading · 4.7 GiB · 6m left","value":"↓ 8.2 MiB/s","status":"up","progress":62},{"title":"sample-dataset.tar","secondary":"Stalled · 1.2 GiB","value":"↓ 0 B/s","status":"warn","progress":18},{"title":"example-album-flac","secondary":"Seeding · 640 MiB","value":"↑ 320 KiB/s","status":"up","progress":100}]}"#,
            fields: &[
                Field {
                    default: Some("active"),
                    choices: &["active", "downloading", "seeding", "completed", "all"],
                    ..Field::new("filter", "Show", Kind::Select, "Which torrents to list.")
                },
                REFRESH,
            ],
            fetch: torrents,
            validate: no_widget_options,
        },
    ],
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
    let mut sink = |chunk: &[u8]| scanner.feed(chunk).map(|()| true).map_err(drop);
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
                let mut tiles = vec![
                    tile(
                        "Download",
                        format!("{}/s", human_bytes(speed.dl_info_speed)),
                        Glyph::Download,
                    ),
                    tile(
                        "Upload",
                        format!("{}/s", human_bytes(speed.up_info_speed)),
                        Glyph::Upload,
                    ),
                ];
                for (filter, label, glyph) in [
                    ("active", "Active", Glyph::Activity),
                    ("seeding", "Seeding", Glyph::Upload),
                ] {
                    let count = within_response_limit(count(conn, &session, filter).await)?;
                    tiles.push(tile(
                        label,
                        count.map_or_else(unavailable, |n| n.to_string()),
                        glyph,
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
                tiles.push(tile(
                    "Queued",
                    queued.unwrap_or_else(unavailable),
                    Glyph::Clock,
                ));
                tiles.push(tile(
                    "Free space",
                    free.map_or_else(unavailable, human_bytes),
                    Glyph::Disk,
                ));
                Ok(StructuredBody::Tiles { tiles })
            },
        )
        .await
    })
}

fn tile(label: &str, value: String, glyph: Glyph) -> StatTile {
    StatTile {
        label: label.into(),
        value,
        glyph: Some(glyph),
        delta: None,
        tone: None,
        series: Vec::new(),
    }
}

const TORRENT_ROWS: usize = 10;

#[derive(Deserialize)]
struct Torrent {
    name: String,
    progress: f64,
    dlspeed: u64,
    upspeed: u64,
    eta: u64,
    state: String,
    size: u64,
}

/// A readable state, and its dot: transferring, stalled or failed. Stopped,
/// queued and checking torrents get no dot.
fn state(state: &str) -> (&'static str, Option<Health>) {
    match state {
        "downloading" | "forcedDL" => ("Downloading", Some(Health::Up)),
        "uploading" | "forcedUP" => ("Seeding", Some(Health::Up)),
        "metaDL" | "forcedMetaDL" => ("Fetching metadata", Some(Health::Up)),
        "stalledDL" | "stalledUP" => ("Stalled", Some(Health::Warn)),
        "error" => ("Error", Some(Health::Down)),
        "missingFiles" => ("Missing files", Some(Health::Down)),
        "pausedDL" | "pausedUP" | "stoppedDL" | "stoppedUP" => ("Stopped", None),
        "queuedDL" | "queuedUP" => ("Queued", None),
        "checkingDL" | "checkingUP" | "checkingResumeData" => ("Checking", None),
        "allocating" => ("Allocating", None),
        "moving" => ("Moving", None),
        _ => ("Unknown", None),
    }
}

fn torrents<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    let filter = options
        .get("filter")
        .and_then(Value::as_str)
        .unwrap_or("active");
    Box::pin(async move {
        conn.retry_session(
            || login(conn),
            |session| async move {
                let session = session.ok_or(Class::Auth)?;
                let limit = TORRENT_ROWS.to_string();
                let sort = if filter == "seeding" {
                    "upspeed"
                } else {
                    "dlspeed"
                };
                let list: Vec<Torrent> = get(
                    conn,
                    &session,
                    "api/v2/torrents/info",
                    &[
                        ("filter", Input::Text(filter)),
                        ("sort", Input::Text(sort)),
                        ("reverse", Input::Text("true")),
                        ("limit", Input::Text(&limit)),
                    ],
                )
                .await?;
                if list.is_empty() {
                    return Ok(StructuredBody::Unavailable {
                        message: "No torrents to show.".into(),
                    });
                }
                let rows = list
                    .into_iter()
                    .take(TORRENT_ROWS)
                    .map(|torrent| torrent_row(conn, torrent))
                    .collect();
                Ok(StructuredBody::Rows { rows })
            },
        )
        .await
    })
}

/// qBittorrent reports 8640000 (100 days) for an unknown ETA.
const ETA_UNKNOWN: u64 = 8_640_000;

fn torrent_row(conn: &Conn<'_>, torrent: Torrent) -> BodyRow {
    let (label, status) = state(&torrent.state);
    let progress = (torrent.progress.clamp(0.0, 1.0) * 100.0).floor() as u8;
    let mut secondary = format!("{label} · {}", human_bytes(torrent.size));
    if progress < 100 && torrent.dlspeed > 0 && torrent.eta < ETA_UNKNOWN {
        secondary.push_str(&format!(" · {} left", human_duration(torrent.eta)));
    }
    let value = if torrent.dlspeed > 0 || progress < 100 {
        format!("↓ {}/s", human_bytes(torrent.dlspeed))
    } else {
        format!("↑ {}/s", human_bytes(torrent.upspeed))
    };
    BodyRow {
        title: display_text(conn, torrent.name),
        secondary: Some(secondary),
        value: Some(value),
        status,
        progress: Some(progress),
        bars: Vec::new(),
    }
}

fn unavailable() -> String {
    "Unavailable".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use serde_json::json;

    /// Tiles as `[label, value]` pairs.
    fn tiles(body: StructuredBody) -> serde_json::Value {
        let body = serde_json::to_value(body).unwrap();
        assert_eq!(body["kind"], "tiles");
        body["tiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tile| json!([tile["label"], tile["value"]]))
            .collect()
    }

    #[tokio::test]
    async fn lists_torrents_with_progress_state_and_speed() {
        const LOGIN: &str = "POST /prefix/api/v2/auth/login HTTP/1.1";
        const LIST: &str = "GET /prefix/api/v2/torrents/info?filter=active&sort=dlspeed&reverse=true&limit=10 HTTP/1.1";
        const SEEDING: &str = "GET /prefix/api/v2/torrents/info?filter=seeding&sort=upspeed&reverse=true&limit=10 HTTP/1.1";
        let list = format!(
            "[{},{},{}]",
            torrent(1, "uploading"),
            r#"{"name":"example-distro.iso","progress":0.625,"dlspeed":2097152,"upspeed":0,"eta":360,"state":"downloading","size":5368709120,"hash":"a"}"#,
            r#"{"name":"broken.tar","progress":0.1,"dlspeed":0,"upspeed":0,"eta":8640000,"state":"missingFiles","size":1024,"hash":"b"}"#,
        );
        let options = |filter: &str| {
            json!({ "filter": filter, "refresh": 60 })
                .as_object()
                .unwrap()
                .clone()
        };
        exercise(
            &QBITTORRENT,
            &[(USERNAME, "fake-user"), (PASSWORD, "fake-password")],
            Values::new(),
            &[
                Exchange { request: LOGIN, status: 200, body: "Ok." },
                Exchange { request: LIST, status: 200, body: &list },
                Exchange { request: SEEDING, status: 200, body: "[]" },
                Exchange { request: LIST, status: 200, body: r#"{"not":"a list"}"# },
                Exchange { request: LIST, status: 403, body: "" },
                Exchange { request: LOGIN, status: 200, body: "Fails." },
            ],
            |index, wire| match index {
                0 => "Set-Cookie: SID=fake-sid; Path=/\r\n",
                5 => "",
                _ => {
                    assert_header(wire, "cookie: SID=fake-sid", "fake-sid");
                    assert!(!wire.contains("fake-password") && !wire.contains("fake-user"));
                    ""
                }
            },
            async |conn| {
                let widget = &QBITTORRENT.widgets[1];
                let body = super::super::fetch(conn, widget, &options("active")).await.unwrap();
                let body = serde_json::to_value(body).unwrap();
                assert_eq!(body["kind"], "rows");
                assert_eq!(body["rows"][0], json!({
                    "title": "debian-1.iso é", "secondary": "Seeding · 1000 MiB",
                    "value": "↑ 2.0 KiB/s", "status": "up", "progress": 100,
                }));
                assert_eq!(body["rows"][1], json!({
                    "title": "example-distro.iso", "secondary": "Downloading · 5.0 GiB · 6m left",
                    "value": "↓ 2.0 MiB/s", "status": "up", "progress": 62,
                }));
                assert_eq!(body["rows"][2]["status"], "down");
                assert_eq!(body["rows"][2]["secondary"], "Missing files · 1.0 KiB");
                let empty = super::super::fetch(conn, widget, &options("seeding")).await.unwrap();
                assert_eq!(serde_json::to_value(empty).unwrap()["message"], "No torrents to show.");
                let fault = super::super::fetch(conn, widget, &options("active")).await.unwrap_err();
                assert!(matches!(fault.class, Class::Protocol));
                let fault = super::super::fetch(conn, widget, &options("active")).await.unwrap_err();
                assert!(matches!(fault.class, Class::Auth));
            },
        )
        .await;
    }

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
                    tiles(fetch().await.unwrap()),
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
            assert_eq!(tiles(body), json!([
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
