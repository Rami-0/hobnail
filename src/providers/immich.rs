use super::*;
use crate::util::human_bytes;
use std::collections::BTreeMap;
use widgets::StructuredBody;

const KEY: &str = "api_key";
const STORAGE: &str = "api/server/storage";

pub const IMMICH: Provider = Provider {
    id: "immich",
    name: "Immich",
    category: "Media",
    icon: "immich",
    description: "Show photo and video totals, disk usage and background jobs.",
    docs: "https://api.immich.app/",
    public_internet: false,
    contacts: "Your configured Immich server.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(
                KEY,
                "API key",
                Kind::Secret,
                "Use an admin API key with server.statistics, server.storage and job.read.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "overview",
        name: "Immich overview",
        description: "Photos, videos, disk usage and active, waiting and failed background jobs.",
        recommended: true,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
struct Statistics {
    photos: u64,
    videos: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Storage {
    disk_size_raw: u64,
    disk_use_raw: u64,
    disk_available_raw: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Queue {
    job_counts: Counts,
}

#[derive(Clone, Copy, Default, Deserialize)]
struct Counts {
    active: u64,
    waiting: u64,
    failed: u64,
}

impl Counts {
    fn add(self, other: Self) -> Option<Self> {
        Some(Self {
            active: self.active.checked_add(other.active)?,
            waiting: self.waiting.checked_add(other.waiting)?,
            failed: self.failed.checked_add(other.failed)?,
        })
    }
}

async fn request(conn: &Conn<'_>, path: &str) -> Result<client::Response, Fault> {
    conn.request(Step {
        auth: &[Auth::Header("x-api-key", Input::Credential(KEY))],
        ..Step::get(path)
    })
    .await
}

async fn get<T: serde::de::DeserializeOwned>(conn: &Conn<'_>, path: &str) -> Result<T, Fault> {
    parse_json(request(conn, path).await?)
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { get::<Storage>(conn, STORAGE).await.map(|_| ()) })
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let storage = within_response_limit(get::<Storage>(conn, STORAGE).await)?;
        let statistics =
            within_response_limit(get::<Statistics>(conn, "api/server/statistics").await)?;
        // A release without the jobs route leaves only the job rows unavailable.
        let jobs = within_response_limit(
            request(conn, "api/jobs")
                .await
                .and_then(parse_optional::<BTreeMap<String, Queue>>),
        )?
        .flatten();
        let mut rows = vec![];
        match storage {
            Some(storage) => {
                rows.push((
                    "Disk usage".into(),
                    format!(
                        "{} / {}",
                        human_bytes(storage.disk_use_raw),
                        human_bytes(storage.disk_size_raw)
                    ),
                ));
                rows.push((
                    "Disk available".into(),
                    human_bytes(storage.disk_available_raw),
                ));
            }
            None => rows.push(("Storage".into(), "Unavailable".into())),
        }
        match statistics {
            Some(stats) => rows.extend([
                ("Photos".into(), stats.photos.to_string()),
                ("Videos".into(), stats.videos.to_string()),
            ]),
            None => rows.extend([
                ("Photos".into(), "Unavailable".into()),
                ("Videos".into(), "Unavailable".into()),
            ]),
        }
        match jobs {
            Some(jobs) if !jobs.is_empty() => {
                let counts = jobs
                    .into_values()
                    .try_fold(Counts::default(), |total, queue| {
                        total.add(queue.job_counts)
                    })
                    .ok_or(Class::Protocol)?;
                rows.extend([
                    ("Active jobs".into(), counts.active.to_string()),
                    ("Waiting jobs".into(), counts.waiting.to_string()),
                    ("Failed jobs".into(), counts.failed.to_string()),
                ]);
            }
            Some(_) => return Err(Class::Protocol.into()),
            None => rows.push(("Jobs".into(), "Unavailable".into())),
        }
        Ok(StructuredBody::StatGrid { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn reads_immich_metrics_and_keeps_storage_and_jobs_when_statistics_is_oversized() {
        const SPACE: &str = "GET /prefix/api/server/storage HTTP/1.1";
        const STATISTICS: &str = "GET /prefix/api/server/statistics HTTP/1.1";
        const JOBS: &str = "GET /prefix/api/jobs HTTP/1.1";
        const SPACE_BODY: &str =
            r#"{"diskSizeRaw":1073741824,"diskUseRaw":1048576,"diskAvailableRaw":1048576}"#;
        const JOBS_BODY: &str = r#"{"editor":{"jobCounts":{"active":2,"waiting":3,"failed":4}},"ocr":{"jobCounts":{"active":5,"waiting":6,"failed":7}}}"#;
        let oversized = format!(r#"{{"padding":"{}"}}"#, "x".repeat(MAX_RESPONSE));
        exercise(
            &IMMICH,
            &[(KEY, "fake-immich-key")],
            Values::new(),
            &[
                Exchange {
                    request: SPACE,
                    status: 200,
                    body: SPACE_BODY,
                },
                Exchange {
                    request: SPACE,
                    status: 200,
                    body: SPACE_BODY,
                },
                Exchange {
                    request: STATISTICS,
                    status: 200,
                    body: r#"{"photos":100,"videos":3,"usageByUser":[]}"#,
                },
                Exchange {
                    request: JOBS,
                    status: 200,
                    body: JOBS_BODY,
                },
                Exchange {
                    request: SPACE,
                    status: 200,
                    body: SPACE_BODY,
                },
                Exchange {
                    request: STATISTICS,
                    status: 200,
                    body: &oversized,
                },
                Exchange {
                    request: JOBS,
                    status: 200,
                    body: JOBS_BODY,
                },
                Exchange {
                    request: SPACE,
                    status: 200,
                    body: SPACE_BODY,
                },
                Exchange {
                    request: STATISTICS,
                    status: 200,
                    body: r#"{"photos":100,"videos":3,"usageByUser":[]}"#,
                },
                Exchange {
                    request: JOBS,
                    status: 404,
                    body: r#"{"message":"Cannot GET /api/jobs","error":"Not Found","statusCode":404}"#,
                },
                Exchange {
                    request: SPACE,
                    status: 403,
                    body: "",
                },
                Exchange {
                    request: SPACE,
                    status: 200,
                    body: "{}",
                },
            ],
            |_, wire| {
                assert_header(wire, "x-api-key: fake-immich-key", "fake-immich-key");
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let fetch = async || {
                    let body = super::super::fetch(conn, &IMMICH.widgets[0], &Values::new())
                        .await
                        .unwrap();
                    serde_json::to_value(body).unwrap()
                };
                let mut rows = json!([
                    ["Disk usage", "1.0 MiB / 1.0 GiB"],
                    ["Disk available", "1.0 MiB"],
                    ["Photos", "100"],
                    ["Videos", "3"],
                    ["Active jobs", "7"],
                    ["Waiting jobs", "9"],
                    ["Failed jobs", "11"],
                ]);
                assert_eq!(fetch().await["rows"], rows);
                rows[2][1] = json!("Unavailable");
                rows[3][1] = json!("Unavailable");
                assert_eq!(fetch().await["rows"], rows);
                assert_eq!(
                    fetch().await["rows"],
                    json!([
                        ["Disk usage", "1.0 MiB / 1.0 GiB"],
                        ["Disk available", "1.0 MiB"],
                        ["Photos", "100"],
                        ["Videos", "3"],
                        ["Jobs", "Unavailable"],
                    ])
                );
                assert_test_fault(conn, &IMMICH, Class::Auth).await;
                assert_test_fault(conn, &IMMICH, Class::Protocol).await;
            },
        )
        .await;
    }
}
