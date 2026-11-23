use super::*;
use crate::util::{human_bytes, human_duration};
use serde_json::json;
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

const SESSION_ID: &str = "x-transmission-session-id";

pub const TRANSMISSION: Provider = Provider {
    id: "transmission",
    name: "Transmission",
    category: "Downloads",
    icon: "transmission",
    color: "#c0392b",
    detect: super::detect(&["transmission"], 9091),
    description: "Show transfer speeds, torrent counts and torrents with their progress.",
    docs: "https://github.com/transmission/transmission/blob/4.0.6/docs/rpc-spec.md",
    public_internet: false,
    contacts: "Your configured Transmission RPC server.",
    fields: &[
        SERVICE_URL,
        OPTIONAL_BASIC_USER,
        OPTIONAL_BASIC_PASSWORD,
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "activity",
            name: "Torrent activity",
            description:
                "Download and upload speeds and the active, paused and total torrent counts.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Download","value":"5.2 MiB/s","glyph":"download"},{"label":"Upload","value":"640 KiB/s","glyph":"upload"},{"label":"Active","value":"2","glyph":"activity"},{"label":"Paused","value":"4","glyph":"clock"},{"label":"Torrents","value":"38","glyph":"database"}]}"#,
            fields: &[REFRESH],
            fetch: overview,
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
            sample: r#"{"kind":"rows","rows":[{"title":"example-distro-24.04.iso","secondary":"Downloading · 4.7 GiB · 6m left","value":"↓ 3.1 MiB/s","status":"up","progress":48},{"title":"sample-dataset.tar","secondary":"Seeding · 1.2 GiB","value":"↑ 220 KiB/s","status":"up","progress":100},{"title":"example-album-flac","secondary":"Stopped · 640 MiB","value":"—","progress":100}]}"#,
            fields: &[
                Field {
                    default: Some("active"),
                    choices: &["active", "all"],
                    ..Field::new(
                        "filter",
                        "Show",
                        Kind::Select,
                        "Recently active torrents, or all of them.",
                    )
                },
                REFRESH,
            ],
            fetch: torrents,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
struct Rpc<T> {
    result: String,
    arguments: Option<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stats {
    active_torrent_count: u64,
    paused_torrent_count: Option<u64>,
    torrent_count: Option<u64>,
    download_speed: f64,
    upload_speed: f64,
}

#[derive(Deserialize)]
struct Torrents {
    torrents: Vec<Torrent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Torrent {
    name: String,
    status: u8,
    #[serde(default)]
    percent_done: f64,
    #[serde(default)]
    rate_download: f64,
    #[serde(default)]
    rate_upload: f64,
    #[serde(default)]
    eta: i64,
    #[serde(default)]
    total_size: u64,
    #[serde(default)]
    error: u64,
}

async fn rpc<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    session: Option<&Session>,
    method: &str,
    arguments: Values,
) -> Result<T, Fault> {
    let mut auth: Vec<_> = optional_basic(conn)?.into_iter().collect();
    if let Some(session) = session {
        auth.push(Auth::Header(SESSION_ID, Input::Session(session)));
    }
    let fields = Values::from_iter([
        ("method".into(), method.into()),
        ("arguments".into(), Value::Object(arguments)),
    ]);
    let response = conn
        .request(Step {
            verb: Verb::Post,
            auth: &auth,
            body: Some(Body::Json(fields, &[])),
            response_headers: &[SESSION_ID],
            ..Step::get("transmission/rpc")
        })
        .await?;
    if response.status == 409 {
        return Err(Fault::session_conflict(
            conn.session_header(&response, SESSION_ID)?,
        ));
    }
    let rpc: Rpc<T> = parse_json(response)?;
    if rpc.result != "success" {
        return Err(Class::Protocol.into());
    }
    rpc.arguments.ok_or_else(|| Class::Protocol.into())
}

async fn stats(conn: &Conn<'_>, session: Option<&Session>) -> Result<Stats, Fault> {
    rpc(conn, session, "session-stats", Values::new()).await
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        conn.retry_session(
            || async { Ok(None) },
            |session| async move { stats(conn, session.as_ref()).await.map(|_| ()) },
        )
        .await
    })
}

fn rate(bytes_per_second: f64) -> Result<u64, Fault> {
    if !(0.0..u64::MAX as f64).contains(&bytes_per_second) {
        return Err(Class::Protocol.into());
    }
    Ok(bytes_per_second.round() as u64)
}

fn speed(bytes_per_second: f64) -> Result<String, Fault> {
    Ok(format!("{}/s", human_bytes(rate(bytes_per_second)?)))
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

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        conn.retry_session(
            || async { Ok(None) },
            |session| async move {
                let stats = stats(conn, session.as_ref()).await?;
                let mut tiles = vec![
                    tile("Download", speed(stats.download_speed)?, Glyph::Download),
                    tile("Upload", speed(stats.upload_speed)?, Glyph::Upload),
                    tile(
                        "Active",
                        stats.active_torrent_count.to_string(),
                        Glyph::Activity,
                    ),
                ];
                if let Some(paused) = stats.paused_torrent_count {
                    tiles.push(tile("Paused", paused.to_string(), Glyph::Clock));
                }
                if let Some(total) = stats.torrent_count {
                    tiles.push(tile("Torrents", total.to_string(), Glyph::Database));
                }
                Ok(StructuredBody::Tiles { tiles })
            },
        )
        .await
    })
}

const ROWS: usize = 10;
const FIELDS: &[&str] = &[
    "name",
    "status",
    "percentDone",
    "rateDownload",
    "rateUpload",
    "eta",
    "totalSize",
    "error",
];

fn torrents<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    let all = options.get("filter").and_then(Value::as_str) == Some("all");
    Box::pin(async move {
        conn.retry_session(
            || async { Ok(None) },
            |session| async move {
                let mut arguments = Values::from_iter([("fields".into(), json!(FIELDS))]);
                if !all {
                    arguments.insert("ids".into(), "recently-active".into());
                }
                let Some(list) = within_response_limit(
                    rpc::<Torrents>(conn, session.as_ref(), "torrent-get", arguments).await,
                )?
                else {
                    return Ok(StructuredBody::Unavailable {
                        message: "Torrent list too large to show.".into(),
                    });
                };
                let mut list = list.torrents;
                // Recently active includes torrents that just stopped.
                if !all {
                    list.retain(|torrent| torrent.status != 0);
                }
                list.sort_by(|a, b| {
                    (b.rate_download + b.rate_upload).total_cmp(&(a.rate_download + a.rate_upload))
                });
                if list.is_empty() {
                    return Ok(StructuredBody::Unavailable {
                        message: "No torrents to show.".into(),
                    });
                }
                let rows = list
                    .into_iter()
                    .take(ROWS)
                    .map(|torrent| row(conn, torrent))
                    .collect::<Result<_, Fault>>()?;
                Ok(StructuredBody::Rows { rows })
            },
        )
        .await
    })
}

fn row(conn: &Conn<'_>, torrent: Torrent) -> Result<BodyRow, Fault> {
    let (label, mut status) = match torrent.status {
        0 => ("Stopped", None),
        1 => ("Queued to verify", None),
        2 => ("Verifying", None),
        3 => ("Queued", None),
        4 => ("Downloading", Some(Health::Up)),
        5 => ("Queued to seed", None),
        6 => ("Seeding", Some(Health::Up)),
        _ => return Err(Class::Protocol.into()),
    };
    let label = if torrent.error != 0 {
        status = Some(Health::Down);
        "Error"
    } else {
        label
    };
    let progress = (torrent.percent_done.clamp(0.0, 1.0) * 100.0).floor() as u8;
    let mut secondary = format!("{label} · {}", human_bytes(torrent.total_size));
    // eta is -1 when not available and -2 when unknown.
    if torrent.status == 4 && torrent.eta > 0 {
        secondary.push_str(&format!(" · {} left", human_duration(torrent.eta as u64)));
    }
    let value = match torrent.status {
        4 => format!("↓ {}", speed(torrent.rate_download)?),
        6 => format!("↑ {}", speed(torrent.rate_upload)?),
        _ => "—".into(),
    };
    Ok(BodyRow {
        title: display_text(conn, torrent.name),
        secondary: Some(secondary),
        value: Some(value),
        status,
        progress: Some(progress),
        bars: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;

    const RPC: &str = "POST /prefix/transmission/rpc HTTP/1.1";
    const ENCODED: &str = "ZmFrZS11c2VyOmZha2UtcGFzcw==";
    const SECRETS: &[(&str, &str)] = &[("username", "fake-user"), ("password", "fake-pass")];

    fn tiles(body: Value) -> Value {
        body["tiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tile| json!([tile["label"], tile["value"]]))
            .collect()
    }

    #[tokio::test]
    async fn cold_rpc_retries_the_real_request_and_reads_session_stats() {
        const STATS: &str = r#"{"result":"success","arguments":{"downloadSpeed":1048576.0,"uploadSpeed":1024,"activeTorrentCount":5,"pausedTorrentCount":2,"torrentCount":9}}"#;
        exercise(&TRANSMISSION, SECRETS, Values::new(), &[
            Exchange { request: RPC, status: 409, body: "" },
            Exchange { request: RPC, status: 200, body: STATS },
            Exchange { request: RPC, status: 200, body: STATS },
            Exchange { request: RPC, status: 200, body: r#"{"result":"success","arguments":{"downloadSpeed":0,"uploadSpeed":0,"activeTorrentCount":0}}"# },
            Exchange { request: RPC, status: 200, body: r#"{"result":"success"}"# },
            Exchange { request: RPC, status: 401, body: "" },
        ], |index, wire| {
            let (head, body) = wire.split_once("\r\n\r\n").unwrap();
            assert_header(wire, &format!("authorization: Basic {ENCODED}"), ENCODED);
            assert!(!wire.contains("fake-user") && !wire.contains("fake-pass"));
            let body: Value = serde_json::from_str(body).unwrap();
            assert_eq!(body, json!({"method":"session-stats","arguments":{}}));
            if index == 0 {
                assert!(!head.contains(SESSION_ID));
                "X-Transmission-Session-Id: fake-transmission-sid\r\n"
            } else {
                assert_header(wire, "x-transmission-session-id: fake-transmission-sid", "fake-transmission-sid");
                ""
            }
        }, async |conn| {
            conn.run(test(conn)).await.unwrap();
            let fetch = async || {
                let body = super::super::fetch(conn, &TRANSMISSION.widgets[0], &Values::new()).await.unwrap();
                serde_json::to_value(body).unwrap()
            };
            assert_eq!(tiles(fetch().await), json!([
                ["Download","1.0 MiB/s"], ["Upload","1.0 KiB/s"], ["Active","5"], ["Paused","2"], ["Torrents","9"],
            ]));
            // Older daemons without the paused and total counts.
            assert_eq!(tiles(fetch().await), json!([["Download","0 B/s"], ["Upload","0 B/s"], ["Active","0"]]));
            assert_test_fault(conn, &TRANSMISSION, Class::Protocol).await;
            assert_test_fault(conn, &TRANSMISSION, Class::Auth).await;
        }).await;
    }

    #[tokio::test]
    async fn lists_torrents_fastest_first_and_bounds_large_lists() {
        let fields = json!(FIELDS);
        let list = r#"{"result":"success","arguments":{"removed":[],"torrents":[
            {"name":"example-album-flac","status":6,"percentDone":1,"rateDownload":0,"rateUpload":225280,"eta":-1,"totalSize":671088640,"error":0},
            {"name":"just-stopped.iso","status":0,"percentDone":0.5,"rateDownload":0,"rateUpload":0,"eta":-1,"totalSize":1024,"error":0},
            {"name":"example-distro.iso","status":4,"percentDone":0.485,"rateDownload":3250585,"rateUpload":0,"eta":360,"totalSize":5046586573,"error":0},
            {"name":"broken.tar","status":4,"percentDone":0.1,"rateDownload":0,"rateUpload":0,"eta":-2,"totalSize":2048,"error":3}]}}"#;
        let oversized = format!(r#"{{"padding":"{}"}}"#, "x".repeat(MAX_RESPONSE));
        let options = |filter: &str| json!({"filter": filter}).as_object().unwrap().clone();
        exercise(&TRANSMISSION, SECRETS, Values::new(), &[
            Exchange { request: RPC, status: 409, body: "" },
            Exchange { request: RPC, status: 200, body: list },
            Exchange { request: RPC, status: 200, body: &oversized },
            Exchange { request: RPC, status: 200, body: r#"{"result":"success","arguments":{"torrents":[{"name":"odd","status":9}]}}"# },
            Exchange { request: RPC, status: 401, body: "" },
        ], |index, wire| {
            let (_, body) = wire.split_once("\r\n\r\n").unwrap();
            assert_header(wire, &format!("authorization: Basic {ENCODED}"), ENCODED);
            let body: Value = serde_json::from_str(body).unwrap();
            if index == 2 {
                assert_eq!(body, json!({"method":"torrent-get","arguments":{"fields":fields}}));
            } else {
                assert_eq!(body, json!({"method":"torrent-get","arguments":{"ids":"recently-active","fields":fields}}));
            }
            if index == 0 { "X-Transmission-Session-Id: fake-transmission-sid\r\n" } else { "" }
        }, async |conn| {
            let widget = &TRANSMISSION.widgets[1];
            let body = super::super::fetch(conn, widget, &options("active")).await.unwrap();
            let body = serde_json::to_value(body).unwrap();
            assert_eq!(body["rows"], json!([
                {"title":"example-distro.iso","secondary":"Downloading · 4.7 GiB · 6m left","value":"↓ 3.1 MiB/s","status":"up","progress":48},
                {"title":"example-album-flac","secondary":"Seeding · 640 MiB","value":"↑ 220 KiB/s","status":"up","progress":100},
                {"title":"broken.tar","secondary":"Error · 2.0 KiB","value":"↓ 0 B/s","status":"down","progress":10},
            ]));
            let large = super::super::fetch(conn, widget, &options("all")).await.unwrap();
            assert_eq!(serde_json::to_value(large).unwrap()["message"], "Torrent list too large to show.");
            let fault = super::super::fetch(conn, widget, &options("active")).await.unwrap_err();
            assert!(matches!(fault.class, Class::Protocol));
            let fault = super::super::fetch(conn, widget, &options("active")).await.unwrap_err();
            assert!(matches!(fault.class, Class::Auth));
        }).await;
    }
}
