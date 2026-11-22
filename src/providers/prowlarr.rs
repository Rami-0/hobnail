use super::*;
use crate::util::{fmt_utc, unix_now};
use widgets::StructuredBody;

pub const PROWLARR: Provider = Provider {
    id: "prowlarr",
    name: "Prowlarr",
    category: "Media",
    icon: "prowlarr",
    description: "Show temporarily blocked indexers and recent grabs.",
    docs: "https://prowlarr.com/docs/api/",
    public_internet: false,
    contacts: "Your configured Prowlarr service. Public addresses require internet access.",
    fields: arr::FIELDS,
    test,
    widgets: &[Widget {
        id: "indexers",
        name: "Indexer activity",
        description: "Temporarily blocked indexers and grab counts from the last seven days.",
        recommended: true,
        fields: &[REFRESH],
        fetch: indexers,
        validate: no_widget_options,
    }],
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
    number_of_grabs: u32,
    number_of_failed_grabs: u32,
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        arr::get::<Vec<Blocked>>(conn, STATUS, &[])
            .await
            .map(|_| ())
    })
}

fn indexers<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let blocked: Vec<Blocked> = arr::get(conn, STATUS, &[]).await?;
        let now = unix_now();
        let start = fmt_utc(now.saturating_sub(7 * 86_400));
        let end = fmt_utc(now);
        let stats = within_response_limit(arr::get::<Stats>(
            conn,
            "api/v1/indexerstats",
            &[
                ("startDate", Input::Text(&start)),
                ("endDate", Input::Text(&end)),
            ],
        )
        .await)?;
        let mut rows = vec![("Temporarily blocked".into(), blocked.len().to_string())];
        let count = |metric: fn(&Indexer) -> u32| {
            stats.as_ref().map_or_else(
                || "Unavailable".into(),
                |stats| stats.indexers.iter().map(|indexer| u64::from(metric(indexer))).sum::<u64>().to_string(),
            )
        };
        rows.push(("Grabs (7 days)".into(), count(|i| i.number_of_grabs)));
        rows.push(("Failed grabs (7 days)".into(), count(|i| i.number_of_failed_grabs)));
        for status in blocked.into_iter().take(5) {
            let name = stats
                .iter()
                .flat_map(|stats| &stats.indexers)
                .find(|indexer| indexer.indexer_id == status.indexer_id)
                .and_then(|indexer| indexer.indexer_name.as_ref())
                .map_or_else(
                    || format!("Indexer #{}", status.indexer_id),
                    |name| display_text(conn, name.clone()),
                );
            let detail = status.disabled_till.map_or_else(
                || "Blocked".into(),
                |until| format!("Until {}", display_text(conn, until)),
            );
            rows.push((name, detail));
        }
        Ok(StructuredBody::StatGrid { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use serde_json::json;

    #[tokio::test]
    async fn reads_blocked_indexers_and_sets_the_grab_window_with_header_auth() {
        const STATUS: &str = "GET /prefix/api/v1/indexerstatus HTTP/1.1";
        const KEY: &str = "fake-prowlarr-api-key";
        const BLOCKED: &str = r#"[{"id":2,"indexerId":9,"disabledTill":"2026-11-22T01:00:00Z"},{"id":3,"indexerId":11,"disabledTill":"2026-11-22T02:00:00Z"}]"#;
        let oversized = format!(r#"{{"indexers":[],"userAgents":[],"hosts":[]{} }}"#, " ".repeat(128 * 1024));
        exercise(&PROWLARR, &[("api_key", KEY)], Values::new(), &[
            Exchange { request: STATUS, status: 200, body: BLOCKED },
            Exchange { request: STATUS, status: 200, body: BLOCKED },
            Exchange { request: "GET /prefix/api/v1/indexerstats?startDate=*", status: 200, body: r#"{"id":0,"indexers":[{"indexerId":9,"indexerName":"Fixture Index","numberOfGrabs":30,"numberOfFailedGrabs":2},{"indexerId":12,"indexerName":"Another Index","numberOfGrabs":4,"numberOfFailedGrabs":1}],"userAgents":[],"hosts":[]}"# },
            Exchange { request: STATUS, status: 200, body: BLOCKED },
            Exchange { request: "GET /prefix/api/v1/indexerstats?startDate=*", status: 200, body: &oversized },
            Exchange { request: STATUS, status: 401, body: "" },
            Exchange { request: STATUS, status: 200, body: r#"[{"id":2}]"# },
        ], |index, wire| {
            assert_header(wire, &format!("x-api-key: {KEY}"), KEY);
            if index == 2 {
                assert!(wire.lines().next().unwrap().contains("&endDate="));
            }
            ""
        }, async |conn| {
            conn.run((PROWLARR.test)(conn)).await.unwrap();
            let body = super::super::fetch(conn, &PROWLARR.widgets[0], &Values::new()).await.unwrap();
            assert_eq!(serde_json::to_value(body).unwrap()["rows"], json!([
                ["Temporarily blocked", "2"], ["Grabs (7 days)", "34"], ["Failed grabs (7 days)", "3"],
                ["Fixture Index", "Until 2026-11-22T01:00:00Z"], ["Indexer #11", "Until 2026-11-22T02:00:00Z"]
            ]));
            let partial = super::super::fetch(conn, &PROWLARR.widgets[0], &Values::new()).await.unwrap();
            assert_eq!(serde_json::to_value(partial).unwrap()["rows"], json!([
                ["Temporarily blocked", "2"], ["Grabs (7 days)", "Unavailable"], ["Failed grabs (7 days)", "Unavailable"],
                ["Indexer #9", "Until 2026-11-22T01:00:00Z"], ["Indexer #11", "Until 2026-11-22T02:00:00Z"]
            ]));
            assert_test_fault(conn, &PROWLARR, Class::Auth).await;
            assert_test_fault(conn, &PROWLARR, Class::Protocol).await;
        }).await;
    }
}
