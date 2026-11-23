use super::*;
use crate::util::human_bytes;
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

const KEY: &str = "api_key";

pub const SABNZBD: Provider = Provider {
    id: "sabnzbd",
    name: "SABnzbd",
    category: "Downloads",
    icon: "sabnzbd",
    color: "#f2a900",
    description: "Show download speed, the queue with progress, recent history and free space.",
    docs: "https://sabnzbd.org/wiki/advanced/api",
    public_internet: false,
    contacts: "Your configured SABnzbd web interface.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(
                KEY,
                "API key",
                Kind::Secret,
                "Use the API Key from Config → General.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "queue",
            name: "Download queue",
            description:
                "Speed, queued jobs, what is left and free space on the incomplete-download drive.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Download","value":"42.0 MiB/s","glyph":"download","tone":"up"},{"label":"Queued jobs","value":"3","glyph":"database"},{"label":"Left","value":"8.4 GiB","glyph":"clock","delta":"0:03:24"},{"label":"Free space","value":"1.2 TiB","glyph":"disk"}]}"#,
            fields: &[REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "jobs",
            name: "Queued jobs",
            description: "The next ten jobs in the queue with progress, state, size and time left.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"Example.Show.S01E01.1080p","secondary":"Downloading · tv · 2.1 GiB","value":"0:01:12","status":"up","progress":64},{"title":"Sample.Movie.2024","secondary":"Queued · movies · 6.3 GiB","value":"0:04:40","progress":0},{"title":"example-album","secondary":"Paused · music · 512 MiB","value":"—","status":"warn","progress":12}]}"#,
            fields: &[REFRESH],
            fetch: jobs,
            validate: no_widget_options,
        },
        Widget {
            id: "history",
            name: "Recent history",
            description: "The last ten finished jobs, completed or failed, with size and category.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"Example.Show.S01E02.1080p","secondary":"Completed · tv","value":"2.0 GiB","status":"up"},{"title":"Sample.Movie.2023","secondary":"Failed · movies · Repair failed, not enough repair blocks","value":"5.8 GiB","status":"down"},{"title":"example-podcast","secondary":"Extracting · audio","value":"310 MiB","status":"warn"}]}"#,
            fields: &[REFRESH],
            fetch: history,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
struct Queue {
    noofslots: u64,
    noofslots_total: Option<u64>,
    kbpersec: String,
    diskspace1: String,
    #[serde(default)]
    mbleft: Option<String>,
    #[serde(default)]
    timeleft: Option<String>,
    #[serde(default)]
    status: Option<String>,
    /// Read only by the job list, so a slot shape it does not need never
    /// fails the summary.
    #[serde(default)]
    slots: Value,
}

#[derive(Deserialize)]
struct Slot {
    filename: String,
    status: String,
    percentage: String,
    mb: String,
    timeleft: Option<String>,
    cat: Option<String>,
}

#[derive(Deserialize)]
struct History {
    slots: Vec<Finished>,
}

#[derive(Deserialize)]
struct Finished {
    name: String,
    status: String,
    bytes: Option<u64>,
    category: Option<String>,
    fail_message: Option<String>,
}

struct Metrics {
    slots: u64,
    speed: u64,
    free: u64,
    left: Option<u64>,
    timeleft: Option<String>,
    paused: bool,
}

fn bytes(value: &str, scale: f64) -> Result<u64, Fault> {
    let bytes = value.parse::<f64>().map_err(|_| Class::Protocol)? * scale;
    if !(0.0..u64::MAX as f64).contains(&bytes) {
        return Err(Class::Protocol.into());
    }
    Ok(bytes.round() as u64)
}

const MIB: f64 = 1024.0 * 1024.0;

/// One API call; `mode` names the response object (`queue` or `history`).
async fn api<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    mode: &'static str,
    limit: &'static str,
) -> Result<T, Fault> {
    #[derive(Deserialize)]
    struct Envelope {
        queue: Option<Value>,
        history: Option<Value>,
        error: Option<String>,
    }
    let response: Envelope = parse_json(
        conn.request(Step {
            verb: Verb::Post,
            body: Some(Body::Form(&[
                ("mode", Input::Text(mode)),
                ("output", Input::Text("json")),
                ("limit", Input::Text(limit)),
                ("apikey", Input::Credential(KEY)),
            ])),
            ..Step::get("api")
        })
        .await?,
    )?;
    if let Some(error) = response.error {
        return Err(match error.as_str() {
            "API Key Incorrect" | "API Key Required" | "Missing authentication" => Class::Auth,
            _ => Class::Protocol,
        }
        .into());
    }
    let value = if mode == "queue" {
        response.queue
    } else {
        response.history
    };
    serde_json::from_value(value.ok_or(Class::Protocol)?).map_err(|_| Class::Protocol.into())
}

fn metrics(queue: &Queue) -> Result<Metrics, Fault> {
    Ok(Metrics {
        slots: queue.noofslots_total.unwrap_or(queue.noofslots),
        speed: bytes(&queue.kbpersec, 1024.0)?,
        free: bytes(&queue.diskspace1, 1024.0_f64.powi(3))?,
        left: queue
            .mbleft
            .as_deref()
            .map(|left| bytes(left, MIB))
            .transpose()?,
        timeleft: queue.timeleft.clone(),
        paused: queue.status.as_deref() == Some("Paused"),
    })
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        let queue: Queue = api(conn, "queue", "1").await?;
        metrics(&queue).map(|_| ())
    })
}

fn tile(label: &str, value: String, glyph: Glyph) -> StatTile {
    StatTile {
        label: label.into(),
        value,
        glyph: Some(glyph),
        delta: None,
        tone: None,
        series: Vec::new(),
    }
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let queue: Queue = api(conn, "queue", "1").await?;
        let metrics = metrics(&queue)?;
        let mut speed = tile(
            "Download",
            format!("{}/s", human_bytes(metrics.speed)),
            Glyph::Download,
        );
        if metrics.paused {
            speed.delta = Some("Paused".into());
            speed.tone = Some(Health::Warn);
        } else if metrics.speed > 0 {
            speed.tone = Some(Health::Up);
        }
        let mut tiles = vec![
            speed,
            tile("Queued jobs", metrics.slots.to_string(), Glyph::Database),
        ];
        if let Some(left) = metrics.left {
            let mut left = tile("Left", human_bytes(left), Glyph::Clock);
            left.delta = metrics
                .timeleft
                .filter(|time| metrics.speed > 0 && time != "0:00:00")
                .map(|time| display_text(conn, time));
            tiles.push(left);
        }
        tiles.push(tile("Free space", human_bytes(metrics.free), Glyph::Disk));
        Ok(StructuredBody::Tiles { tiles })
    })
}

fn health(status: &str) -> Option<Health> {
    match status {
        "Downloading" | "Completed" => Some(Health::Up),
        "Failed" => Some(Health::Down),
        "Queued" | "Idle" => None,
        _ => Some(Health::Warn),
    }
}

fn label(conn: &Conn<'_>, parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty() && part != "None" && part != "*")
        .map(|part| display_text(conn, part))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn jobs<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(queue) = within_response_limit(api::<Queue>(conn, "queue", "10").await)? else {
            return Ok(StructuredBody::Unavailable {
                message: "Queue too large to show.".into(),
            });
        };
        let slots: Vec<Slot> = serde_json::from_value(queue.slots).map_err(|_| Class::Protocol)?;
        if slots.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "The queue is empty.".into(),
            });
        }
        let rows = slots
            .into_iter()
            .take(10)
            .map(|slot| {
                let size = bytes(&slot.mb, MIB)?;
                let progress = slot.percentage.parse::<u8>().map_err(|_| Class::Protocol)?;
                let time = slot.timeleft.filter(|time| time != "0:00:00");
                Ok(BodyRow {
                    title: display_text(conn, slot.filename),
                    secondary: Some(label(
                        conn,
                        [Some(slot.status.clone()), slot.cat, Some(human_bytes(size))],
                    )),
                    value: Some(time.map_or_else(|| "—".into(), |time| display_text(conn, time))),
                    status: health(&slot.status),
                    progress: Some(progress),
                    bars: Vec::new(),
                })
            })
            .collect::<Result<_, Fault>>()?;
        Ok(StructuredBody::Rows { rows })
    })
}

fn history<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(history) = within_response_limit(api::<History>(conn, "history", "10").await)?
        else {
            return Ok(StructuredBody::Unavailable {
                message: "History too large to show.".into(),
            });
        };
        if history.slots.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No finished jobs yet.".into(),
            });
        }
        let rows = history
            .slots
            .into_iter()
            .take(10)
            .map(|job| BodyRow {
                title: display_text(conn, job.name),
                secondary: Some(label(
                    conn,
                    [
                        Some(job.status.clone()),
                        job.category,
                        job.fail_message.filter(|_| job.status == "Failed"),
                    ],
                )),
                value: job.bytes.map(human_bytes),
                status: health(&job.status),
                progress: None,
                bars: Vec::new(),
            })
            .collect();
        Ok(StructuredBody::Rows { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    const PATH: &str = "POST /prefix/api HTTP/1.1";

    /// The key travels only in the form body, URL-encoded.
    fn key_in_body(mode: &str, limit: &str, wire: &str) {
        let (headers, body) = wire.split_once("\r\n\r\n").unwrap();
        assert!(!headers.contains("fake"));
        assert!(headers.contains("content-type: application/x-www-form-urlencoded"));
        assert_eq!(
            body,
            format!("mode={mode}&output=json&limit={limit}&apikey=fake%26sab")
        );
    }

    #[tokio::test]
    async fn reads_limited_queue_with_key_only_in_body_and_maps_http_errors() {
        const BODY: &str = r#"{"queue":{"noofslots":9,"noofslots_total":9,"kbpersec":"1024.00","diskspace1":"12.50","slots":[{"filename":"first item"}]}}"#;
        const FULL: &str = r#"{"queue":{"status":"Paused","noofslots":1,"noofslots_total":3,"kbpersec":"0.00","diskspace1":"100.00","mbleft":"8601.60","mb":"9000.00","timeleft":"0:00:00","slots":[]}}"#;
        exercise(
            &SABNZBD,
            &[(KEY, "fake&sab")],
            Values::new(),
            &[
                Exchange { request: PATH, status: 200, body: BODY },
                Exchange { request: PATH, status: 200, body: BODY },
                Exchange { request: PATH, status: 200, body: FULL },
                Exchange { request: PATH, status: 403, body: "API Key Incorrect" },
                Exchange { request: PATH, status: 200, body: r#"{"queue":{}}"# },
                // api.py report(error=...) answers 200 with a JSON status.
                Exchange { request: PATH, status: 200, body: r#"{"status":false,"error":"API Key Incorrect"}"# },
                Exchange { request: PATH, status: 200, body: r#"{"status":false,"error":"API Key Required"}"# },
                Exchange { request: PATH, status: 200, body: r#"{"status":false,"error":"not implemented"}"# },
            ],
            |_, wire| {
                key_in_body("queue", "1", wire);
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let fetch = async || {
                    let body = super::super::fetch(conn, &SABNZBD.widgets[0], &Values::new())
                        .await
                        .unwrap();
                    serde_json::to_value(body).unwrap()["tiles"].clone()
                };
                assert_eq!(
                    fetch().await,
                    json!([
                        {"label": "Download", "value": "1.0 MiB/s", "glyph": "download", "tone": "up"},
                        {"label": "Queued jobs", "value": "9", "glyph": "database"},
                        {"label": "Free space", "value": "12.5 GiB", "glyph": "disk"},
                    ])
                );
                assert_eq!(
                    fetch().await,
                    json!([
                        {"label": "Download", "value": "0 B/s", "glyph": "download", "delta": "Paused", "tone": "warn"},
                        {"label": "Queued jobs", "value": "3", "glyph": "database"},
                        {"label": "Left", "value": "8.4 GiB", "glyph": "clock"},
                        {"label": "Free space", "value": "100 GiB", "glyph": "disk"},
                    ])
                );
                assert_test_fault(conn, &SABNZBD, Class::Auth).await;
                assert_test_fault(conn, &SABNZBD, Class::Protocol).await;
                assert_test_fault(conn, &SABNZBD, Class::Auth).await;
                assert_test_fault(conn, &SABNZBD, Class::Auth).await;
                assert_test_fault(conn, &SABNZBD, Class::Protocol).await;
            },
        )
        .await;
    }

    #[tokio::test]
    async fn lists_queued_jobs_and_recent_history() {
        // Slot and history shapes from sabnzbd 4.5.5 api.py and database.py.
        const QUEUE: &str = r#"{"queue":{"status":"Downloading","noofslots":2,"noofslots_total":2,"kbpersec":"30720.00","diskspace1":"500.00","mbleft":"3000.00","timeleft":"0:01:40","slots":[
            {"index":0,"nzo_id":"SABnzbd_nzo_1","filename":"Example.Show.S01E01.1080p","cat":"tv","priority":"Normal","status":"Downloading","mb":"2150.40","mbleft":"774.14","percentage":"64","size":"2.1 GB","sizeleft":"774 MB","timeleft":"0:01:12","labels":[]},
            {"index":1,"nzo_id":"SABnzbd_nzo_2","filename":"Sample.Movie.2024","cat":"None","status":"Queued","mb":"6451.20","mbleft":"6451.20","percentage":"0","timeleft":"0:00:00"}]}}"#;
        const HISTORY: &str = r#"{"history":{"noofslots":2,"day_size":"2.0 G","slots":[
            {"name":"Example.Show.S01E02.1080p","nzo_id":"SABnzbd_nzo_3","status":"Completed","bytes":2147483648,"category":"tv","fail_message":"","completed":1700000000,"stage_log":[]},
            {"name":"Sample.Movie.2023","status":"Failed","bytes":6227702579,"category":"movies","fail_message":"Repair failed, not enough repair blocks"}]}}"#;
        exercise(
            &SABNZBD,
            &[(KEY, "fake&sab")],
            Values::new(),
            &[
                Exchange { request: PATH, status: 200, body: QUEUE },
                Exchange { request: PATH, status: 200, body: HISTORY },
                Exchange { request: PATH, status: 200, body: r#"{"queue":{"noofslots":0,"kbpersec":"0","diskspace1":"1","slots":[]}}"# },
                Exchange { request: PATH, status: 200, body: r#"{"queue":{"noofslots":1,"kbpersec":"0","diskspace1":"1","slots":[{"filename":"x","status":"Queued","mb":"1","percentage":"many"}]}}"# },
                Exchange { request: PATH, status: 200, body: r#"{"status":false,"error":"API Key Incorrect"}"# },
            ],
            |index, wire| {
                key_in_body(if index == 1 || index == 4 { "history" } else { "queue" }, "10", wire);
                ""
            },
            async |conn| {
                let fetch = async |widget: usize| {
                    super::super::fetch(conn, &SABNZBD.widgets[widget], &Values::new())
                        .await
                        .map(|body| serde_json::to_value(body).unwrap())
                };
                assert_eq!(
                    fetch(1).await.unwrap()["rows"],
                    json!([
                        {"title": "Example.Show.S01E01.1080p", "secondary": "Downloading · tv · 2.1 GiB", "value": "0:01:12", "status": "up", "progress": 64},
                        {"title": "Sample.Movie.2024", "secondary": "Queued · 6.3 GiB", "value": "—", "progress": 0},
                    ])
                );
                assert_eq!(
                    fetch(2).await.unwrap()["rows"],
                    json!([
                        {"title": "Example.Show.S01E02.1080p", "secondary": "Completed · tv", "value": "2.0 GiB", "status": "up"},
                        {"title": "Sample.Movie.2023", "secondary": "Failed · movies · Repair failed, not enough repair blocks", "value": "5.8 GiB", "status": "down"},
                    ])
                );
                assert_eq!(fetch(1).await.unwrap()["message"], "The queue is empty.");
                assert!(matches!(fetch(1).await.unwrap_err().class, Class::Protocol));
                assert!(matches!(fetch(2).await.unwrap_err().class, Class::Auth));
            },
        )
        .await;
    }
}
