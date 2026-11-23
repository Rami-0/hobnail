use super::*;
use crate::util::{fmt_utc, unix_now};
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

pub const PROWLARR: Provider = Provider {
    id: "prowlarr",
    name: "Prowlarr",
    category: "Media",
    icon: "prowlarr",
    color: "#d4541e",
    description: "Show indexer queries, grabs and failures, and blocked indexers.",
    docs: "https://prowlarr.com/docs/api/",
    public_internet: false,
    contacts: "Your configured Prowlarr service. Public addresses require internet access.",
    fields: arr::FIELDS,
    test,
    widgets: &[
        Widget {
            id: "indexers",
            name: "Indexer activity",
            description: "Blocked indexers, and queries, grabs and failed grabs over the last seven days.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Blocked","value":"1","glyph":"alert","tone":"warn"},{"label":"Queries (7 days)","value":"4,212","glyph":"activity"},{"label":"Grabs (7 days)","value":"38","glyph":"download"},{"label":"Failed grabs","value":"0","glyph":"check","tone":"up"}]}"#,
            fields: &[REFRESH],
            fetch: indexers,
            validate: no_widget_options,
        },
        Widget {
            id: "status",
            name: "Indexer status",
            description: "The ten busiest indexers this week: queries, grabs, response time, failures and blocks.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"Example Indexer","secondary":"24 grabs · 310 ms","value":"1,840 queries","status":"up"},{"title":"Sample Tracker","secondary":"Blocked until 2026-11-24 02:00","value":"412 queries","status":"down"},{"title":"Another Index","secondary":"6 grabs · 12 failed queries","value":"230 queries","status":"warn"}]}"#,
            fields: &[REFRESH],
            fetch: status,
            validate: no_widget_options,
        },
    ],
};

const STATUS: &str = "api/v1/indexerstatus";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Blocked {
    indexer_id: u32,
    disabled_till: Option<String>,
}

#[derive(Deserialize)]
struct Stats {
    indexers: Vec<Indexer>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Indexer {
    indexer_id: u32,
    indexer_name: Option<String>,
    #[serde(default)]
    average_response_time: u32,
    #[serde(default)]
    number_of_queries: u32,
    number_of_grabs: u32,
    #[serde(default)]
    number_of_failed_queries: u32,
    number_of_failed_grabs: u32,
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        arr::get::<Vec<Blocked>>(conn, STATUS, &[])
            .await
            .map(|_| ())
    })
}

/// Blocked indexers, and the last seven days of statistics when they fit.
async fn read(conn: &Conn<'_>) -> Result<(Vec<Blocked>, Option<Stats>), Fault> {
    let blocked: Vec<Blocked> = arr::get(conn, STATUS, &[]).await?;
    let now = unix_now();
    let start = fmt_utc(now.saturating_sub(7 * 86_400));
    let end = fmt_utc(now);
    let stats = within_response_limit(
        arr::get::<Stats>(
            conn,
            "api/v1/indexerstats",
            &[
                ("startDate", Input::Text(&start)),
                ("endDate", Input::Text(&end)),
            ],
        )
        .await,
    )?;
    Ok((blocked, stats))
}

fn tile(label: &str, value: String, glyph: Glyph, tone: Option<Health>) -> StatTile {
    StatTile {
        label: label.into(),
        value,
        glyph: Some(glyph),
        delta: None,
        tone,
        series: Vec::new(),
    }
}

fn indexers<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let (blocked, stats) = read(conn).await?;
        let total = |metric: fn(&Indexer) -> u32| {
            stats.as_ref().map(|stats| {
                stats
                    .indexers
                    .iter()
                    .map(|indexer| u64::from(metric(indexer)))
                    .sum::<u64>()
            })
        };
        let text = |count: Option<u64>| {
            count.map_or_else(|| "Unavailable".into(), super::jellyfin::grouped)
        };
        let failed = total(|i| i.number_of_failed_grabs);
        Ok(StructuredBody::Tiles {
            tiles: vec![
                tile(
                    "Blocked",
                    blocked.len().to_string(),
                    Glyph::Alert,
                    Some(if blocked.is_empty() {
                        Health::Up
                    } else {
                        Health::Warn
                    }),
                ),
                tile(
                    "Queries (7 days)",
                    text(total(|i| i.number_of_queries)),
                    Glyph::Activity,
                    None,
                ),
                tile(
                    "Grabs (7 days)",
                    text(total(|i| i.number_of_grabs)),
                    Glyph::Download,
                    None,
                ),
                tile(
                    "Failed grabs",
                    text(failed),
                    if failed.unwrap_or(0) > 0 {
                        Glyph::Alert
                    } else {
                        Glyph::Check
                    },
                    failed.map(|n| if n > 0 { Health::Warn } else { Health::Up }),
                ),
            ],
        })
    })
}

fn status<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let (blocked, stats) = read(conn).await?;
        let Some(mut stats) = stats else {
            return Ok(StructuredBody::Unavailable {
                message: "Indexer statistics are too large to show.".into(),
            });
        };
        if stats.indexers.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No indexer activity in the last seven days.".into(),
            });
        }
        // Blocked first, then the busiest.
        let blocked_until = |id: u32| blocked.iter().find(|b| b.indexer_id == id);
        stats.indexers.sort_by_key(|i| {
            (
                blocked_until(i.indexer_id).is_none(),
                std::cmp::Reverse(i.number_of_queries),
            )
        });
        let rows = stats
            .indexers
            .into_iter()
            .take(10)
            .map(|indexer| {
                let block = blocked_until(indexer.indexer_id);
                let grabs = indexer.number_of_grabs;
                let mut parts = vec![format!("{grabs} grab{}", if grabs == 1 { "" } else { "s" })];
                if indexer.number_of_failed_queries > 0 {
                    parts.push(format!(
                        "{} failed queries",
                        indexer.number_of_failed_queries
                    ));
                } else if indexer.average_response_time > 0 {
                    parts.push(format!("{} ms", indexer.average_response_time));
                }
                let secondary = match block {
                    Some(block) => match block.disabled_till.as_deref() {
                        Some(until) if until.len() >= 16 => {
                            format!("Blocked until {} {}", &until[..10], &until[11..16])
                        }
                        _ => "Blocked".into(),
                    },
                    None => parts.join(" · "),
                };
                BodyRow {
                    title: indexer.indexer_name.map_or_else(
                        || format!("Indexer #{}", indexer.indexer_id),
                        |name| display_text(conn, name),
                    ),
                    secondary: Some(display_text(conn, secondary)),
                    value: Some(format!(
                        "{} queries",
                        super::jellyfin::grouped(u64::from(indexer.number_of_queries))
                    )),
                    status: Some(if block.is_some() {
                        Health::Down
                    } else if indexer.number_of_failed_queries > 0
                        || indexer.number_of_failed_grabs > 0
                    {
                        Health::Warn
                    } else {
                        Health::Up
                    }),
                    ..BodyRow::default()
                }
            })
            .collect();
        Ok(StructuredBody::Rows { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use serde_json::json;

    #[tokio::test]
    async fn reads_activity_and_indexer_status_with_header_auth() {
        const STATUS: &str = "GET /prefix/api/v1/indexerstatus HTTP/1.1";
        const STATS: &str = "GET /prefix/api/v1/indexerstats?startDate=*";
        const KEY: &str = "fake-prowlarr-api-key";
        const BLOCKED: &str = r#"[{"id":2,"indexerId":9,"disabledTill":"2026-11-22T01:00:00Z"}]"#;
        const STATS_BODY: &str = r#"{"id":0,"indexers":[
            {"indexerId":9,"indexerName":"Fixture Index","averageResponseTime":200,"numberOfQueries":100,"numberOfGrabs":30,"numberOfFailedQueries":0,"numberOfFailedGrabs":2},
            {"indexerId":12,"indexerName":"Another Index","averageResponseTime":310,"numberOfQueries":1500,"numberOfGrabs":4,"numberOfFailedQueries":0,"numberOfFailedGrabs":0},
            {"indexerId":13,"indexerName":"Flaky Index","averageResponseTime":900,"numberOfQueries":20,"numberOfGrabs":1,"numberOfFailedQueries":5,"numberOfFailedGrabs":1}
        ],"userAgents":[],"hosts":[]}"#;
        let oversized = format!(
            r#"{{"indexers":[],"userAgents":[],"hosts":[]{} }}"#,
            " ".repeat(128 * 1024)
        );
        exercise(&PROWLARR, &[("api_key", KEY)], Values::new(), &[
            Exchange { request: STATUS, status: 200, body: BLOCKED },
            Exchange { request: STATUS, status: 200, body: BLOCKED },
            Exchange { request: STATS, status: 200, body: STATS_BODY },
            Exchange { request: STATUS, status: 200, body: "[]" },
            Exchange { request: STATS, status: 200, body: &oversized },
            Exchange { request: STATUS, status: 200, body: BLOCKED },
            Exchange { request: STATS, status: 200, body: STATS_BODY },
            Exchange { request: STATUS, status: 200, body: BLOCKED },
            Exchange { request: STATS, status: 200, body: &oversized },
            Exchange { request: STATUS, status: 200, body: BLOCKED },
            Exchange { request: STATS, status: 200, body: r#"{"indexers":"none"}"# },
            Exchange { request: STATUS, status: 401, body: "" },
            Exchange { request: STATUS, status: 200, body: r#"[{"id":2}]"# },
        ], |index, wire| {
            assert_header(wire, &format!("x-api-key: {KEY}"), KEY);
            assert!(!wire.lines().next().unwrap().contains(KEY));
            if index == 2 {
                assert!(wire.lines().next().unwrap().contains("&endDate="));
            }
            ""
        }, async |conn| {
            conn.run((PROWLARR.test)(conn)).await.unwrap();
            let fetch = async |index: usize| super::super::fetch(conn, &PROWLARR.widgets[index], &Values::new()).await.map(|body| serde_json::to_value(body).unwrap());
            assert_eq!(fetch(0).await.unwrap()["tiles"], json!([
                {"label":"Blocked","value":"1","glyph":"alert","tone":"warn"},
                {"label":"Queries (7 days)","value":"1,620","glyph":"activity"},
                {"label":"Grabs (7 days)","value":"35","glyph":"download"},
                {"label":"Failed grabs","value":"3","glyph":"alert","tone":"warn"}
            ]));
            assert_eq!(fetch(0).await.unwrap()["tiles"], json!([
                {"label":"Blocked","value":"0","glyph":"alert","tone":"up"},
                {"label":"Queries (7 days)","value":"Unavailable","glyph":"activity"},
                {"label":"Grabs (7 days)","value":"Unavailable","glyph":"download"},
                {"label":"Failed grabs","value":"Unavailable","glyph":"check"}
            ]));
            assert_eq!(fetch(1).await.unwrap()["rows"], json!([
                {"title":"Fixture Index","secondary":"Blocked until 2026-11-22 01:00","value":"100 queries","status":"down"},
                {"title":"Another Index","secondary":"4 grabs · 310 ms","value":"1,500 queries","status":"up"},
                {"title":"Flaky Index","secondary":"1 grab · 5 failed queries","value":"20 queries","status":"warn"}
            ]));
            assert_eq!(fetch(1).await.unwrap()["message"], "Indexer statistics are too large to show.");
            assert_eq!(fetch(1).await.unwrap_err().class.message(), Class::Protocol.message());
            assert_test_fault(conn, &PROWLARR, Class::Auth).await;
            assert_test_fault(conn, &PROWLARR, Class::Protocol).await;
        }).await;
    }
}
