use super::*;
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

pub const JELLYFIN: Provider = Provider {
    id: "jellyfin",
    name: "Jellyfin",
    category: "Media",
    icon: "jellyfin",
    color: "#9b5bd0",
    description: "Show what is playing now, library counts and recently added media.",
    docs: "https://api.jellyfin.org/",
    public_internet: false,
    contacts: "Your configured Jellyfin server. Public addresses require internet access.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(128),
            ..Field::new(
                "api_key",
                "API key",
                Kind::Secret,
                "In Jellyfin, open Dashboard → API Keys, select + and name the key Hobnail.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "streams",
            name: "Now playing",
            description: "Who is watching or listening, on which device, with playback progress.",
            recommended: true,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"rows","rows":[{"title":"The Example Movie","secondary":"alex · Living room TV","value":"42m left","status":"up","progress":38},{"title":"Example Show · S02E05","secondary":"sam · Jellyfin Web","value":"Paused","status":"warn","progress":71}]}"#,
            fields: &[REFRESH],
            fetch: streams,
            validate: no_widget_options,
        },
        Widget {
            id: "library",
            name: "Library counts",
            description: "Movies, series, episodes, albums, songs and books.",
            recommended: false,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Movies","value":"812","glyph":"film"},{"label":"Series","value":"96","glyph":"tv"},{"label":"Episodes","value":"4,120","glyph":"tv"},{"label":"Songs","value":"3,318","glyph":"music"}]}"#,
            fields: &[REFRESH],
            fetch: library,
            validate: no_widget_options,
        },
        Widget {
            id: "latest",
            name: "Latest added",
            description: "Ten recent movies, episodes, albums, songs or books.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"The Example Movie","secondary":"Movie","value":"2026"},{"title":"Example Show · S02E05","secondary":"Episode · The Sample Episode"},{"title":"Sample Album","secondary":"Album · Example Artist","value":"2025"}]}"#,
            fields: &[REFRESH],
            fetch: latest,
            validate: no_widget_options,
        },
    ],
};

/// A .NET tick is 100 ns.
const TICKS_PER_SECOND: u64 = 10_000_000;

async fn get<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    path: &str,
    query: &[(&str, Input<'_>)],
) -> Result<T, Fault> {
    parse_json(
        conn.request(Step {
            auth: &[Auth::Authorization(
                "MediaBrowser Token=",
                Input::Credential("api_key"),
            )],
            query,
            ..Step::get(path)
        })
        .await?,
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Counts {
    movie_count: u64,
    series_count: u64,
    episode_count: u64,
    album_count: u64,
    song_count: u64,
    book_count: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Playback {
    #[serde(rename = "Id")]
    _id: String,
    now_playing_item: Option<Media>,
    play_state: Option<PlayState>,
    client: Option<String>,
    device_name: Option<String>,
    user_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PlayState {
    position_ticks: Option<u64>,
    #[serde(default)]
    is_paused: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Media {
    name: Option<String>,
    #[serde(rename = "Type")]
    kind: Option<String>,
    series_name: Option<String>,
    album_artist: Option<String>,
    index_number: Option<u32>,
    parent_index_number: Option<u32>,
    run_time_ticks: Option<u64>,
    production_year: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Latest {
    items: Vec<Media>,
}

impl Media {
    /// "Series · S01E02" for an episode, otherwise the item's name.
    fn heading(&self, conn: &Conn<'_>) -> String {
        let name = || self.name.clone().unwrap_or_else(|| "Untitled media".into());
        let text = match (self.kind.as_deref(), &self.series_name) {
            (Some("Episode"), Some(series)) => {
                match self.parent_index_number.zip(self.index_number) {
                    Some((season, episode)) => format!("{series} · S{season:02}E{episode:02}"),
                    None => format!("{series} · {}", name()),
                }
            }
            _ => name(),
        };
        let sources = [self.name.as_deref(), self.series_name.as_deref()];
        let sources = sources.into_iter().flatten().collect::<Vec<_>>();
        display_text(conn, conn.output_redactor().transformed(&sources, text))
    }

    /// The item type in words, with the episode title or album artist.
    fn detail(&self, conn: &Conn<'_>) -> Option<String> {
        let kind = match self.kind.as_deref()? {
            "Movie" => "Movie",
            "Episode" => "Episode",
            "MusicAlbum" => "Album",
            "Audio" => "Song",
            "Book" => "Book",
            "Series" => "Series",
            other => return Some(display_text(conn, other.into())),
        };
        let extra = match kind {
            "Episode" if self.series_name.is_some() => self.name.clone(),
            "Album" | "Song" => self.album_artist.clone(),
            _ => None,
        };
        Some(match extra {
            Some(extra) => format!("{kind} · {}", display_text(conn, extra)),
            None => kind.into(),
        })
    }
}

/// A count with thousands separators: 4120 → "4,120".
pub(super) fn grouped(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A count tile; None when there is nothing of that kind.
pub(super) fn count_tile(label: &str, count: u64, glyph: Option<Glyph>) -> Option<StatTile> {
    (count > 0).then(|| StatTile {
        label: label.into(),
        value: grouped(count),
        glyph,
        delta: None,
        tone: None,
        series: Vec::new(),
    })
}

/// Playback progress and the time left, from positions in any unit.
pub(super) fn playback(
    position: Option<u64>,
    runtime: Option<u64>,
    per_second: u64,
) -> (Option<u8>, Option<String>) {
    let Some(runtime) = runtime.filter(|&runtime| runtime > 0) else {
        return (None, None);
    };
    let position = position.unwrap_or(0).min(runtime);
    let percent = (position as f64 / runtime as f64 * 100.0).round() as u8;
    let left = (runtime - position) / per_second;
    (
        Some(percent),
        Some(format!("{} left", crate::util::human_duration(left))),
    )
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        get::<Vec<Playback>>(
            conn,
            "Sessions",
            &[("activeWithinSeconds", Input::Text("1"))],
        )
        .await
        .map(|_| ())
    })
}

fn streams<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(sessions) =
            within_response_limit(get::<Vec<Playback>>(conn, "Sessions", &[]).await)?
        else {
            return Ok(StructuredBody::Unavailable {
                message: "Session list too large to show active streams.".into(),
            });
        };
        let rows: Vec<_> = sessions
            .into_iter()
            .filter_map(|session| {
                let item = session.now_playing_item?;
                let state = session.play_state;
                let paused = state.as_ref().is_some_and(|state| state.is_paused);
                let (progress, left) = playback(
                    state.and_then(|state| state.position_ticks),
                    item.run_time_ticks,
                    TICKS_PER_SECOND,
                );
                let who = [session.user_name, session.device_name.or(session.client)]
                    .into_iter()
                    .flatten()
                    .map(|part| display_text(conn, part))
                    .collect::<Vec<_>>();
                Some(BodyRow {
                    title: item.heading(conn),
                    secondary: (!who.is_empty()).then(|| who.join(" · ")),
                    value: if paused { Some("Paused".into()) } else { left },
                    status: Some(if paused { Health::Warn } else { Health::Up }),
                    progress,
                    bars: Vec::new(),
                })
            })
            .take(10)
            .collect();
        if rows.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "Nothing is playing.".into(),
            });
        }
        Ok(StructuredBody::Rows { rows })
    })
}

fn library<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let counts: Counts = get(conn, "Items/Counts", &[]).await?;
        let tiles: Vec<_> = [
            ("Movies", counts.movie_count, Some(Glyph::Film)),
            ("Series", counts.series_count, Some(Glyph::Tv)),
            ("Episodes", counts.episode_count, Some(Glyph::Tv)),
            ("Albums", counts.album_count, Some(Glyph::Music)),
            ("Songs", counts.song_count, Some(Glyph::Music)),
            ("Books", counts.book_count, None),
        ]
        .into_iter()
        .filter_map(|(label, count, glyph)| count_tile(label, count, glyph))
        .collect();
        if tiles.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "The libraries are empty.".into(),
            });
        }
        Ok(StructuredBody::Tiles { tiles })
    })
}

fn latest<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let result: Latest = get(
            conn,
            "Items",
            &[
                ("recursive", Input::Text("true")),
                (
                    "includeItemTypes",
                    Input::Text("Movie,Episode,MusicAlbum,Audio,Book"),
                ),
                ("sortBy", Input::Text("DateCreated")),
                ("sortOrder", Input::Text("Descending")),
                ("limit", Input::Text("10")),
                ("enableImages", Input::Text("false")),
                ("enableUserData", Input::Text("false")),
                ("enableTotalRecordCount", Input::Text("false")),
            ],
        )
        .await?;
        if result.items.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No recently added media.".into(),
            });
        }
        Ok(StructuredBody::Rows {
            rows: result
                .items
                .into_iter()
                .take(10)
                .map(|item| BodyRow {
                    title: item.heading(conn),
                    secondary: item.detail(conn),
                    value: item.production_year.map(|year| year.to_string()),
                    ..BodyRow::default()
                })
                .collect(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use serde_json::json;

    #[test]
    fn groups_counts_and_reads_playback() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(4120), "4,120");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(
            playback(Some(30), Some(120), 1),
            (Some(25), Some("1m left".into()))
        );
        assert_eq!(
            playback(Some(500), Some(120), 1),
            (Some(100), Some("0s left".into()))
        );
        assert_eq!(playback(None, None, 1), (None, None));
    }

    #[tokio::test]
    async fn reads_playback_counts_and_recent_items_with_api_key() {
        const KEY: &str = "abcdef0123456789abcdef0123456789";
        const PROBE: &str = "GET /prefix/Sessions?activeWithinSeconds=1 HTTP/1.1";
        const SESSIONS: &str = "GET /prefix/Sessions HTTP/1.1";
        const COUNTS: &str = "GET /prefix/Items/Counts HTTP/1.1";
        const COUNTS_BODY: &str = r#"{"MovieCount":42,"SeriesCount":3,"EpisodeCount":105,"AlbumCount":6,"SongCount":0,"MusicVideoCount":0,"BoxSetCount":0,"BookCount":4}"#;
        let oversized = format!("[{}]", " ".repeat(128 * 1024));
        exercise(&JELLYFIN, &[("api_key", KEY)], Values::new(), &[
            Exchange { request: PROBE, status: 200, body: "[]" },
            Exchange { request: SESSIONS, status: 200, body: r#"[
                {"Id":"one","UserName":"alex","Client":"Jellyfin Web","DeviceName":"Living room","NowPlayingItem":{"Name":"Dune","Type":"Movie","RunTimeTicks":36000000000},"PlayState":{"PositionTicks":9000000000,"IsPaused":false}},
                {"Id":"two","NowPlayingItem":null},
                {"Id":"three","Client":"Jellyfin Android","NowPlayingItem":{"Name":"Pilot","Type":"Episode","SeriesName":"Sample series","ParentIndexNumber":1,"IndexNumber":2},"PlayState":{"IsPaused":true}}
            ]"# },
            Exchange { request: SESSIONS, status: 200, body: r#"[{"Id":"idle"}]"# },
            Exchange { request: SESSIONS, status: 200, body: &oversized },
            Exchange { request: COUNTS, status: 200, body: COUNTS_BODY },
            Exchange { request: "GET /prefix/Items?recursive=true&includeItemTypes=Movie%2CEpisode%2CMusicAlbum%2CAudio%2CBook&sortBy=DateCreated&sortOrder=Descending&limit=10&enableImages=false&enableUserData=false&enableTotalRecordCount=false HTTP/1.1", status: 200, body: r#"{"Items":[{"Name":"New episode","Type":"Episode","SeriesName":"Sample series","ParentIndexNumber":2,"IndexNumber":5},{"Name":"Sample album","Type":"MusicAlbum","AlbumArtist":"Example artist","ProductionYear":2025}],"TotalRecordCount":0,"StartIndex":0}"# },
            Exchange { request: COUNTS, status: 200, body: r#"{"MovieCount":"many"}"# },
            Exchange { request: PROBE, status: 401, body: "" },
            Exchange { request: PROBE, status: 200, body: r#"[{"Client":"Jellyfin Web"}]"# },
        ], |_, wire| {
            assert_header(wire, &format!("authorization: MediaBrowser Token={KEY}"), KEY);
            assert!(!wire.lines().next().unwrap().contains(KEY));
            ""
        }, async |conn| {
            conn.run((JELLYFIN.test)(conn)).await.unwrap();
            let fetch = async |index: usize| {
                serde_json::to_value(super::super::fetch(conn, &JELLYFIN.widgets[index], &Values::new()).await.unwrap()).unwrap()
            };
            assert_eq!(fetch(0).await["rows"], json!([
                {"title":"Dune","secondary":"alex · Living room","value":"45m left","status":"up","progress":25},
                {"title":"Sample series · S01E02","secondary":"Jellyfin Android","value":"Paused","status":"warn"}
            ]));
            assert_eq!(fetch(0).await["message"], "Nothing is playing.");
            assert_eq!(fetch(0).await["message"], "Session list too large to show active streams.");
            assert_eq!(fetch(1).await["tiles"], json!([
                {"label":"Movies","value":"42","glyph":"film"},
                {"label":"Series","value":"3","glyph":"tv"},
                {"label":"Episodes","value":"105","glyph":"tv"},
                {"label":"Albums","value":"6","glyph":"music"},
                {"label":"Books","value":"4"}
            ]));
            assert_eq!(fetch(2).await["rows"], json!([
                {"title":"Sample series · S02E05","secondary":"Episode · New episode"},
                {"title":"Sample album","secondary":"Album · Example artist","value":"2025"}
            ]));
            let fault = super::super::fetch(conn, &JELLYFIN.widgets[1], &Values::new()).await.unwrap_err();
            assert_eq!(fault.class.message(), Class::Protocol.message());
            assert_test_fault(conn, &JELLYFIN, Class::Auth).await;
            assert_test_fault(conn, &JELLYFIN, Class::Protocol).await;
        }).await;
    }
}
