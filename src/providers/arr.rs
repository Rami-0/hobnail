use super::*;
use crate::util::{fmt_utc, human_duration, unix_now};
use widgets::{AgendaEntry, BodyRow, Glyph, Health, StatTile, StructuredBody};

pub(super) const FIELDS: &[Field] = &[
    SERVICE_URL,
    Field {
        required: true,
        max_length: Some(256),
        ..Field::new(
            "api_key",
            "API key",
            Kind::Secret,
            "In the web interface, open Settings → General → Security and copy the API key.",
        )
    },
    ENABLED,
];

/// The most rows a list widget shows.
pub(super) const LIST: usize = 10;
/// Days ahead an agenda covers.
pub(super) const AGENDA_DAYS: u64 = 7;

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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Queue {
    total_count: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Page<T> {
    pub total_records: u64,
    #[serde(default = "Vec::new")]
    pub records: Vec<T>,
}

pub(super) fn test_v3<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        get::<Vec<serde::de::IgnoredAny>>(conn, "api/v3/health", &[])
            .await
            .map(|_| ())
    })
}

/// Queue, missing and health as tiles. `missing` names what is missing.
pub(super) async fn overview(
    conn: &Conn<'_>,
    missing_label: &str,
) -> Result<StructuredBody, Fault> {
    let queue: Queue = get(conn, "api/v3/queue/status", &[]).await?;
    let missing: Page<serde::de::IgnoredAny> = get(
        conn,
        "api/v3/wanted/missing",
        &[
            ("page", Input::Text("1")),
            ("pageSize", Input::Text("1")),
            ("monitored", Input::Text("true")),
        ],
    )
    .await?;
    let issues = within_response_limit(get::<Vec<HealthIssue>>(conn, "api/v3/health", &[]).await)?;
    let health = match &issues {
        None => StatTile {
            label: "Health".into(),
            value: "Unavailable".into(),
            glyph: Some(Glyph::Shield),
            delta: None,
            tone: None,
            series: Vec::new(),
        },
        Some(issues) => {
            let error = issues.iter().any(|issue| issue.severity == "error");
            StatTile {
                label: "Health".into(),
                value: match issues.len() {
                    0 => "OK".into(),
                    1 => "1 issue".into(),
                    n => format!("{n} issues"),
                },
                glyph: Some(if issues.is_empty() {
                    Glyph::Check
                } else {
                    Glyph::Alert
                }),
                delta: None,
                tone: Some(match (issues.is_empty(), error) {
                    (true, _) => Health::Up,
                    (false, true) => Health::Down,
                    (false, false) => Health::Warn,
                }),
                series: Vec::new(),
            }
        }
    };
    Ok(StructuredBody::Tiles {
        tiles: vec![
            tile("Queue", queue.total_count, Glyph::Download),
            tile(missing_label, missing.total_records, Glyph::Alert),
            health,
        ],
    })
}

fn tile(label: &str, count: u64, glyph: Glyph) -> StatTile {
    StatTile {
        label: label.into(),
        value: count.to_string(),
        glyph: Some(glyph),
        delta: None,
        tone: None,
        series: Vec::new(),
    }
}

pub(super) async fn calendar<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    extra: &[(&str, &str)],
) -> Result<Option<Vec<T>>, Fault> {
    let now = unix_now();
    // From the start of the UTC day, so something airing tonight still shows.
    let start = fmt_utc(now - now % 86_400);
    let end = fmt_utc(now + AGENDA_DAYS * 86_400);
    let mut query = vec![("start", Input::Text(&start)), ("end", Input::Text(&end))];
    query.extend(extra.iter().map(|&(key, text)| (key, Input::Text(text))));
    within_response_limit(get(conn, "api/v3/calendar", &query).await)
}

/// Calendar entries as an agenda, earliest first. `entry` returns None for
/// an entry without a usable date.
pub(super) fn agenda<T>(
    entries: Option<Vec<T>>,
    entry: impl FnMut(T) -> Option<AgendaEntry>,
) -> StructuredBody {
    let Some(entries) = entries else {
        return StructuredBody::Unavailable {
            message: "Calendar response too large for the next 7 days.".into(),
        };
    };
    let mut entries = entries.into_iter().filter_map(entry).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.time);
    entries.truncate(20);
    if entries.is_empty() {
        StructuredBody::Unavailable {
            message: "Nothing upcoming in the next 7 days.".into(),
        }
    } else {
        StructuredBody::Agenda { entries }
    }
}

/// An RFC 3339 time as unix seconds.
pub(super) fn unix(text: Option<&str>) -> Option<u64> {
    let (secs, _) = crate::logs::frame::parse_rfc3339_parts(text?)?;
    u64::try_from(secs).ok()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QueueRecord<M> {
    pub title: Option<String>,
    #[serde(default)]
    pub size: f64,
    #[serde(default)]
    pub sizeleft: f64,
    pub timeleft: Option<String>,
    pub status: Option<String>,
    pub tracked_download_status: Option<String>,
    #[serde(flatten)]
    pub media: M,
}

/// Download queue rows: `label` gives the title and secondary text.
pub(super) async fn queue<M: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    include: &[(&str, &str)],
    mut label: impl FnMut(&QueueRecord<M>) -> (Option<String>, Option<String>),
) -> Result<StructuredBody, Fault> {
    let size = LIST.to_string();
    let mut query = vec![("page", Input::Text("1")), ("pageSize", Input::Text(&size))];
    query.extend(include.iter().map(|&(key, text)| (key, Input::Text(text))));
    let Some(page) =
        within_response_limit(get::<Page<QueueRecord<M>>>(conn, "api/v3/queue", &query).await)?
    else {
        return Ok(StructuredBody::Unavailable {
            message: "Queue response too large to list.".into(),
        });
    };
    if page.records.is_empty() {
        return Ok(StructuredBody::Unavailable {
            message: "The download queue is empty.".into(),
        });
    }
    let rows = page
        .records
        .iter()
        .take(LIST)
        .map(|record| {
            let (title, secondary) = label(record);
            let state = record.status.as_deref().unwrap_or_default();
            let status = match record.tracked_download_status.as_deref() {
                Some("error") => Health::Down,
                Some("warning") => Health::Warn,
                _ if matches!(state, "failed" | "warning") => Health::Warn,
                _ => Health::Up,
            };
            let progress = (record.size > 0.0).then(|| {
                let done = (record.size - record.sizeleft.clamp(0.0, record.size)) / record.size;
                (done * 100.0).round() as u8
            });
            let value = if state == "downloading" {
                record.timeleft.as_deref().and_then(time_left)
            } else {
                None
            }
            .or_else(|| (!state.is_empty()).then(|| capitalize(state)));
            BodyRow {
                title: display_text(
                    conn,
                    title
                        .or_else(|| record.title.clone())
                        .unwrap_or_else(|| "Unknown download".into()),
                ),
                secondary: secondary.map(|text| display_text(conn, text)),
                value: value.map(|text| display_text(conn, text)),
                status: Some(status),
                progress,
                bars: Vec::new(),
            }
        })
        .collect();
    Ok(StructuredBody::Rows { rows })
}

/// `[d.]hh:mm:ss` as "3h 4m".
fn time_left(text: &str) -> Option<String> {
    let (days, clock) = text.split_once('.').unwrap_or(("0", text));
    let mut parts = clock.split(':').map(|part| part.parse::<u64>().ok());
    let (h, m, s) = (parts.next()??, parts.next()??, parts.next()??);
    let secs = days.parse::<u64>().ok()? * 86_400 + h * 3600 + m * 60 + s;
    Some(format!("{} left", human_duration(secs)))
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// The first page of monitored missing items in `sort` (key, direction).
pub(super) async fn missing<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    sort: (&str, &str),
    include: &[(&str, &str)],
    row: impl FnMut(T) -> BodyRow,
) -> Result<StructuredBody, Fault> {
    let size = LIST.to_string();
    let mut query = vec![
        ("page", Input::Text("1")),
        ("pageSize", Input::Text(&size)),
        ("monitored", Input::Text("true")),
        ("sortKey", Input::Text(sort.0)),
        ("sortDirection", Input::Text(sort.1)),
    ];
    query.extend(include.iter().map(|&(key, text)| (key, Input::Text(text))));
    let Some(page) =
        within_response_limit(get::<Page<T>>(conn, "api/v3/wanted/missing", &query).await)?
    else {
        return Ok(StructuredBody::Unavailable {
            message: "Missing list too large to show.".into(),
        });
    };
    if page.records.is_empty() {
        return Ok(StructuredBody::Unavailable {
            message: "Nothing monitored is missing.".into(),
        });
    }
    Ok(StructuredBody::Rows {
        rows: page.records.into_iter().take(LIST).map(row).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_time_left_and_dates() {
        assert_eq!(time_left("00:12:30").as_deref(), Some("12m left"));
        assert_eq!(time_left("1.02:00:00").as_deref(), Some("1d 2h left"));
        assert_eq!(time_left("soon"), None);
        assert_eq!(unix(Some("2026-11-22T02:00:00Z")), Some(1_795_312_800));
        assert_eq!(unix(Some("not a date")), None);
        assert_eq!(capitalize("downloading"), "Downloading");
    }
}
