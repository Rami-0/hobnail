use super::*;
use crate::util::{fmt_utc, unix_now};
use widgets::{BodyItem, StructuredBody};

pub(super) const FIELDS: &[Field] = &[
    SERVICE_URL,
    Field {
        required: true,
        max_length: Some(256),
        ..Field::new(
            "api_key",
            "API key",
            Kind::Secret,
            "Find the API key in Settings → General.",
        )
    },
    ENABLED,
];

pub(super) async fn get<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    path: &str,
    query: &[(&str, Input<'_>)],
) -> Result<T, Fault> {
    parse_json(
        conn.request(Step {
            auth: &[Auth::Header("x-api-key", Input::Credential("api_key"))],
            query,
            ..Step::get(path)
        })
        .await?,
    )
}

#[derive(Deserialize)]
struct HealthIssue {
    #[serde(rename = "type")]
    severity: String,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Queue {
    total_count: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Missing {
    total_records: u64,
}

pub(super) fn test_v3<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        get::<Vec<HealthIssue>>(conn, "api/v3/health", &[])
            .await
            .map(|_| ())
    })
}

pub(super) fn overview_v3<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let queue: Queue = get(conn, "api/v3/queue/status", &[]).await?;
        let missing: Missing = get(
            conn,
            "api/v3/wanted/missing",
            &[
                ("page", Input::Text("1")),
                ("pageSize", Input::Text("1")),
                ("monitored", Input::Text("true")),
            ],
        )
        .await?;
        let issues =
            within_response_limit(get::<Vec<HealthIssue>>(conn, "api/v3/health", &[]).await)?;
        let mut rows = vec![
            ("Queue".into(), queue.total_count.to_string()),
            ("Missing".into(), missing.total_records.to_string()),
            (
                "Health issues".into(),
                issues
                    .as_ref()
                    .map_or_else(|| "Unavailable".into(), |issues| issues.len().to_string()),
            ),
        ];
        rows.extend(issues.into_iter().flatten().take(5).map(|issue| {
            (
                display_text(conn, issue.severity),
                display_text(conn, issue.message),
            )
        }));
        Ok(StructuredBody::StatGrid { rows })
    })
}

pub(super) async fn calendar<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    extra: &[(&str, &str)],
) -> Result<Option<Vec<T>>, Fault> {
    let now = unix_now();
    let start = fmt_utc(now);
    let end = fmt_utc(now + 2 * 86_400);
    let mut query = vec![("start", Input::Text(&start)), ("end", Input::Text(&end))];
    query.extend(extra.iter().map(|&(key, text)| (key, Input::Text(text))));
    within_response_limit(get(conn, "api/v3/calendar", &query).await)
}

pub(super) fn upcoming<T>(
    entries: Option<Vec<T>>,
    item: impl FnMut(T) -> BodyItem,
) -> StructuredBody {
    let Some(entries) = entries else {
        return StructuredBody::Unavailable {
            message: "Calendar response too large for the next 48 hours.".into(),
        };
    };
    let items = entries.into_iter().take(10).map(item).collect::<Vec<_>>();
    if items.is_empty() {
        StructuredBody::Unavailable {
            message: "Nothing upcoming in the next 48 hours.".into(),
        }
    } else {
        StructuredBody::ItemList { items }
    }
}
