use super::*;
use widgets::{AgendaEntry, BodyRow, Health, StructuredBody};

pub const SONARR: Provider = Provider {
    id: "sonarr",
    name: "Sonarr",
    category: "Media",
    icon: "sonarr",
    color: "#2fb6e6",
    detect: super::detect(&["sonarr"], 8989),
    description: "Show upcoming episodes, the download queue, missing episodes and health.",
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
            recommended: false,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Queue","value":"3","glyph":"download"},{"label":"Missing","value":"14","glyph":"alert"},{"label":"Health","value":"OK","glyph":"check","tone":"up"}]}"#,
            fields: &[REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "upcoming",
            name: "Episode calendar",
            description:
                "Monitored episodes airing in the next 7 days, by day, marked when downloaded.",
            recommended: true,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"agenda","entries":[{"time":1795312800,"title":"Example Show","secondary":"S02E05 · The Sample Episode","status":"up"},{"time":1795399200,"title":"Sample Series","secondary":"S01E03 · Another Example"},{"time":1795485600,"title":"Example Show","secondary":"S02E06 · Next Time"}]}"#,
            fields: &[REFRESH],
            fetch: upcoming,
            validate: no_widget_options,
        },
        Widget {
            id: "queue",
            name: "Download queue",
            description:
                "Up to ten queued episodes with download progress, time left and problems.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"Example Show","secondary":"S02E05 · The Sample Episode","value":"12m left","status":"up","progress":64},{"title":"Sample Series","secondary":"S01E03 · Another Example","value":"Queued","status":"up","progress":0},{"title":"Example Show","secondary":"S02E04 · Import blocked","value":"Completed","status":"warn","progress":100}]}"#,
            fields: &[REFRESH],
            fetch: queue,
            validate: no_widget_options,
        },
        Widget {
            id: "missing",
            name: "Missing episodes",
            description: "The ten most recently aired monitored episodes without a file.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"Example Show","secondary":"S02E03 · The Missing Piece","value":"2026-11-18","status":"warn"},{"title":"Sample Series","secondary":"S01E01 · Pilot","value":"2026-11-02","status":"warn"}]}"#,
            fields: &[REFRESH],
            fetch: missing,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Episode {
    title: Option<String>,
    season_number: Option<u32>,
    episode_number: Option<u32>,
    air_date_utc: Option<String>,
    #[serde(default)]
    has_file: bool,
    series: Option<Series>,
}

#[derive(Deserialize)]
struct Series {
    title: Option<String>,
}

#[derive(Deserialize)]
struct QueueMedia {
    series: Option<Series>,
    episode: Option<Episode>,
}

/// "S02E05 · Title", or whichever part is known.
fn episode_label(conn: &Conn<'_>, episode: &Episode) -> Option<String> {
    let number = episode
        .season_number
        .zip(episode.episode_number)
        .map(|(season, number)| format!("S{season:02}E{number:02}"));
    let title = episode.title.clone().map(|title| display_text(conn, title));
    match (number, title) {
        (Some(number), Some(title)) => Some(format!("{number} · {title}")),
        (number, title) => number.or(title),
    }
}

fn series_title(conn: &Conn<'_>, series: &Option<Series>) -> Option<String> {
    series
        .as_ref()
        .and_then(|series| series.title.clone())
        .map(|title| display_text(conn, title))
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(arr::overview(conn, "Missing"))
}

fn upcoming<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let episodes: Option<Vec<Episode>> =
            arr::calendar(conn, &[("includeSeries", "true")]).await?;
        Ok(arr::agenda(episodes, |episode| {
            let time = arr::unix(episode.air_date_utc.as_deref())?;
            let secondary = episode_label(conn, &episode);
            Some(AgendaEntry {
                time,
                title: series_title(conn, &episode.series)
                    .or_else(|| secondary.clone())
                    .unwrap_or_else(|| "Untitled episode".into()),
                secondary,
                all_day: false,
                status: episode.has_file.then_some(Health::Up),
            })
        }))
    })
}

fn queue<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        arr::queue::<QueueMedia>(
            conn,
            &[("includeSeries", "true"), ("includeEpisode", "true")],
            |record| {
                (
                    series_title(conn, &record.media.series),
                    record
                        .media
                        .episode
                        .as_ref()
                        .and_then(|episode| episode_label(conn, episode)),
                )
            },
        )
        .await
    })
}

fn missing<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        arr::missing(
            conn,
            ("episodes.airDateUtc", "descending"),
            &[("includeSeries", "true")],
            |episode: Episode| BodyRow {
                title: series_title(conn, &episode.series)
                    .unwrap_or_else(|| "Unknown series".into()),
                secondary: episode_label(conn, &episode),
                value: episode
                    .air_date_utc
                    .as_deref()
                    .and_then(|date| date.get(..10))
                    .map(|date| display_text(conn, date.into())),
                status: Some(Health::Warn),
                progress: None,
                bars: Vec::new(),
            },
        )
        .await
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use serde_json::json;

    const KEY: &str = "fake-sonarr-api-key";
    const HEALTH: &str = "GET /prefix/api/v3/health HTTP/1.1";

    fn widget(id: &str) -> &'static Widget {
        SONARR.widgets.iter().find(|w| w.id == id).unwrap()
    }

    #[tokio::test]
    async fn reads_overview_calendar_queue_and_missing_with_header_key() {
        let oversized = format!("[{}]", " ".repeat(128 * 1024));
        exercise(&SONARR, &[("api_key", KEY)], Values::new(), &[
            Exchange { request: HEALTH, status: 200, body: "[]" },
            Exchange { request: "GET /prefix/api/v3/queue/status HTTP/1.1", status: 200, body: r#"{"totalCount":5,"count":4,"unknownCount":1,"errors":false}"# },
            Exchange { request: "GET /prefix/api/v3/wanted/missing?page=1&pageSize=1&monitored=true HTTP/1.1", status: 200, body: r#"{"page":1,"pageSize":1,"totalRecords":42,"records":[{"id":12,"title":"Pilot"}]}"# },
            Exchange { request: HEALTH, status: 200, body: r#"[{"id":1,"source":"IndexerStatusCheck","type":"warning","message":"Indexer unavailable"}]"# },
            Exchange { request: "GET /prefix/api/v3/queue/status HTTP/1.1", status: 200, body: r#"{"totalCount":0}"# },
            Exchange { request: "GET /prefix/api/v3/wanted/missing?page=1&pageSize=1&monitored=true HTTP/1.1", status: 200, body: r#"{"totalRecords":0,"records":[]}"# },
            Exchange { request: HEALTH, status: 200, body: &oversized },
            Exchange { request: "GET /prefix/api/v3/calendar?start=*", status: 200, body: r#"[
                {"id":2,"title":"Later","seasonNumber":1,"episodeNumber":2,"airDateUtc":"2026-11-23T02:00:00Z","hasFile":false,"series":{"title":"Sample series"}},
                {"id":1,"title":"The first episode","seasonNumber":1,"episodeNumber":1,"airDateUtc":"2026-11-22T02:00:00Z","hasFile":true,"series":{"title":"Sample series"}},
                {"id":3,"title":"No date","seasonNumber":1,"episodeNumber":3}
            ]"# },
            Exchange { request: "GET /prefix/api/v3/calendar?start=*", status: 200, body: &oversized },
            Exchange { request: "GET /prefix/api/v3/calendar?start=*", status: 200, body: "[]" },
            Exchange { request: "GET /prefix/api/v3/queue?page=1&pageSize=10&includeSeries=true&includeEpisode=true HTTP/1.1", status: 200, body: r#"{"page":1,"pageSize":10,"totalRecords":2,"records":[
                {"id":1,"title":"Sample.Series.S01E01.1080p","size":1000,"sizeleft":250,"timeleft":"00:12:30","status":"downloading","trackedDownloadStatus":"ok","series":{"title":"Sample series"},"episode":{"title":"Pilot","seasonNumber":1,"episodeNumber":1}},
                {"id":2,"title":"Other.Release","size":0,"sizeleft":0,"status":"completed","trackedDownloadStatus":"warning"}
            ]}"# },
            Exchange { request: "GET /prefix/api/v3/queue?page=1&pageSize=10&includeSeries=true&includeEpisode=true HTTP/1.1", status: 200, body: r#"{"totalRecords":0,"records":[]}"# },
            Exchange { request: "GET /prefix/api/v3/wanted/missing?page=1&pageSize=10&monitored=true&sortKey=episodes.airDateUtc&sortDirection=descending&includeSeries=true HTTP/1.1", status: 200, body: r#"{"totalRecords":1,"records":[{"title":"Gone","seasonNumber":2,"episodeNumber":3,"airDateUtc":"2026-11-18T01:00:00Z","series":{"title":"Sample series"}}]}"# },
            Exchange { request: "GET /prefix/api/v3/queue?page=1*", status: 200, body: r#"{"records":"not a list"}"# },
            Exchange { request: HEALTH, status: 401, body: "" },
            Exchange { request: HEALTH, status: 200, body: r#"{"not":"a list"}"# },
        ], |index, wire| {
            assert_header(wire, &format!("x-api-key: {KEY}"), KEY);
            assert!(!wire.lines().next().unwrap().contains(KEY), "the key stays out of the URL");
            if index == 7 {
                assert!(wire.lines().next().unwrap().contains("&end="));
                assert!(wire.lines().next().unwrap().ends_with("&includeSeries=true HTTP/1.1"));
            }
            ""
        }, async |conn| {
            conn.run((SONARR.test)(conn)).await.unwrap();
            let fetch = async |id| serde_json::to_value(super::super::fetch(conn, widget(id), &Values::new()).await.unwrap()).unwrap();
            assert_eq!(fetch("overview").await["tiles"], json!([
                {"label":"Queue","value":"5","glyph":"download"},
                {"label":"Missing","value":"42","glyph":"alert"},
                {"label":"Health","value":"1 issue","glyph":"alert","tone":"warn"}
            ]));
            assert_eq!(fetch("overview").await["tiles"][2]["value"], "Unavailable");
            assert_eq!(fetch("upcoming").await["entries"], json!([
                {"time":1_795_312_800u64,"title":"Sample series","secondary":"S01E01 · The first episode","status":"up"},
                {"time":1_795_399_200u64,"title":"Sample series","secondary":"S01E02 · Later"}
            ]));
            for message in ["Calendar response too large for the next 7 days.", "Nothing upcoming in the next 7 days."] {
                assert_eq!(fetch("upcoming").await["message"], message);
            }
            assert_eq!(fetch("queue").await["rows"], json!([
                {"title":"Sample series","secondary":"S01E01 · Pilot","value":"12m left","status":"up","progress":75},
                {"title":"Other.Release","value":"Completed","status":"warn"}
            ]));
            assert_eq!(fetch("queue").await["message"], "The download queue is empty.");
            assert_eq!(fetch("missing").await["rows"], json!([
                {"title":"Sample series","secondary":"S02E03 · Gone","value":"2026-11-18","status":"warn"}
            ]));
            let fault = super::super::fetch(conn, widget("queue"), &Values::new()).await.unwrap_err();
            assert_eq!(fault.class.message(), Class::Protocol.message());
            assert_test_fault(conn, &SONARR, Class::Auth).await;
            assert_test_fault(conn, &SONARR, Class::Protocol).await;
        }).await;
    }
}
