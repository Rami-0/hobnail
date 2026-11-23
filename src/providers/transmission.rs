use super::*;
use crate::util::human_bytes;
use serde_json::json;
use widgets::StructuredBody;

const SESSION_ID: &str = "x-transmission-session-id";

pub const TRANSMISSION: Provider = Provider {
    id: "transmission",
    name: "Transmission",
    category: "Downloads",
    icon: "transmission",
    color: "#c0392b",
    description: "Show transfer speeds, the active torrent count and recent active torrents.",
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
    widgets: &[Widget {
        id: "activity",
        name: "Torrent activity",
        description:
            "Download and upload speeds, active torrent count and recently active torrent names.",
        recommended: true,
        default_size: size(4, 2),
        min_size: size(3, 2),
        sample: r#"{"kind":"stat_grid","rows":[["Download","5.2 MB/s"],["Upload","640 kB/s"],["Active","2"],["Paused","4"]]}"#,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
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
    download_speed: f64,
    upload_speed: f64,
}

#[derive(Deserialize)]
struct Torrents {
    torrents: Vec<Torrent>,
}

#[derive(Deserialize)]
struct Torrent {
    name: String,
    status: u8,
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

fn speed(bytes_per_second: f64) -> Result<String, Fault> {
    if bytes_per_second < 0.0 || bytes_per_second >= u64::MAX as f64 {
        return Err(Class::Protocol.into());
    }
    Ok(format!(
        "{}/s",
        human_bytes(bytes_per_second.round() as u64)
    ))
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        conn.retry_session(
            || async { Ok(None) },
            |session| async move {
                let stats = stats(conn, session.as_ref()).await?;
                let mut rows = vec![
                    ("Download".into(), speed(stats.download_speed)?),
                    ("Upload".into(), speed(stats.upload_speed)?),
                    (
                        "Active torrents".into(),
                        stats.active_torrent_count.to_string(),
                    ),
                ];
                if stats.active_torrent_count > 0 {
                    let arguments = Values::from_iter([
                        ("fields".into(), json!(["name", "status"])),
                        ("ids".into(), "recently-active".into()),
                    ]);
                    let recent = within_response_limit(
                        rpc::<Torrents>(conn, session.as_ref(), "torrent-get", arguments).await,
                    )?;
                    match recent {
                        Some(recent) => rows.extend(
                            recent
                                .torrents
                                .into_iter()
                                .filter_map(|torrent| {
                                    let state = match torrent.status {
                                        2 => "Verifying",
                                        4 => "Downloading",
                                        6 => "Seeding",
                                        _ => return None,
                                    };
                                    Some((display_text(conn, torrent.name), state.into()))
                                })
                                .take(10),
                        ),
                        None => {
                            rows.push(("Recently active torrents".into(), "Unavailable".into()))
                        }
                    }
                }
                Ok(StructuredBody::StatGrid { rows })
            },
        )
        .await
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;

    #[tokio::test]
    async fn cold_rpc_retries_the_real_request_and_preserves_stats_when_details_exceed_cap() {
        const RPC: &str = "POST /prefix/transmission/rpc HTTP/1.1";
        const STATS: &str = r#"{"result":"success","arguments":{"downloadSpeed":1048576.0,"uploadSpeed":1024,"activeTorrentCount":5}}"#;
        let oversized = format!(r#"{{"padding":"{}"}}"#, "x".repeat(MAX_RESPONSE));
        exercise(&TRANSMISSION, &[("username","fake-user"), ("password","fake-pass")], Values::new(), &[
            Exchange { request: RPC, status: 409, body: "" },
            Exchange { request: RPC, status: 200, body: STATS },
            Exchange { request: RPC, status: 200, body: STATS },
            Exchange { request: RPC, status: 200, body: r#"{"result":"success","arguments":{"torrents":[{"name":"Active file","status":4},{"name":"Stopped file","status":0}]}}"# },
            Exchange { request: RPC, status: 200, body: STATS },
            Exchange { request: RPC, status: 200, body: &oversized },
            Exchange { request: RPC, status: 200, body: r#"{"result":"success"}"# },
            Exchange { request: RPC, status: 401, body: "" },
        ], |index, wire| {
            let (head, body) = wire.split_once("\r\n\r\n").unwrap();
            const ENCODED: &str = "ZmFrZS11c2VyOmZha2UtcGFzcw==";
            assert_header(wire, &format!("authorization: Basic {ENCODED}"), ENCODED);
            assert!(!wire.contains("fake-user") && !wire.contains("fake-pass"));
            let body: Value = serde_json::from_str(body).unwrap();
            if [3, 5].contains(&index) {
                assert_eq!(body, json!({"method":"torrent-get","arguments":{"ids":"recently-active","fields":["name","status"]}}));
            } else {
                assert_eq!(body, json!({"method":"session-stats","arguments":{}}));
            }
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
            let mut rows = json!([["Download","1.0 MiB/s"], ["Upload","1.0 KiB/s"], ["Active torrents","5"], ["Active file","Downloading"]]);
            assert_eq!(fetch().await["rows"], rows);
            rows[3] = json!(["Recently active torrents","Unavailable"]);
            assert_eq!(fetch().await["rows"], rows);
            assert_test_fault(conn, &TRANSMISSION, Class::Protocol).await;
            assert_test_fault(conn, &TRANSMISSION, Class::Auth).await;
        }).await;
    }
}
