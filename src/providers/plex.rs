use super::*;
use widgets::{BodyItem, StructuredBody};

const TOKEN: &str = "token";
const SECTIONS: &str = "library/sections";
const TOTAL_SIZE: &str = "x-plex-container-total-size";

pub const PLEX: Provider = Provider {
    id: "plex",
    name: "Plex",
    category: "Media",
    icon: "plex",
    description: "Show active streams, library item counts and recently added media.",
    docs: "https://developer.plex.tv/pms/",
    public_internet: false,
    contacts: "Your configured Plex Media Server. Hobnail uses its local server API.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(4096),
            ..Field::new(
                TOKEN,
                "Plex server token",
                Kind::Secret,
                "Use the access token for this Plex Media Server.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "streams",
            name: "Active streams",
            description: "Current playback count and titles.",
            recommended: true,
            fields: &[REFRESH],
            fetch: streams,
            validate: no_widget_options,
        },
        Widget {
            id: "library",
            name: "Library counts",
            description: "Number of libraries and item counts for up to five libraries.",
            recommended: true,
            fields: &[REFRESH],
            fetch: libraries,
            validate: no_widget_options,
        },
        Widget {
            id: "latest",
            name: "Recently added",
            description: "Up to ten recently added items.",
            recommended: false,
            fields: &[REFRESH],
            fetch: recent,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(rename = "MediaContainer")]
    container: T,
}

#[derive(Deserialize)]
struct Sections {
    size: u64,
    #[serde(default, rename = "Directory")]
    libraries: Vec<Library>,
}

#[derive(Deserialize)]
struct Library {
    key: String,
    title: String,
}

#[derive(Deserialize)]
struct Count {
    size: u64,
    #[serde(rename = "totalSize")]
    total_size: Option<u64>,
}

#[derive(Deserialize)]
struct Media {
    size: u64,
    #[serde(default, rename = "Metadata")]
    items: Vec<MediaItem>,
}

#[derive(Deserialize)]
struct MediaItem {
    title: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

async fn get(
    conn: &Conn<'_>,
    path: &str,
    page_size: Option<&str>,
) -> Result<client::Response, Fault> {
    let pagination = page_size.map(|size| {
        [
            ("X-Plex-Container-Start", Input::Text("0")),
            ("X-Plex-Container-Size", Input::Text(size)),
        ]
    });
    conn.request(Step {
        auth: &[Auth::Header("x-plex-token", Input::Credential(TOKEN))],
        query: pagination.as_ref().map_or(&[], |page| page.as_slice()),
        response_headers: &[TOTAL_SIZE],
        accept_json: true,
        ..Step::get(path)
    })
    .await
}

async fn sections(conn: &Conn<'_>) -> Result<Sections, Fault> {
    let result: Envelope<Sections> = parse_json(get(conn, SECTIONS, None).await?)?;
    Ok(result.container)
}

async fn item_count(conn: &Conn<'_>, key: &str) -> Result<Option<u64>, Fault> {
    let key = key.parse::<u64>().map_err(|_| Class::Protocol)?;
    let path = format!("{SECTIONS}/{key}/all");
    let response = get(conn, &path, Some("1")).await?;
    let header_total = response
        .headers
        .get(TOTAL_SIZE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let result: Envelope<Count> = parse_json(response)?;
    let count = result.container;
    Ok(count
        .total_size
        .or(header_total)
        .or((count.size == 0).then_some(0)))
}

async fn media(conn: &Conn<'_>, path: &str, page_size: Option<&str>) -> Result<Media, Fault> {
    let result: Envelope<Media> = parse_json(get(conn, path, page_size).await?)?;
    Ok(result.container)
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { sections(conn).await.map(|_| ()) })
}

fn streams<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(media) = within_response_limit(media(conn, "status/sessions", None).await)? else {
            return Ok(unavailable("Active streams exceed the response limit."));
        };
        let mut rows = vec![("Active streams".into(), media.size.to_string())];
        rows.extend(media.items.into_iter().take(10).map(|item| {
            (
                display_text(conn, item.title),
                display_text(conn, item.kind.unwrap_or_default()),
            )
        }));
        Ok(StructuredBody::StatGrid { rows })
    })
}

fn libraries<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(sections) = within_response_limit(sections(conn).await)? else {
            return Ok(unavailable("The library list exceeds the response limit."));
        };
        let mut rows = vec![("Libraries".into(), sections.size.to_string())];
        for library in sections.libraries.into_iter().take(5) {
            let count = within_response_limit(item_count(conn, &library.key).await)?.flatten();
            rows.push((
                display_text(conn, library.title),
                count.map_or_else(|| "Unavailable".into(), |count| count.to_string()),
            ));
        }
        Ok(StructuredBody::StatGrid { rows })
    })
}

fn recent<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(media) =
            within_response_limit(media(conn, "library/recentlyAdded", Some("10")).await)?
        else {
            return Ok(unavailable(
                "Recently added items exceed the response limit.",
            ));
        };
        if media.items.is_empty() {
            return Ok(StructuredBody::Stat {
                value: "No recently added items".into(),
            });
        }
        Ok(StructuredBody::ItemList {
            items: media
                .items
                .into_iter()
                .take(10)
                .map(|item| BodyItem {
                    icon: None,
                    title: display_text(conn, item.title),
                    secondary: item.kind.map(|kind| display_text(conn, kind)),
                    link: None,
                    time: None,
                })
                .collect(),
        })
    })
}

fn unavailable(message: &str) -> StructuredBody {
    StructuredBody::Unavailable {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn requests_json_and_preserves_other_library_counts_after_an_oversized_page() {
        const PROBE: &str = "GET /prefix/library/sections HTTP/1.1";
        const SECTIONS_BODY: &str = r#"{"MediaContainer":{"size":2,"Directory":[{"key":"2","title":"Movies"},{"key":"3","title":"Series"}]}}"#;
        let oversized = format!(r#"{{"padding":"{}"}}"#, "x".repeat(MAX_RESPONSE));
        exercise(&PLEX, &[(TOKEN, "fake-plex-token")], Values::new(), &[
            Exchange { request: PROBE, status: 200, body: SECTIONS_BODY },
            Exchange { request: "GET /prefix/status/sessions HTTP/1.1", status: 200, body: r#"{"MediaContainer":{"size":1,"Metadata":[{"title":"Example","type":"movie"}]}}"# },
            Exchange { request: PROBE, status: 200, body: SECTIONS_BODY },
            Exchange { request: "GET /prefix/library/sections/2/all?X-Plex-Container-Start=0&X-Plex-Container-Size=1 HTTP/1.1", status: 200, body: &oversized },
            Exchange { request: "GET /prefix/library/sections/3/all?X-Plex-Container-Start=0&X-Plex-Container-Size=1 HTTP/1.1", status: 200, body: r#"{"MediaContainer":{"size":1}}"# },
            Exchange { request: "GET /prefix/library/recentlyAdded?X-Plex-Container-Start=0&X-Plex-Container-Size=10 HTTP/1.1", status: 200, body: r#"{"MediaContainer":{"size":1,"Metadata":[{"title":"New episode","type":"episode"}]}}"# },
            Exchange { request: PROBE, status: 401, body: "" },
            Exchange { request: PROBE, status: 200, body: "{}" },
        ], |index, wire| {
            assert_header(wire, "x-plex-token: fake-plex-token", "fake-plex-token");
            assert!(wire.contains("\r\naccept: application/json\r\n"));
            assert_eq!(wire.lines().filter(|line| line.starts_with("accept:")).count(), 1);
            if index == 4 { "X-Plex-Container-Total-Size: 18\r\n" } else { "" }
        }, async |conn| {
            conn.run(test(conn)).await.unwrap();
            let fetch = async |index: usize| {
                let body = super::super::fetch(conn, &PLEX.widgets[index], &Values::new()).await.unwrap();
                serde_json::to_value(body).unwrap()
            };
            assert_eq!(fetch(0).await["rows"], json!([["Active streams","1"], ["Example","movie"]]));
            assert_eq!(fetch(1).await["rows"], json!([["Libraries","2"], ["Movies","Unavailable"], ["Series","18"]]));
            assert_eq!(fetch(2).await["items"][0]["title"], "New episode");
            assert_test_fault(conn, &PLEX, Class::Auth).await;
            assert_test_fault(conn, &PLEX, Class::Protocol).await;
        }).await;
    }
}
