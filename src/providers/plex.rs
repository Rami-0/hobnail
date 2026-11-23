use super::*;
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

const TOKEN: &str = "token";
const SECTIONS: &str = "library/sections";
const TOTAL_SIZE: &str = "x-plex-container-total-size";

pub const PLEX: Provider = Provider {
    id: "plex",
    name: "Plex",
    category: "Media",
    icon: "plex",
    color: "#e5a00d",
    description: "Show what is playing now, library item counts and recently added media.",
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
                "Use your X-Plex-Token: in Plex Web open any item's ⋯ menu → Get Info → View XML, and copy the X-Plex-Token value from the address.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "streams",
            name: "Now playing",
            description: "Who is watching or listening, on which player, with playback progress.",
            recommended: true,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"rows","rows":[{"title":"The Example Movie","secondary":"alex · Living room TV","value":"1h 2m left","status":"up","progress":44},{"title":"Example Show · S01E03","secondary":"sam · Plex Web","value":"Paused","status":"warn","progress":12}]}"#,
            fields: &[REFRESH],
            fetch: streams,
            validate: no_widget_options,
        },
        Widget {
            id: "library",
            name: "Library counts",
            description: "Item counts for up to six libraries.",
            recommended: false,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Movies","value":"640","glyph":"film"},{"label":"TV Shows","value":"71","glyph":"tv"},{"label":"Music","value":"182","glyph":"music"}]}"#,
            fields: &[REFRESH],
            fetch: libraries,
            validate: no_widget_options,
        },
        Widget {
            id: "latest",
            name: "Recently added",
            description: "Up to ten recently added items.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"The Example Movie","secondary":"Movies","value":"2026"},{"title":"Example Show · S01E04","secondary":"The Sample Episode","value":"2025"},{"title":"Sample Album","secondary":"Example Artist","value":"2024"}]}"#,
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
    #[serde(default, rename = "Directory")]
    libraries: Vec<Library>,
}

#[derive(Deserialize)]
struct Library {
    key: String,
    title: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct Count {
    size: u64,
    #[serde(rename = "totalSize")]
    total_size: Option<u64>,
}

#[derive(Deserialize)]
struct Media {
    #[serde(default, rename = "Metadata")]
    items: Vec<MediaItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaItem {
    title: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    grandparent_title: Option<String>,
    parent_title: Option<String>,
    parent_index: Option<u32>,
    index: Option<u32>,
    year: Option<u32>,
    library_section_title: Option<String>,
    /// Milliseconds.
    duration: Option<u64>,
    view_offset: Option<u64>,
    #[serde(rename = "User")]
    user: Option<Named>,
    #[serde(rename = "Player")]
    player: Option<Player>,
}

#[derive(Deserialize)]
struct Named {
    title: Option<String>,
}

#[derive(Deserialize)]
struct Player {
    title: Option<String>,
    product: Option<String>,
    state: Option<String>,
}

impl MediaItem {
    /// "Show · S01E02" for an episode, "Artist · Track" for a track.
    fn heading(&self, conn: &Conn<'_>) -> String {
        let text = match (self.kind.as_deref(), &self.grandparent_title) {
            (Some("episode"), Some(show)) => match self.parent_index.zip(self.index) {
                Some((season, episode)) => format!("{show} · S{season:02}E{episode:02}"),
                None => format!("{show} · {}", self.title),
            },
            (Some("track"), Some(artist)) => format!("{artist} · {}", self.title),
            _ => self.title.clone(),
        };
        let sources = [Some(self.title.as_str()), self.grandparent_title.as_deref()];
        let sources = sources.into_iter().flatten().collect::<Vec<_>>();
        display_text(conn, conn.output_redactor().transformed(&sources, text))
    }
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
        if media.items.is_empty() {
            return Ok(unavailable("Nothing is playing."));
        }
        let rows = media
            .items
            .into_iter()
            .take(10)
            .map(|item| {
                let player = item.player.as_ref();
                let state = player.and_then(|player| player.state.as_deref());
                let (progress, left) =
                    super::jellyfin::playback(item.view_offset, item.duration, 1000);
                let who = [
                    item.user.as_ref().and_then(|user| user.title.clone()),
                    player.and_then(|player| player.title.clone().or(player.product.clone())),
                ]
                .into_iter()
                .flatten()
                .map(|part| display_text(conn, part))
                .collect::<Vec<_>>();
                BodyRow {
                    title: item.heading(conn),
                    secondary: (!who.is_empty()).then(|| who.join(" · ")),
                    value: match state {
                        Some("paused") => Some("Paused".into()),
                        Some("buffering") => Some("Buffering".into()),
                        _ => left,
                    },
                    status: Some(match state {
                        Some("paused" | "buffering") => Health::Warn,
                        _ => Health::Up,
                    }),
                    progress,
                    bars: Vec::new(),
                }
            })
            .collect();
        Ok(StructuredBody::Rows { rows })
    })
}

fn libraries<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(sections) = within_response_limit(sections(conn).await)? else {
            return Ok(unavailable("The library list exceeds the response limit."));
        };
        if sections.libraries.is_empty() {
            return Ok(unavailable("This server has no libraries."));
        }
        let mut tiles = Vec::new();
        for library in sections.libraries.into_iter().take(6) {
            let count = within_response_limit(item_count(conn, &library.key).await)?.flatten();
            tiles.push(StatTile {
                label: display_text(conn, library.title),
                value: count.map_or_else(|| "Unavailable".into(), super::jellyfin::grouped),
                glyph: match library.kind.as_deref() {
                    Some("movie") => Some(Glyph::Film),
                    Some("show") => Some(Glyph::Tv),
                    Some("artist") => Some(Glyph::Music),
                    Some("photo") => Some(Glyph::Image),
                    _ => None,
                },
                delta: None,
                tone: None,
                series: Vec::new(),
            });
        }
        Ok(StructuredBody::Tiles { tiles })
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
            return Ok(unavailable("No recently added items."));
        }
        Ok(StructuredBody::Rows {
            rows: media
                .items
                .into_iter()
                .take(10)
                .map(|item| BodyRow {
                    title: item.heading(conn),
                    secondary: match item.kind.as_deref() {
                        Some("episode") => Some(item.title.clone()),
                        Some("album") => item.parent_title.clone(),
                        _ => None,
                    }
                    .or_else(|| item.library_section_title.clone())
                    .map(|text| display_text(conn, text)),
                    value: item
                        .year
                        .filter(|&year| year > 0)
                        .map(|year| year.to_string()),
                    ..BodyRow::default()
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
        const SECTIONS_BODY: &str = r#"{"MediaContainer":{"size":2,"Directory":[{"key":"2","title":"Movies","type":"movie"},{"key":"3","title":"Series","type":"show"}]}}"#;
        let oversized = format!(r#"{{"padding":"{}"}}"#, "x".repeat(MAX_RESPONSE));
        exercise(&PLEX, &[(TOKEN, "fake-plex-token")], Values::new(), &[
            Exchange { request: PROBE, status: 200, body: SECTIONS_BODY },
            Exchange { request: "GET /prefix/status/sessions HTTP/1.1", status: 200, body: r#"{"MediaContainer":{"size":2,"Metadata":[{"title":"Example","type":"movie","duration":7200000,"viewOffset":1800000,"User":{"title":"alex"},"Player":{"title":"Living room","product":"Plex for TV","state":"playing"}},{"title":"Pilot","type":"episode","grandparentTitle":"Sample show","parentIndex":1,"index":2,"Player":{"product":"Plex Web","state":"paused"}}]}}"# },
            Exchange { request: "GET /prefix/status/sessions HTTP/1.1", status: 200, body: r#"{"MediaContainer":{"size":0}}"# },
            Exchange { request: "GET /prefix/status/sessions HTTP/1.1", status: 200, body: r#"{"MediaContainer":{"Metadata":"one"}}"# },
            Exchange { request: PROBE, status: 200, body: SECTIONS_BODY },
            Exchange { request: "GET /prefix/library/sections/2/all?X-Plex-Container-Start=0&X-Plex-Container-Size=1 HTTP/1.1", status: 200, body: &oversized },
            Exchange { request: "GET /prefix/library/sections/3/all?X-Plex-Container-Start=0&X-Plex-Container-Size=1 HTTP/1.1", status: 200, body: r#"{"MediaContainer":{"size":1}}"# },
            Exchange { request: "GET /prefix/library/recentlyAdded?X-Plex-Container-Start=0&X-Plex-Container-Size=10 HTTP/1.1", status: 200, body: r#"{"MediaContainer":{"size":2,"Metadata":[{"title":"New episode","type":"episode","grandparentTitle":"Sample show","parentIndex":2,"index":5,"year":2025},{"title":"Sample album","type":"album","parentTitle":"Example artist","librarySectionTitle":"Music"}]}}"# },
            Exchange { request: PROBE, status: 401, body: "" },
            Exchange { request: PROBE, status: 200, body: "{}" },
        ], |index, wire| {
            assert_header(wire, "x-plex-token: fake-plex-token", "fake-plex-token");
            assert!(wire.contains("\r\naccept: application/json\r\n"));
            assert_eq!(wire.lines().filter(|line| line.starts_with("accept:")).count(), 1);
            if index == 6 { "X-Plex-Container-Total-Size: 18\r\n" } else { "" }
        }, async |conn| {
            conn.run(test(conn)).await.unwrap();
            let fetch = async |index: usize| {
                let body = super::super::fetch(conn, &PLEX.widgets[index], &Values::new()).await.unwrap();
                serde_json::to_value(body).unwrap()
            };
            assert_eq!(fetch(0).await["rows"], json!([
                {"title":"Example","secondary":"alex · Living room","value":"1h 30m left","status":"up","progress":25},
                {"title":"Sample show · S01E02","secondary":"Plex Web","value":"Paused","status":"warn"}
            ]));
            assert_eq!(fetch(0).await["message"], "Nothing is playing.");
            let fault = super::super::fetch(conn, &PLEX.widgets[0], &Values::new()).await.unwrap_err();
            assert_eq!(fault.class.message(), Class::Protocol.message());
            assert_eq!(fetch(1).await["tiles"], json!([
                {"label":"Movies","value":"Unavailable","glyph":"film"},
                {"label":"Series","value":"18","glyph":"tv"}
            ]));
            assert_eq!(fetch(2).await["rows"], json!([
                {"title":"Sample show · S02E05","secondary":"New episode","value":"2025"},
                {"title":"Sample album","secondary":"Example artist"}
            ]));
            assert_test_fault(conn, &PLEX, Class::Auth).await;
            assert_test_fault(conn, &PLEX, Class::Protocol).await;
        }).await;
    }
}
