use super::*;
use widgets::{BodyItem, StructuredBody};

pub const RADARR: Provider = Provider {
    id: "radarr",
    name: "Radarr",
    category: "Media",
    icon: "radarr",
    description: "Show the download queue, missing movies, health and upcoming releases.",
    docs: "https://radarr.video/docs/api/",
    public_internet: false,
    contacts: "Your configured Radarr service. Public addresses require internet access.",
    fields: arr::FIELDS,
    test: arr::test_v3,
    widgets: &[
        Widget {
            id: "overview",
            name: "Radarr overview",
            description: "Queue size, monitored missing movies and service health.",
            recommended: true,
            fields: &[REFRESH],
            fetch: arr::overview_v3,
            validate: no_widget_options,
        },
        Widget {
            id: "upcoming",
            name: "Upcoming movies",
            description: "Up to ten monitored movies with a release in the next 48 hours.",
            recommended: true,
            fields: &[REFRESH],
            fetch: upcoming,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
struct Movie {
    title: String,
    year: Option<u32>,
}

fn upcoming<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let movies: Vec<Movie> = arr::calendar(conn, &[]).await?;
        Ok(arr::upcoming(movies.into_iter().map(|movie| {
            BodyItem {
                title: display_text(conn, movie.title),
                secondary: movie
                    .year
                    .filter(|&year| year > 0)
                    .map(|year| year.to_string()),
                link: None,
                time: None,
            }
        })))
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
        const KEY: &str = "fake-radarr-api-key";
        exercise(&RADARR, &[("api_key", KEY)], Values::new(), &[
            Exchange { request: HEALTH, status: 200, body: "[]" },
            Exchange { request: "GET /prefix/api/v3/queue/status HTTP/1.1", status: 200, body: r#"{"totalCount":3,"count":2,"unknownCount":1,"errors":false}"# },
            Exchange { request: "GET /prefix/api/v3/wanted/missing?page=1&pageSize=1&monitored=true HTTP/1.1", status: 200, body: r#"{"page":1,"pageSize":1,"totalRecords":14,"records":[{"id":12,"title":"A missing movie"}]}"# },
            Exchange { request: HEALTH, status: 200, body: r#"[{"id":1,"source":"DiskSpaceCheck","type":"error","message":"Disk space is low"}]"# },
            Exchange { request: "GET /prefix/api/v3/calendar?start=*", status: 200, body: r#"[{"id":3,"title":"Sample movie","year":2026,"digitalRelease":"2026-11-22T00:00:00Z"}]"# },
            Exchange { request: HEALTH, status: 403, body: "" },
            Exchange { request: HEALTH, status: 200, body: "{}" },
        ], |index, wire| {
            assert_header(wire, &format!("x-api-key: {KEY}"), KEY);
            if index == 4 {
                let query = wire.lines().next().unwrap();
                assert!(query.contains("&end="));
                assert!(!query.contains("includeSeries"));
            }
            ""
        }, async |conn| {
            conn.run((RADARR.test)(conn)).await.unwrap();
            let overview = super::super::fetch(conn, &RADARR.widgets[0], &Values::new()).await.unwrap();
            assert_eq!(serde_json::to_value(overview).unwrap()["rows"], json!([
                ["Queue", "3"], ["Missing", "14"], ["Health issues", "1"], ["error", "Disk space is low"]
            ]));
            let upcoming = super::super::fetch(conn, &RADARR.widgets[1], &Values::new()).await.unwrap();
            let movie = &serde_json::to_value(upcoming).unwrap()["items"][0];
            assert_eq!(movie["title"], "Sample movie");
            assert_eq!(movie["secondary"], "2026");
            assert_test_fault(conn, &RADARR, Class::Auth).await;
            assert_test_fault(conn, &RADARR, Class::Protocol).await;
        }).await;
    }
}
