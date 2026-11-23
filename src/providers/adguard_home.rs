use super::pihole::grouped;
use super::*;
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

const USER: &str = "username";
const PASSWORD: &str = "password";

pub const ADGUARD_HOME: Provider = Provider {
    id: "adguard_home",
    name: "AdGuard Home",
    category: "DNS",
    icon: "adguard-home",
    color: "#5fb36b",
    description: "Show DNS queries, blocked queries, protection status and the top domains and clients.",
    docs: "https://github.com/AdguardTeam/AdGuardHome/blob/v0.107.73/openapi/openapi.yaml",
    public_internet: false,
    contacts: "Your configured AdGuard Home Web UI.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(
                USER,
                "Username",
                Kind::Secret,
                "The AdGuard Home web interface login (AdGuardHome.yaml → users).",
            )
        },
        Field {
            required: true,
            max_length: Some(4096),
            ..Field::new(PASSWORD, "Password", Kind::Secret, "The password of that login.")
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "dns",
            name: "DNS protection",
            description: "Queries and filtering blocks over the statistics period, the share blocked, the average processing time, protection status and a query trend.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Queries","value":"31,902","glyph":"activity","series":[820,760,640,590,610,880,1210,1530,1640,1590,1510,1480,1520,1610,1700,1660,1580,1490,1420,1380,1300,1180,1020,910]},{"label":"Blocked","value":"4,410","glyph":"shield"},{"label":"Blocked share","value":"13.8%","glyph":"shield"},{"label":"Avg. time","value":"12 ms","glyph":"clock"},{"label":"Protection","value":"On","glyph":"check","tone":"up"}]}"#,
            fields: &[REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "top_queried",
            name: "Top queried domains",
            description: "The ten most queried domains over the statistics period.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"api.example.com","value":"3,904","progress":100},{"title":"cdn.example.net","value":"2,211","progress":56},{"title":"time.example.org","value":"960","progress":24}]}"#,
            fields: &[REFRESH],
            fetch: top_queried,
            validate: no_widget_options,
        },
        Widget {
            id: "top_blocked",
            name: "Top blocked domains",
            description: "The ten domains filtering rules blocked most often.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"ads.example.net","value":"1,820","progress":100,"status":"down"},{"title":"tracker.example.com","value":"902","progress":49,"status":"down"},{"title":"pixel.example.org","value":"311","progress":17,"status":"down"}]}"#,
            fields: &[REFRESH],
            fetch: top_blocked,
            validate: no_widget_options,
        },
        Widget {
            id: "top_clients",
            name: "Top clients",
            description: "The ten clients that sent the most queries.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"192.0.2.10","value":"9,480","progress":100},{"title":"192.0.2.21","value":"5,012","progress":52},{"title":"192.0.2.33","value":"1,730","progress":18}]}"#,
            fields: &[REFRESH],
            fetch: top_clients,
            validate: no_widget_options,
        },
    ],
};

async fn get<T: serde::de::DeserializeOwned>(conn: &Conn<'_>, path: &str) -> Result<T, Fault> {
    parse_json(
        conn.request(Step {
            auth: &[Auth::Basic(
                Input::Credential(USER),
                Input::Credential(PASSWORD),
            )],
            ..Step::get(path)
        })
        .await?,
    )
}

#[derive(Deserialize)]
struct Status {
    protection_enabled: bool,
}

#[derive(Deserialize)]
struct Stats {
    num_dns_queries: u64,
    num_blocked_filtering: u64,
    #[serde(default)]
    avg_processing_time: Option<f64>,
    #[serde(default)]
    dns_queries: Vec<u64>,
    #[serde(default)]
    top_queried_domains: Vec<Map<String, Value>>,
    #[serde(default)]
    top_blocked_domains: Vec<Map<String, Value>>,
    #[serde(default)]
    top_clients: Vec<Map<String, Value>>,
}

#[derive(Deserialize)]
struct StatsConfig {
    enabled: bool,
}

const STATS_OFF: &str = "Statistics are turned off in AdGuard Home.";

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { get::<Status>(conn, "control/status").await.map(|_| ()) })
}

/// The statistics, or None when AdGuard Home does not record them.
async fn stats(conn: &Conn<'_>) -> Result<Option<Stats>, Fault> {
    let config: StatsConfig = get(conn, "control/stats/config").await?;
    if !config.enabled {
        return Ok(None);
    }
    get(conn, "control/stats").await.map(Some)
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
        let status: Status = get(conn, "control/status").await?;
        let mut tiles = match stats(conn).await? {
            Some(stats) => {
                let percent = if stats.num_dns_queries == 0 {
                    "—".into()
                } else {
                    format!(
                        "{:.1}%",
                        stats.num_blocked_filtering as f64 / stats.num_dns_queries as f64 * 100.0
                    )
                };
                let mut tiles = vec![
                    StatTile {
                        series: stats.dns_queries.iter().map(|&n| n as f32).collect(),
                        ..tile("Queries", grouped(stats.num_dns_queries), Glyph::Activity)
                    },
                    tile(
                        "Blocked",
                        grouped(stats.num_blocked_filtering),
                        Glyph::Shield,
                    ),
                    tile("Blocked share", percent, Glyph::Shield),
                ];
                if let Some(seconds) = stats.avg_processing_time.filter(|s| s.is_finite()) {
                    tiles.push(tile(
                        "Avg. time",
                        format!("{} ms", (seconds * 1000.0).round().max(0.0) as u64),
                        Glyph::Clock,
                    ));
                }
                tiles
            }
            None => vec![tile("Statistics", "Off".into(), Glyph::Activity)],
        };
        tiles.push(StatTile {
            tone: Some(if status.protection_enabled {
                Health::Up
            } else {
                Health::Warn
            }),
            ..tile(
                "Protection",
                if status.protection_enabled {
                    "On"
                } else {
                    "Off"
                }
                .into(),
                if status.protection_enabled {
                    Glyph::Check
                } else {
                    Glyph::Alert
                },
            )
        });
        Ok(StructuredBody::Tiles { tiles })
    })
}

#[derive(Clone, Copy)]
enum Top {
    Queried,
    Blocked,
    Clients,
}

fn top_queried<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(top(conn, Top::Queried))
}

fn top_blocked<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(top(conn, Top::Blocked))
}

fn top_clients<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(top(conn, Top::Clients))
}

/// A top list: each entry is a one-key object, `{"<domain or client>": count}`.
async fn top(conn: &Conn<'_>, which: Top) -> Result<StructuredBody, Fault> {
    let Some(stats) = stats(conn).await? else {
        return Ok(StructuredBody::Unavailable {
            message: STATS_OFF.into(),
        });
    };
    let entries = match which {
        Top::Queried => stats.top_queried_domains,
        Top::Blocked => stats.top_blocked_domains,
        Top::Clients => stats.top_clients,
    };
    let mut ranked = Vec::with_capacity(entries.len().min(10));
    for entry in entries.into_iter().take(10) {
        let (key, count) = entry.into_iter().next().ok_or(Class::Protocol)?;
        let count = count
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .ok_or(Class::Protocol)? as u64;
        ranked.push((key, count));
    }
    if ranked.is_empty() {
        return Ok(StructuredBody::Unavailable {
            message: "No queries recorded".into(),
        });
    }
    let max = ranked.iter().map(|(_, n)| *n).max().unwrap_or(0).max(1);
    Ok(StructuredBody::Rows {
        rows: ranked
            .into_iter()
            .map(|(key, count)| BodyRow {
                title: display_text(conn, key),
                value: Some(grouped(count)),
                progress: Some((count.saturating_mul(100) / max).min(100) as u8),
                status: matches!(which, Top::Blocked).then_some(Health::Down),
                ..Default::default()
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    const STATUS: &str = "GET /prefix/control/status HTTP/1.1";
    const CONFIG: &str = "GET /prefix/control/stats/config HTTP/1.1";
    const STATS: &str = "GET /prefix/control/stats HTTP/1.1";
    const ACTIVE: &str = r#"{"protection_enabled":true}"#;
    const ON: &str = r#"{"enabled":true,"interval":86400000,"ignored":[]}"#;
    const OFF: &str = r#"{"enabled":false,"interval":86400000,"ignored":[]}"#;
    // Shape from openapi.yaml `Stats` / `TopArrayEntry`.
    const FULL: &str = r#"{"time_units":"hours","num_dns_queries":40,"num_blocked_filtering":5,"num_replaced_safebrowsing":0,"num_replaced_safesearch":0,"num_replaced_parental":0,"avg_processing_time":0.0123,"top_queried_domains":[{"example.com":30},{"example.org":10}],"top_clients":[{"192.0.2.10":36},{"192.0.2.11":4}],"top_blocked_domains":[{"ads.example.net":5}],"top_upstreams_responses":[],"top_upstreams_avg_time":[],"dns_queries":[1,2,37],"blocked_filtering":[0,1,4],"replaced_safebrowsing":[0,0,0],"replaced_parental":[0,0,0]}"#;

    fn credentials(wire: &str) {
        const ENCODED: &str = "ZmFrZS11c2VyOmZha2UtcGFzcw==";
        assert_header(wire, &format!("authorization: Basic {ENCODED}"), ENCODED);
        assert!(!wire.contains("fake-user") && !wire.contains("fake-pass"));
    }

    #[tokio::test]
    async fn authenticates_with_basic_and_reads_dns_metrics() {
        exercise(
            &ADGUARD_HOME,
            &[(USER, "fake-user"), (PASSWORD, "fake-pass")],
            Values::new(),
            &[
                Exchange {
                    request: STATUS,
                    status: 200,
                    body: ACTIVE,
                },
                Exchange {
                    request: STATUS,
                    status: 200,
                    body: ACTIVE,
                },
                Exchange {
                    request: CONFIG,
                    status: 200,
                    body: ON,
                },
                Exchange {
                    request: STATS,
                    status: 200,
                    body: FULL,
                },
                Exchange {
                    request: STATUS,
                    status: 200,
                    body: r#"{"protection_enabled":false}"#,
                },
                Exchange {
                    request: CONFIG,
                    status: 200,
                    body: OFF,
                },
                Exchange {
                    request: STATUS,
                    status: 401,
                    body: "",
                },
                Exchange {
                    request: STATUS,
                    status: 200,
                    body: "{}",
                },
            ],
            |_, wire| {
                credentials(wire);
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let body = super::super::fetch(conn, &ADGUARD_HOME.widgets[0], &Values::new())
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(body).unwrap(),
                    json!({"kind":"tiles","tiles":[
                        {"label":"Queries","value":"40","glyph":"activity","series":[1.0,2.0,37.0]},
                        {"label":"Blocked","value":"5","glyph":"shield"},
                        {"label":"Blocked share","value":"12.5%","glyph":"shield"},
                        {"label":"Avg. time","value":"12 ms","glyph":"clock"},
                        {"label":"Protection","value":"On","glyph":"check","tone":"up"},
                    ]})
                );
                let disabled = super::super::fetch(conn, &ADGUARD_HOME.widgets[0], &Values::new())
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(disabled).unwrap()["tiles"],
                    json!([
                        {"label":"Statistics","value":"Off","glyph":"activity"},
                        {"label":"Protection","value":"Off","glyph":"alert","tone":"warn"},
                    ])
                );
                assert_test_fault(conn, &ADGUARD_HOME, Class::Auth).await;
                assert_test_fault(conn, &ADGUARD_HOME, Class::Protocol).await;
            },
        )
        .await;
    }

    #[tokio::test]
    async fn ranks_top_lists_and_reports_disabled_statistics_and_bad_entries() {
        exercise(
            &ADGUARD_HOME,
            &[(USER, "fake-user"), (PASSWORD, "fake-pass")],
            Values::new(),
            &[
                Exchange { request: CONFIG, status: 200, body: ON },
                Exchange { request: STATS, status: 200, body: FULL },
                Exchange { request: CONFIG, status: 200, body: ON },
                Exchange { request: STATS, status: 200, body: FULL },
                Exchange { request: CONFIG, status: 200, body: ON },
                Exchange { request: STATS, status: 200, body: FULL },
                Exchange { request: CONFIG, status: 200, body: OFF },
                Exchange { request: CONFIG, status: 200, body: ON },
                Exchange { request: STATS, status: 200, body: r#"{"num_dns_queries":1,"num_blocked_filtering":0,"top_queried_domains":[{}]}"# },
                Exchange { request: CONFIG, status: 401, body: "" },
            ],
            |_, wire| {
                credentials(wire);
                ""
            },
            async |conn| {
                let empty = Values::new();
                let fetch = |index: usize| super::super::fetch(conn, &ADGUARD_HOME.widgets[index], &empty);
                assert_eq!(
                    serde_json::to_value(fetch(1).await.unwrap()).unwrap(),
                    json!({"kind":"rows","rows":[
                        {"title":"example.com","value":"30","progress":100},
                        {"title":"example.org","value":"10","progress":33},
                    ]})
                );
                assert_eq!(
                    serde_json::to_value(fetch(2).await.unwrap()).unwrap()["rows"],
                    json!([{"title":"ads.example.net","value":"5","progress":100,"status":"down"}])
                );
                assert_eq!(
                    serde_json::to_value(fetch(3).await.unwrap()).unwrap()["rows"][1],
                    json!({"title":"192.0.2.11","value":"4","progress":11})
                );
                assert_eq!(
                    serde_json::to_value(fetch(1).await.unwrap()).unwrap()["message"],
                    STATS_OFF
                );
                assert_eq!(fetch(1).await.unwrap_err().class.message(), Class::Protocol.message());
                assert_eq!(fetch(1).await.unwrap_err().class.message(), Class::Auth.message());
            },
        )
        .await;
    }
}
