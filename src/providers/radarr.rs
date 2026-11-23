use super::*;
use widgets::{AgendaEntry, BodyRow, Health, StructuredBody};

pub const RADARR: Provider = Provider {
    id: "radarr",
    name: "Radarr",
    category: "Media",
    icon: "radarr",
    color: "#e8b322",
    description: "Show upcoming releases, the download queue, missing movies and health.",
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
            recommended: false,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Queue","value":"1","glyph":"download"},{"label":"Missing","value":"6","glyph":"alert"},{"label":"Health","value":"OK","glyph":"check","tone":"up"}]}"#,
            fields: &[REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "upcoming",
            name: "Release calendar",
            description:
                "Monitored movies released in cinemas, digitally or on disc in the next 7 days.",
            recommended: true,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"agenda","entries":[{"time":1795312800,"title":"The Example Movie","secondary":"Digital release · 2026","all_day":true,"status":"up"},{"time":1795399200,"title":"Sample Film","secondary":"In cinemas · 2026","all_day":true},{"time":1795572000,"title":"Another Example","secondary":"Physical release · 2025","all_day":true}]}"#,
            fields: &[REFRESH],
            fetch: upcoming,
            validate: no_widget_options,
        },
        Widget {
            id: "queue",
            name: "Download queue",
            description: "Up to ten queued movies with download progress, time left and problems.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"The Example Movie","secondary":"2026","value":"8m left","status":"up","progress":82},{"title":"Sample Film","secondary":"2025","value":"Paused","status":"up","progress":30}]}"#,
            fields: &[REFRESH],
            fetch: queue,
            validate: no_widget_options,
        },
        Widget {
            id: "missing",
            name: "Missing movies",
            description: "Ten monitored movies without a file, in title order.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"Another Example","secondary":"2024","value":"Released","status":"warn"},{"title":"Sample Film","secondary":"2026","value":"Announced","status":"warn"}]}"#,
            fields: &[REFRESH],
            fetch: missing,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Movie {
    title: Option<String>,
    year: Option<u32>,
    in_cinemas: Option<String>,
    digital_release: Option<String>,
    physical_release: Option<String>,
    #[serde(default)]
    has_file: bool,
    status: Option<String>,
}

#[derive(Deserialize)]
struct QueueMedia {
    movie: Option<Movie>,
}

fn year(movie: &Movie) -> Option<String> {
    movie
        .year
        .filter(|&year| year > 0)
        .map(|year| year.to_string())
}

fn title(conn: &Conn<'_>, movie: &Movie) -> Option<String> {
    movie.title.clone().map(|title| display_text(conn, title))
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(arr::overview(conn, "Missing"))
}

fn upcoming<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let movies: Option<Vec<Movie>> = arr::calendar(conn, &[]).await?;
        let (now, end) = {
            let now = crate::util::unix_now();
            (now - now % 86_400, now + arr::AGENDA_DAYS * 86_400)
        };
        Ok(arr::agenda(movies, |movie| {
            // The calendar returns a movie when any of its releases falls in
            // the window; show the first one that does.
            let (time, kind) = [
                (&movie.in_cinemas, "In cinemas"),
                (&movie.digital_release, "Digital release"),
                (&movie.physical_release, "Physical release"),
            ]
            .into_iter()
            .filter_map(|(date, kind)| Some((arr::unix(date.as_deref())?, kind)))
            .filter(|&(time, _)| (now..=end).contains(&time))
            .min_by_key(|&(time, _)| time)?;
            Some(AgendaEntry {
                time,
                title: title(conn, &movie).unwrap_or_else(|| "Untitled movie".into()),
                secondary: Some(match year(&movie) {
                    Some(year) => format!("{kind} · {year}"),
                    None => kind.into(),
                }),
                all_day: true,
                status: movie.has_file.then_some(Health::Up),
            })
        }))
    })
}

fn queue<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        arr::queue::<QueueMedia>(conn, &[("includeMovie", "true")], |record| {
            let movie = record.media.movie.as_ref();
            (
                movie.and_then(|movie| title(conn, movie)),
                movie.and_then(year),
            )
        })
        .await
    })
}

fn missing<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        arr::missing(
            conn,
            ("movieMetadata.sortTitle", "ascending"),
            &[],
            |movie: Movie| BodyRow {
                title: title(conn, &movie).unwrap_or_else(|| "Untitled movie".into()),
                secondary: year(&movie),
                value: movie.status.as_deref().map(|status| {
                    match status {
                        "tba" => "To be announced",
                        "announced" => "Announced",
                        "inCinemas" => "In cinemas",
                        "released" => "Released",
                        _ => "Unknown",
                    }
                    .into()
                }),
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
    use crate::util::{fmt_utc, unix_now};
    use serde_json::json;

    fn widget(id: &str) -> &'static Widget {
        RADARR.widgets.iter().find(|w| w.id == id).unwrap()
    }

    #[tokio::test]
    async fn reads_overview_calendar_queue_and_missing_with_header_key() {
        const HEALTH: &str = "GET /prefix/api/v3/health HTTP/1.1";
        const KEY: &str = "fake-radarr-api-key";
        // Calendar dates relative to now: one release in the window, one long past.
        let today = unix_now() - unix_now() % 86_400;
        let soon = today + 2 * 86_400;
        let calendar = format!(
            r#"[{{"id":3,"title":"Sample movie","year":2026,"inCinemas":"2020-01-01T00:00:00Z","digitalRelease":"{}","hasFile":true}},{{"id":4,"title":"Old","physicalRelease":"2020-01-01T00:00:00Z"}}]"#,
            fmt_utc(soon)
        );
        exercise(&RADARR, &[("api_key", KEY)], Values::new(), &[
            Exchange { request: HEALTH, status: 200, body: "[]" },
            Exchange { request: "GET /prefix/api/v3/queue/status HTTP/1.1", status: 200, body: r#"{"totalCount":3,"count":2,"unknownCount":1,"errors":false}"# },
            Exchange { request: "GET /prefix/api/v3/wanted/missing?page=1&pageSize=1&monitored=true HTTP/1.1", status: 200, body: r#"{"page":1,"pageSize":1,"totalRecords":14,"records":[{"id":12,"title":"A missing movie"}]}"# },
            Exchange { request: HEALTH, status: 200, body: r#"[{"id":1,"source":"DiskSpaceCheck","type":"error","message":"Disk space is low"}]"# },
            Exchange { request: "GET /prefix/api/v3/calendar?start=*", status: 200, body: &calendar },
            Exchange { request: "GET /prefix/api/v3/queue?page=1&pageSize=10&includeMovie=true HTTP/1.1", status: 200, body: r#"{"totalRecords":1,"records":[{"title":"Sample.Movie.2026","size":400,"sizeleft":100,"timeleft":"1.02:00:00","status":"downloading","trackedDownloadStatus":"error","movie":{"title":"Sample movie","year":2026}}]}"# },
            Exchange { request: "GET /prefix/api/v3/wanted/missing?page=1&pageSize=10&monitored=true&sortKey=movieMetadata.sortTitle&sortDirection=ascending HTTP/1.1", status: 200, body: r#"{"totalRecords":1,"records":[{"title":"A missing movie","year":2024,"status":"released"}]}"# },
            Exchange { request: "GET /prefix/api/v3/wanted/missing?page=1*", status: 200, body: r#"{"totalRecords":0,"records":[]}"# },
            Exchange { request: "GET /prefix/api/v3/calendar?start=*", status: 200, body: "{}" },
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
            let fetch = async |id| serde_json::to_value(super::super::fetch(conn, widget(id), &Values::new()).await.unwrap()).unwrap();
            assert_eq!(fetch("overview").await["tiles"][2], json!({"label":"Health","value":"1 issue","glyph":"alert","tone":"down"}));
            assert_eq!(fetch("upcoming").await["entries"], json!([
                {"time":soon,"title":"Sample movie","secondary":"Digital release · 2026","all_day":true,"status":"up"}
            ]));
            assert_eq!(fetch("queue").await["rows"], json!([
                {"title":"Sample movie","secondary":"2026","value":"1d 2h left","status":"down","progress":75}
            ]));
            assert_eq!(fetch("missing").await["rows"], json!([
                {"title":"A missing movie","secondary":"2024","value":"Released","status":"warn"}
            ]));
            assert_eq!(fetch("missing").await["message"], "Nothing monitored is missing.");
            let fault = super::super::fetch(conn, widget("upcoming"), &Values::new()).await.unwrap_err();
            assert_eq!(fault.class.message(), Class::Protocol.message());
            assert_test_fault(conn, &RADARR, Class::Auth).await;
            assert_test_fault(conn, &RADARR, Class::Protocol).await;
        }).await;
    }
}
