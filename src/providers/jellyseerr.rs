use super::*;
use widgets::{Glyph, Health, StatTile, StructuredBody};

pub const JELLYSEERR: Provider = Provider {
    id: "jellyseerr",
    name: "Jellyseerr / Overseerr",
    category: "Media",
    icon: "jellyseerr",
    color: "#7c4ddb",
    detect: super::detect(&["jellyseerr", "overseerr"], 5055),
    description: "Show media requests by status and reported issues.",
    docs: "https://api-docs.overseerr.dev/",
    public_internet: false,
    contacts:
        "Your configured Jellyseerr or Overseerr service. Public addresses require internet access.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(
                "api_key",
                "API key",
                Kind::Secret,
                "In Jellyseerr or Overseerr, open Settings → General and copy the API Key.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "requests",
            name: "Media requests",
            description: "Requests waiting for approval, approved, processing and available.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Pending","value":"4","glyph":"clock","tone":"warn"},{"label":"Approved","value":"18","glyph":"check"},{"label":"Processing","value":"2","glyph":"download"},{"label":"Available","value":"130","glyph":"play"}]}"#,
            fields: &[REFRESH],
            fetch: requests,
            validate: no_widget_options,
        },
        Widget {
            id: "issues",
            name: "Reported issues",
            description:
                "Open and closed issues users reported about media, and all issues by type.",
            recommended: false,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Open issues","value":"3","glyph":"alert","tone":"warn"},{"label":"Closed","value":"27","glyph":"check"},{"label":"Video","value":"9","glyph":"film"},{"label":"Audio","value":"4","glyph":"music"},{"label":"Subtitles","value":"12"},{"label":"Other","value":"5"}]}"#,
            fields: &[REFRESH],
            fetch: issues,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
struct Counts {
    pending: u64,
    approved: u64,
    #[serde(default)]
    processing: u64,
    #[serde(default)]
    available: u64,
}

#[derive(Deserialize)]
struct Issues {
    open: u64,
    closed: u64,
    #[serde(default)]
    video: u64,
    #[serde(default)]
    audio: u64,
    #[serde(default)]
    subtitles: u64,
    #[serde(default)]
    others: u64,
}

async fn get<T: serde::de::DeserializeOwned>(conn: &Conn<'_>, path: &str) -> Result<T, Fault> {
    parse_json(
        conn.request(Step {
            auth: &[Auth::Header("x-api-key", Input::Credential("api_key"))],
            ..Step::get(path)
        })
        .await?,
    )
}

fn tile(label: &str, count: u64, glyph: Option<Glyph>, tone: Option<Health>) -> StatTile {
    StatTile {
        label: label.into(),
        value: super::jellyfin::grouped(count),
        glyph,
        delta: None,
        tone,
        series: Vec::new(),
    }
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        get::<Counts>(conn, "api/v1/request/count")
            .await
            .map(|_| ())
    })
}

fn requests<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let counts: Counts = get(conn, "api/v1/request/count").await?;
        let waiting = (counts.pending > 0).then_some(Health::Warn);
        Ok(StructuredBody::Tiles {
            tiles: vec![
                tile("Pending", counts.pending, Some(Glyph::Clock), waiting),
                tile("Approved", counts.approved, Some(Glyph::Check), None),
                tile("Processing", counts.processing, Some(Glyph::Download), None),
                tile("Available", counts.available, Some(Glyph::Play), None),
            ],
        })
    })
}

fn issues<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let issues: Issues = get(conn, "api/v1/issue/count").await?;
        let open = Some(if issues.open > 0 {
            Health::Warn
        } else {
            Health::Up
        });
        Ok(StructuredBody::Tiles {
            tiles: vec![
                tile("Open issues", issues.open, Some(Glyph::Alert), open),
                tile("Closed", issues.closed, Some(Glyph::Check), None),
                tile("Video", issues.video, Some(Glyph::Film), None),
                tile("Audio", issues.audio, Some(Glyph::Music), None),
                tile("Subtitles", issues.subtitles, None, None),
                tile("Other", issues.others, None, None),
            ],
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use serde_json::json;

    #[tokio::test]
    async fn reads_request_and_issue_counts_with_header_key() {
        const REQUEST: &str = "GET /prefix/api/v1/request/count HTTP/1.1";
        const ISSUES: &str = "GET /prefix/api/v1/issue/count HTTP/1.1";
        const KEY: &str = "fake-requests-api-key";
        const COUNTS: &str = r#"{"total":28,"movie":18,"tv":10,"pending":5,"approved":20,"declined":3,"processing":7,"available":13,"completed":2}"#;
        exercise(&JELLYSEERR, &[("api_key", KEY)], Values::new(), &[
            Exchange { request: REQUEST, status: 200, body: COUNTS },
            Exchange { request: REQUEST, status: 200, body: COUNTS },
            Exchange { request: ISSUES, status: 200, body: r#"{"total":10,"video":4,"audio":3,"subtitles":2,"others":1,"open":0,"closed":10}"# },
            Exchange { request: ISSUES, status: 403, body: r#"{"status":403,"error":"You do not have permission to access this endpoint"}"# },
            Exchange { request: ISSUES, status: 200, body: r#"{"open":"many"}"# },
            Exchange { request: REQUEST, status: 403, body: r#"{"status":403,"error":"You do not have permission to access this endpoint"}"# },
            Exchange { request: REQUEST, status: 200, body: r#"{"pending":5,"approved":"20"}"# },
        ], |_, wire| {
            assert_header(wire, &format!("x-api-key: {KEY}"), KEY);
            assert!(!wire.lines().next().unwrap().contains(KEY));
            ""
        }, async |conn| {
            conn.run((JELLYSEERR.test)(conn)).await.unwrap();
            let fetch = async |index: usize| super::super::fetch(conn, &JELLYSEERR.widgets[index], &Values::new()).await.map(|body| serde_json::to_value(body).unwrap());
            assert_eq!(fetch(0).await.unwrap()["tiles"], json!([
                {"label":"Pending","value":"5","glyph":"clock","tone":"warn"},
                {"label":"Approved","value":"20","glyph":"check"},
                {"label":"Processing","value":"7","glyph":"download"},
                {"label":"Available","value":"13","glyph":"play"}
            ]));
            let issues = fetch(1).await.unwrap();
            assert_eq!(issues["tiles"][0], json!({"label":"Open issues","value":"0","glyph":"alert","tone":"up"}));
            assert_eq!(issues["tiles"][5], json!({"label":"Other","value":"1"}));
            assert_eq!(fetch(1).await.unwrap_err().class.message(), Class::Auth.message());
            assert_eq!(fetch(1).await.unwrap_err().class.message(), Class::Protocol.message());
            assert_test_fault(conn, &JELLYSEERR, Class::Auth).await;
            assert_test_fault(conn, &JELLYSEERR, Class::Protocol).await;
        }).await;
    }
}
