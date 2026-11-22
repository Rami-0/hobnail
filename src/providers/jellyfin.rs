use super::*;
use widgets::{BodyItem, StructuredBody};

pub const JELLYFIN: Provider = Provider {
    id: "jellyfin",
    name: "Jellyfin",
    category: "Media",
    icon: "jellyfin",
    description: "Show active streams, library counts and recently added media.",
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
                "Create an API key in the Jellyfin dashboard.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "streams",
            name: "Active streams",
            description: "Number of playback sessions and what they are playing.",
            recommended: true,
            fields: &[REFRESH],
            fetch: streams,
            validate: no_widget_options,
        },
        Widget {
            id: "library",
            name: "Library counts",
            description: "Movies, series, episodes, albums, songs and books.",
            recommended: true,
            fields: &[REFRESH],
            fetch: library,
            validate: no_widget_options,
        },
        Widget {
            id: "latest",
            name: "Latest added",
            description: "Ten recent movies, episodes, albums, songs or books.",
            recommended: false,
            fields: &[REFRESH],
            fetch: latest,
            validate: no_widget_options,
        },
    ],
};

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
    client: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Media {
    name: Option<String>,
    #[serde(rename = "Type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Latest {
    items: Vec<Media>,
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
        let mut rows = vec![("Active streams".into(), "0".into())];
        let mut count = 0;
        for session in sessions {
            if let Some(item) = session.now_playing_item {
                count += 1;
                if count <= 10 {
                    rows.push((
                        display_text(conn, item.name.unwrap_or_else(|| "Untitled media".into())),
                        display_text(
                            conn,
                            session.client.unwrap_or_else(|| "Unknown client".into()),
                        ),
                    ));
                }
            }
        }
        rows[0].1 = count.to_string();
        Ok(StructuredBody::StatGrid { rows })
    })
}

fn library<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let counts: Counts = get(conn, "Items/Counts", &[]).await?;
        let rows = [
            ("Movies", counts.movie_count),
            ("Series", counts.series_count),
            ("Episodes", counts.episode_count),
            ("Albums", counts.album_count),
            ("Songs", counts.song_count),
            ("Books", counts.book_count),
        ]
        .into_iter()
        .map(|(label, count)| (label.into(), count.to_string()))
        .collect();
        Ok(StructuredBody::StatGrid { rows })
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
        Ok(StructuredBody::ItemList {
            items: result
                .items
                .into_iter()
                .take(10)
                .map(|item| BodyItem {
                    icon: None,
                    title: display_text(conn, item.name.unwrap_or_else(|| "Untitled media".into())),
                    secondary: item.kind.map(|kind| display_text(conn, kind)),
                    link: None,
                    time: None,
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

    #[tokio::test]
    async fn reads_playback_counts_and_recent_items_with_api_key() {
        const KEY: &str = "abcdef0123456789abcdef0123456789";
        const PROBE: &str = "GET /prefix/Sessions?activeWithinSeconds=1 HTTP/1.1";
        const COUNTS: &str = "GET /prefix/Items/Counts HTTP/1.1";
        const COUNTS_BODY: &str = r#"{"MovieCount":42,"SeriesCount":3,"EpisodeCount":105,"AlbumCount":6,"SongCount":72,"MusicVideoCount":0,"BoxSetCount":0,"BookCount":4}"#;
        exercise(&JELLYFIN, &[("api_key", KEY)], Values::new(), &[
            Exchange { request: PROBE, status: 200, body: "[]" },
            Exchange { request: "GET /prefix/Sessions HTTP/1.1", status: 200, body: r#"[{"Id":"one","Client":"Jellyfin Web","NowPlayingItem":{"Name":"Dune","Type":"Movie"}},{"Id":"two","NowPlayingItem":null}]"# },
            Exchange { request: COUNTS, status: 200, body: COUNTS_BODY },
            Exchange { request: "GET /prefix/Items?recursive=true&includeItemTypes=Movie%2CEpisode%2CMusicAlbum%2CAudio%2CBook&sortBy=DateCreated&sortOrder=Descending&limit=10&enableImages=false&enableUserData=false&enableTotalRecordCount=false HTTP/1.1", status: 200, body: r#"{"Items":[{"Name":"New episode","Type":"Episode"}],"TotalRecordCount":0,"StartIndex":0}"# },
            Exchange { request: PROBE, status: 401, body: "" },
            Exchange { request: PROBE, status: 200, body: r#"[{"Client":"Jellyfin Web"}]"# },
        ], |_, wire| {
            assert_header(wire, &format!("authorization: MediaBrowser Token={KEY}"), KEY);
                ""
        }, async |conn| {
            conn.run((JELLYFIN.test)(conn)).await.unwrap();
            let fetch = async |index: usize| {
                serde_json::to_value(super::super::fetch(conn, &JELLYFIN.widgets[index], &Values::new()).await.unwrap()).unwrap()
            };
            assert_eq!(fetch(0).await["rows"], json!([["Active streams", "1"], ["Dune", "Jellyfin Web"]]));
            assert_eq!(fetch(1).await["rows"], json!([["Movies", "42"], ["Series", "3"], ["Episodes", "105"], ["Albums", "6"], ["Songs", "72"], ["Books", "4"]]));
            let recent = fetch(2).await;
            assert_eq!(recent["items"][0]["title"], "New episode");
            assert_eq!(recent["items"][0]["secondary"], "Episode");
            assert_test_fault(conn, &JELLYFIN, Class::Auth).await;
            assert_test_fault(conn, &JELLYFIN, Class::Protocol).await;
        }).await;
    }
}
