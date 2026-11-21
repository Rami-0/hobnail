use super::*;
use crate::util::human_bytes;
use widgets::StructuredBody;

const KEY: &str = "api_key";

pub const SABNZBD: Provider = Provider {
    id: "sabnzbd",
    name: "SABnzbd",
    category: "Downloads",
    icon: "sabnzbd",
    description: "Show the download queue, download speed and free space.",
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
    widgets: &[Widget {
        id: "queue",
        name: "Download queue",
        description: "Queue size, download speed and free space on the incomplete-download drive.",
        recommended: true,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
struct Envelope {
    queue: Option<Queue>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct Queue {
    noofslots: u64,
    noofslots_total: Option<u64>,
    kbpersec: String,
    diskspace1: String,
}

struct Metrics {
    slots: u64,
    speed: u64,
    free: u64,
}

fn bytes(value: &str, scale: f64) -> Result<u64, Fault> {
    let bytes = value.parse::<f64>().map_err(|_| Class::Protocol)? * scale;
    if !(0.0..u64::MAX as f64).contains(&bytes) {
        return Err(Class::Protocol.into());
    }
    Ok(bytes.round() as u64)
}

async fn metrics(conn: &Conn<'_>) -> Result<Metrics, Fault> {
    let response: Envelope = parse_json(
        conn.request(Step {
            query: &[
                ("mode", Input::Text("queue")),
                ("output", Input::Text("json")),
                ("limit", Input::Text("1")),
                ("apikey", Input::Credential(KEY)),
            ],
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
    let queue = response.queue.ok_or(Class::Protocol)?;
    Ok(Metrics {
        slots: queue.noofslots_total.unwrap_or(queue.noofslots),
        speed: bytes(&queue.kbpersec, 1024.0)?,
        free: bytes(&queue.diskspace1, 1024.0_f64.powi(3))?,
    })
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { metrics(conn).await.map(|_| ()) })
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let metrics = metrics(conn).await?;
        Ok(StructuredBody::StatGrid {
            rows: vec![
                ("Queued jobs".into(), metrics.slots.to_string()),
                (
                    "Download".into(),
                    format!("{}/s", human_bytes(metrics.speed)),
                ),
                ("Free space".into(), human_bytes(metrics.free)),
            ],
        })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn reads_limited_queue_with_key_only_in_query_and_maps_application_errors() {
        const PATH: &str =
            "GET /prefix/api?mode=queue&output=json&limit=1&apikey=fake%26sab HTTP/1.1";
        const BODY: &str = r#"{"queue":{"noofslots":9,"noofslots_total":9,"kbpersec":"1024.00","diskspace1":"12.50","slots":[{"filename":"first item"}]}}"#;
        exercise(
            &SABNZBD,
            &[(KEY, "fake&sab")],
            Values::new(),
            &[
                Exchange {
                    request: PATH,
                    status: 200,
                    body: BODY,
                },
                Exchange {
                    request: PATH,
                    status: 200,
                    body: BODY,
                },
                Exchange {
                    request: PATH,
                    status: 200,
                    body: r#"{"status":false,"error":"API Key Incorrect"}"#,
                },
                Exchange {
                    request: PATH,
                    status: 200,
                    body: r#"{"queue":{}}"#,
                },
            ],
            |_, wire| {
                assert!(!wire.split_once("\r\n").unwrap().1.contains("fake"));
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let body = super::super::fetch(conn, &SABNZBD.widgets[0], &Values::new())
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(body).unwrap()["rows"],
                    json!([
                        ["Queued jobs", "9"],
                        ["Download", "1.0 MiB/s"],
                        ["Free space", "12.5 GiB"],
                    ])
                );
                assert_test_fault(conn, &SABNZBD, Class::Auth).await;
                assert_test_fault(conn, &SABNZBD, Class::Protocol).await;
            },
        )
        .await;
    }
}
