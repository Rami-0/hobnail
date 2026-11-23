use super::*;
use widgets::{BodyItem, StructuredBody};

pub const SONARR: Provider = Provider {
    id: "sonarr",
    name: "Sonarr",
    category: "Media",
    icon: "sonarr",
    color: "#2fb6e6",
    description: "Show the download queue, missing episodes, health and upcoming airings.",
    docs: "https://github.com/Sonarr/Sonarr/blob/v4.0.17.2952/src/Sonarr.Api.V3/openapi.json",
    public_internet: false,
    contacts: "Your configured Sonarr service. Public addresses require internet access.",
    fields: arr::FIELDS,
    test: arr::test_v3,
    widgets: &[
        Widget {
            id: "overview",
            name: "Sonarr overview",
            description: "Queue size, monitored missing episodes and service health.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"stat_grid","rows":[["Queue","3"],["Missing","14"],["Health issues","0"]]}"#,
            fields: &[REFRESH],
            fetch: arr::overview_v3,
            validate: no_widget_options,
        },
        Widget {
            id: "upcoming",
            name: "Upcoming episodes",
            description: "Up to ten monitored episodes airing in the next 48 hours.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"item_list","items":[{"title":"The First Episode","secondary":"Example Show · tomorrow 21:00","link":null,"time":null},{"title":"Second Episode","secondary":"Sample Series · Fri 20:00","link":null,"time":null}]}"#,
            fields: &[REFRESH],
            fetch: upcoming,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Episode {
    title: Option<String>,
    air_date_utc: String,
    series: Option<Series>,
}

#[derive(Deserialize)]
struct Series {
    title: Option<String>,
}

fn upcoming<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let episodes: Option<Vec<Episode>> =
            arr::calendar(conn, &[("includeSeries", "true")]).await?;
        Ok(arr::upcoming(episodes, |episode| {
            let title = display_text(
                conn,
                episode.title.unwrap_or_else(|| "Untitled episode".into()),
            );
            let series = episode.series.and_then(|series| series.title);
            let airing = display_text(conn, episode.air_date_utc);
            let secondary = series.map_or_else(
                || airing.clone(),
                |series| format!("{} · {airing}", display_text(conn, series)),
            );
            BodyItem {
                icon: None,
                title,
                secondary: Some(secondary),
                link: None,
                time: None,
            }
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use serde_json::json;

    #[tokio::test]
    async fn reads_health_queue_missing_and_calendar_with_header_key() {
        const HEALTH: &str = "GET /prefix/api/v3/health HTTP/1.1";
        const ISSUE: &str = r#"[{"id":1,"source":"IndexerStatusCheck","type":"warning","message":"Indexer unavailable"}]"#;
        const KEY: &str = "fake-sonarr-api-key";
        let oversized = format!("[{}]", " ".repeat(128 * 1024));
        exercise(&SONARR, &[("api_key", KEY)], Values::new(), &[
            Exchange { request: HEALTH, status: 200, body: "[]" },
            Exchange { request: "GET /prefix/api/v3/queue/status HTTP/1.1", status: 200, body: r#"{"totalCount":5,"count":4,"unknownCount":1,"errors":false}"# },
            Exchange { request: "GET /prefix/api/v3/wanted/missing?page=1&pageSize=1&monitored=true HTTP/1.1", status: 200, body: r#"{"page":1,"pageSize":1,"totalRecords":42,"records":[{"id":12,"title":"Pilot"}]}"# },
            Exchange { request: HEALTH, status: 200, body: ISSUE },
            Exchange { request: "GET /prefix/api/v3/calendar?start=*", status: 200, body: r#"[{"id":1,"title":"The first episode","airDateUtc":"2026-11-22T02:00:00Z","series":{"title":"Sample series"}}]"# },
            Exchange { request: "GET /prefix/api/v3/queue/status HTTP/1.1", status: 200, body: r#"{"totalCount":5}"# },
            Exchange { request: "GET /prefix/api/v3/wanted/missing?page=1&pageSize=1&monitored=true HTTP/1.1", status: 200, body: r#"{"totalRecords":42,"records":[]}"# },
            Exchange { request: HEALTH, status: 200, body: &oversized },
            Exchange { request: "GET /prefix/api/v3/calendar?start=*", status: 200, body: &oversized },
            Exchange { request: "GET /prefix/api/v3/calendar?start=*", status: 200, body: "[]" },
            Exchange { request: HEALTH, status: 401, body: "" },
            Exchange { request: HEALTH, status: 200, body: r#"[{"id":7}]"# },
        ], |index, wire| {
            assert_header(wire, &format!("x-api-key: {KEY}"), KEY);
            if index == 4 {
                assert!(wire.lines().next().unwrap().contains("&end="));
                assert!(wire.lines().next().unwrap().ends_with("&includeSeries=true HTTP/1.1"));
            }
            ""
        }, async |conn| {
            conn.run((SONARR.test)(conn)).await.unwrap();
            let overview = super::super::fetch(conn, &SONARR.widgets[0], &Values::new()).await.unwrap();
            assert_eq!(serde_json::to_value(overview).unwrap()["rows"], json!([
                ["Queue", "5"], ["Missing", "42"], ["Health issues", "1"], ["warning", "Indexer unavailable"]
            ]));
            let upcoming = super::super::fetch(conn, &SONARR.widgets[1], &Values::new()).await.unwrap();
            assert_eq!(serde_json::to_value(upcoming).unwrap()["items"][0]["secondary"], "Sample series · 2026-11-22T02:00:00Z");
            let overview = super::super::fetch(conn, &SONARR.widgets[0], &Values::new()).await.unwrap();
            assert_eq!(serde_json::to_value(overview).unwrap()["rows"], json!([
                ["Queue", "5"], ["Missing", "42"], ["Health issues", "Unavailable"]
            ]));
            for message in ["Calendar response too large for the next 48 hours.", "Nothing upcoming in the next 48 hours."] {
                let upcoming = super::super::fetch(conn, &SONARR.widgets[1], &Values::new()).await.unwrap();
                assert_eq!(serde_json::to_value(upcoming).unwrap()["message"], message);
            }
            assert_test_fault(conn, &SONARR, Class::Auth).await;
            assert_test_fault(conn, &SONARR, Class::Protocol).await;
        }).await;
    }
}
