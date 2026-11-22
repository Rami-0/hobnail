use super::*;
use widgets::StructuredBody;

const USER: &str = "username";
const PASSWORD: &str = "password";

pub const ADGUARD_HOME: Provider = Provider {
    id: "adguard_home",
    name: "AdGuard Home",
    category: "DNS",
    icon: "adguard-home",
    description: "Show DNS queries, blocked queries and protection status.",
    docs: "https://github.com/AdguardTeam/AdGuardHome/blob/v0.107.73/openapi/openapi.yaml",
    public_internet: false,
    contacts: "Your configured AdGuard Home Web UI.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(USER, "Username", Kind::Secret, "")
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
        id: "dns",
        name: "DNS protection",
        description: "DNS queries, filtering blocks and current protection status.",
        recommended: true,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
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
}

#[derive(Deserialize)]
struct StatsConfig {
    enabled: bool,
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { get::<Status>(conn, "control/status").await.map(|_| ()) })
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let status: Status = get(conn, "control/status").await?;
        let config: StatsConfig = get(conn, "control/stats/config").await?;
        let (queries, blocked, percent) = if config.enabled {
            let stats: Stats = get(conn, "control/stats").await?;
            let percent = if stats.num_dns_queries == 0 {
                "—".into()
            } else {
                format!(
                    "{:.1}%",
                    stats.num_blocked_filtering as f64 / stats.num_dns_queries as f64 * 100.0
                )
            };
            (
                stats.num_dns_queries.to_string(),
                stats.num_blocked_filtering.to_string(),
                percent,
            )
        } else {
            (
                "Unavailable".into(),
                "Unavailable".into(),
                "Unavailable".into(),
            )
        };
        Ok(StructuredBody::StatGrid {
            rows: vec![
                ("Queries".into(), queries),
                ("Blocked by filters".into(), blocked),
                ("Blocked %".into(), percent),
                (
                    "Protection".into(),
                    if status.protection_enabled {
                        "On"
                    } else {
                        "Off"
                    }
                    .into(),
                ),
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
    async fn authenticates_with_basic_and_reads_dns_metrics() {
        const STATUS: &str = "GET /prefix/control/status HTTP/1.1";
        const ACTIVE: &str = r#"{"protection_enabled":true}"#;
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
                    request: "GET /prefix/control/stats/config HTTP/1.1",
                    status: 200,
                    body: r#"{"enabled":true,"interval":86400000,"ignored":[]}"#,
                },
                Exchange {
                    request: "GET /prefix/control/stats HTTP/1.1",
                    status: 200,
                    body: r#"{"num_dns_queries":40,"num_blocked_filtering":5}"#,
                },
                Exchange {
                    request: STATUS,
                    status: 200,
                    body: ACTIVE,
                },
                Exchange {
                    request: "GET /prefix/control/stats/config HTTP/1.1",
                    status: 200,
                    body: r#"{"enabled":false,"interval":86400000,"ignored":[]}"#,
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
                const ENCODED: &str = "ZmFrZS11c2VyOmZha2UtcGFzcw==";
                assert_header(wire, &format!("authorization: Basic {ENCODED}"), ENCODED);
                assert!(!wire.contains("fake-user") && !wire.contains("fake-pass"));
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let body = super::super::fetch(conn, &ADGUARD_HOME.widgets[0], &Values::new())
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(body).unwrap()["rows"],
                    json!([
                        ["Queries", "40"],
                        ["Blocked by filters", "5"],
                        ["Blocked %", "12.5%"],
                        ["Protection", "On"],
                    ])
                );
                let disabled = super::super::fetch(conn, &ADGUARD_HOME.widgets[0], &Values::new())
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(disabled).unwrap()["rows"],
                    json!([
                        ["Queries", "Unavailable"],
                        ["Blocked by filters", "Unavailable"],
                        ["Blocked %", "Unavailable"],
                        ["Protection", "On"]
                    ])
                );
                assert_test_fault(conn, &ADGUARD_HOME, Class::Auth).await;
                assert_test_fault(conn, &ADGUARD_HOME, Class::Protocol).await;
            },
        )
        .await;
    }
}
