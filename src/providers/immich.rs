use super::*;
use crate::util::human_bytes;
use std::collections::BTreeMap;
use widgets::{BodyRow, Glyph, Health, MeterRow, StatTile, StructuredBody};

const KEY: &str = "api_key";
const STORAGE: &str = "api/server/storage";

pub const IMMICH: Provider = Provider {
    id: "immich",
    name: "Immich",
    category: "Media",
    icon: "immich",
    color: "#4250af",
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
                "In Immich, open Account Settings → API Keys → New API Key, as an admin, with server.statistics, server.storage and job.read.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "overview",
            name: "Immich overview",
            description: "Photos, videos, disk usage and background job activity.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Photos","value":"24,310","glyph":"image"},{"label":"Videos","value":"1,204","glyph":"film"},{"label":"Storage","value":"182 GiB","glyph":"disk","delta":"of 1.8 TiB"},{"label":"Jobs","value":"2 active","glyph":"activity","tone":"up"}]}"#,
            fields: &[REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "storage",
            name: "Storage by user",
            description: "Server disk use and each user's library size, against their quota when one is set.",
            recommended: false,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"meters","rows":[{"label":"Server disk","percent":38,"detail":"702 GiB of 1.8 TiB"},{"label":"alex","percent":61,"detail":"122 GiB of 200 GiB quota"},{"label":"sam","percent":29,"detail":"58 GiB · 29% of all media"}]}"#,
            fields: &[REFRESH],
            fetch: storage,
            validate: no_widget_options,
        },
        Widget {
            id: "jobs",
            name: "Background jobs",
            description: "Immich's job queues, busiest first, with active, waiting and failed counts.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"Thumbnail generation","value":"2 active · 140 waiting","status":"up"},{"title":"Smart search","value":"3 failed","status":"warn"},{"title":"Metadata extraction","value":"Idle"}]}"#,
            fields: &[REFRESH],
            fetch: jobs,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Statistics {
    photos: u64,
    videos: u64,
    #[serde(default)]
    usage: u64,
    #[serde(default)]
    usage_by_user: Vec<UserUsage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserUsage {
    user_name: String,
    usage: u64,
    quota_size_in_bytes: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Storage {
    disk_size_raw: u64,
    disk_use_raw: u64,
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

    fn busy(self) -> u64 {
        self.active
            .saturating_add(self.waiting)
            .saturating_add(self.failed)
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

/// The job queues, or None for a release without the route.
async fn queues(conn: &Conn<'_>) -> Result<Option<BTreeMap<String, Queue>>, Fault> {
    let queues = within_response_limit(
        request(conn, "api/jobs")
            .await
            .and_then(parse_optional::<BTreeMap<String, Queue>>),
    )?
    .flatten();
    match queues {
        Some(queues) if queues.is_empty() => Err(Class::Protocol.into()),
        queues => Ok(queues),
    }
}

fn percent(part: u64, whole: u64) -> u8 {
    if whole == 0 {
        return 0;
    }
    (part as f64 / whole as f64 * 100.0).round().min(100.0) as u8
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

/// "thumbnailGeneration" → "Thumbnail generation".
fn queue_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else if i == 0 {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { get::<Storage>(conn, STORAGE).await.map(|_| ()) })
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let storage = within_response_limit(get::<Storage>(conn, STORAGE).await)?;
        let statistics =
            within_response_limit(get::<Statistics>(conn, "api/server/statistics").await)?;
        // A release without the jobs route leaves only the jobs tile unavailable.
        let jobs = queues(conn).await?;
        let unavailable = || "Unavailable".to_string();
        let grouped = super::jellyfin::grouped;
        let mut tiles = vec![
            tile(
                "Photos",
                statistics
                    .as_ref()
                    .map_or_else(unavailable, |s| grouped(s.photos)),
                Glyph::Image,
            ),
            tile(
                "Videos",
                statistics
                    .as_ref()
                    .map_or_else(unavailable, |s| grouped(s.videos)),
                Glyph::Film,
            ),
        ];
        let mut disk = tile(
            "Storage",
            storage
                .as_ref()
                .map_or_else(unavailable, |s| human_bytes(s.disk_use_raw)),
            Glyph::Disk,
        );
        disk.delta = storage.map(|s| format!("of {}", human_bytes(s.disk_size_raw)));
        tiles.push(disk);
        let mut activity = tile("Jobs", unavailable(), Glyph::Activity);
        if let Some(jobs) = jobs {
            let counts = jobs
                .into_values()
                .try_fold(Counts::default(), |total, queue| {
                    total.add(queue.job_counts)
                })
                .ok_or(Class::Protocol)?;
            activity.value = if counts.active > 0 {
                format!("{} active", counts.active)
            } else if counts.waiting > 0 {
                format!("{} waiting", counts.waiting)
            } else {
                "Idle".into()
            };
            if counts.failed > 0 {
                activity.delta = Some(format!("{} failed", counts.failed));
            }
            activity.tone = Some(if counts.failed > 0 {
                Health::Warn
            } else {
                Health::Up
            });
        }
        tiles.push(activity);
        Ok(StructuredBody::Tiles { tiles })
    })
}

fn storage<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let storage: Storage = get(conn, STORAGE).await?;
        let mut rows = vec![MeterRow {
            label: "Server disk".into(),
            percent: percent(storage.disk_use_raw, storage.disk_size_raw),
            detail: Some(format!(
                "{} of {}",
                human_bytes(storage.disk_use_raw),
                human_bytes(storage.disk_size_raw)
            )),
        }];
        let Some(mut statistics) =
            within_response_limit(get::<Statistics>(conn, "api/server/statistics").await)?
        else {
            return Ok(StructuredBody::Meters { rows });
        };
        statistics
            .usage_by_user
            .sort_by_key(|user| std::cmp::Reverse(user.usage));
        for user in statistics.usage_by_user.into_iter().take(8) {
            let (percent, detail) = match user.quota_size_in_bytes.filter(|&quota| quota > 0) {
                Some(quota) => (
                    percent(user.usage, quota),
                    format!(
                        "{} of {} quota",
                        human_bytes(user.usage),
                        human_bytes(quota)
                    ),
                ),
                None => {
                    let share = percent(user.usage, statistics.usage);
                    (
                        share,
                        format!("{} · {share}% of all media", human_bytes(user.usage)),
                    )
                }
            };
            rows.push(MeterRow {
                label: display_text(conn, user.user_name),
                percent,
                detail: Some(detail),
            });
        }
        Ok(StructuredBody::Meters { rows })
    })
}

fn jobs<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(queues) = queues(conn).await? else {
            return Ok(StructuredBody::Unavailable {
                message: "This Immich release has no job queue route.".into(),
            });
        };
        let mut queues: Vec<_> = queues.into_iter().collect();
        queues.sort_by(|a, b| {
            b.1.job_counts
                .busy()
                .cmp(&a.1.job_counts.busy())
                .then(a.0.cmp(&b.0))
        });
        let rows = queues
            .into_iter()
            .take(10)
            .map(|(name, queue)| {
                let counts = queue.job_counts;
                let parts: Vec<_> = [
                    (counts.active, "active"),
                    (counts.waiting, "waiting"),
                    (counts.failed, "failed"),
                ]
                .into_iter()
                .filter(|&(count, _)| count > 0)
                .map(|(count, word)| format!("{count} {word}"))
                .collect();
                BodyRow {
                    title: display_text(conn, queue_name(&name)),
                    value: Some(if parts.is_empty() {
                        "Idle".into()
                    } else {
                        parts.join(" · ")
                    }),
                    status: if counts.failed > 0 {
                        Some(Health::Warn)
                    } else if counts.active > 0 {
                        Some(Health::Up)
                    } else {
                        None
                    },
                    ..BodyRow::default()
                }
            })
            .collect();
        Ok(StructuredBody::Rows { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    const SPACE: &str = "GET /prefix/api/server/storage HTTP/1.1";
    const STATISTICS: &str = "GET /prefix/api/server/statistics HTTP/1.1";
    const JOBS: &str = "GET /prefix/api/jobs HTTP/1.1";
    const SPACE_BODY: &str = r#"{"diskSizeRaw":1073741824,"diskUseRaw":268435456,"diskAvailableRaw":805306368,"diskUsagePercentage":25}"#;
    const STATISTICS_BODY: &str = r#"{"photos":1200,"videos":3,"usage":4194304,"usagePhotos":3145728,"usageVideos":1048576,"usageByUser":[
        {"userId":"b","userName":"sam","photos":1,"videos":0,"usage":1048576,"usagePhotos":1048576,"usageVideos":0,"quotaSizeInBytes":null},
        {"userId":"a","userName":"alex","photos":2,"videos":3,"usage":3145728,"usagePhotos":2097152,"usageVideos":1048576,"quotaSizeInBytes":4194304}
    ]}"#;
    const JOBS_BODY: &str = r#"{"thumbnailGeneration":{"jobCounts":{"active":2,"waiting":3,"failed":0,"completed":0,"delayed":0,"paused":0}},"smartSearch":{"jobCounts":{"active":0,"waiting":0,"failed":7}},"ocr":{"jobCounts":{"active":0,"waiting":0,"failed":0}}}"#;

    #[test]
    fn names_queues() {
        assert_eq!(queue_name("thumbnailGeneration"), "Thumbnail generation");
        assert_eq!(queue_name("ocr"), "Ocr");
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(5, 0), 0);
    }

    #[tokio::test]
    async fn reads_overview_storage_and_jobs_with_header_key() {
        let oversized = format!(r#"{{"padding":"{}"}}"#, "x".repeat(MAX_RESPONSE));
        exercise(&IMMICH, &[(KEY, "fake-immich-key")], Values::new(), &[
            Exchange { request: SPACE, status: 200, body: SPACE_BODY },
            Exchange { request: SPACE, status: 200, body: SPACE_BODY },
            Exchange { request: STATISTICS, status: 200, body: STATISTICS_BODY },
            Exchange { request: JOBS, status: 200, body: JOBS_BODY },
            Exchange { request: SPACE, status: 200, body: SPACE_BODY },
            Exchange { request: STATISTICS, status: 200, body: &oversized },
            Exchange { request: JOBS, status: 404, body: r#"{"message":"Cannot GET /api/jobs","error":"Not Found","statusCode":404}"# },
            Exchange { request: SPACE, status: 200, body: SPACE_BODY },
            Exchange { request: STATISTICS, status: 200, body: STATISTICS_BODY },
            Exchange { request: JOBS, status: 200, body: JOBS_BODY },
            Exchange { request: JOBS, status: 200, body: "{}" },
            Exchange { request: SPACE, status: 403, body: "" },
            Exchange { request: SPACE, status: 200, body: "{}" },
        ], |_, wire| {
            assert_header(wire, "x-api-key: fake-immich-key", "fake-immich-key");
            assert!(!wire.lines().next().unwrap().contains("fake-immich-key"));
            ""
        }, async |conn| {
            conn.run(test(conn)).await.unwrap();
            let fetch = async |id: &str| {
                let widget = IMMICH.widgets.iter().find(|w| w.id == id).unwrap();
                super::super::fetch(conn, widget, &Values::new()).await.map(|body| serde_json::to_value(body).unwrap())
            };
            assert_eq!(fetch("overview").await.unwrap()["tiles"], json!([
                {"label":"Photos","value":"1,200","glyph":"image"},
                {"label":"Videos","value":"3","glyph":"film"},
                {"label":"Storage","value":"256 MiB","glyph":"disk","delta":"of 1.0 GiB"},
                {"label":"Jobs","value":"2 active","glyph":"activity","delta":"7 failed","tone":"warn"}
            ]));
            let partial = fetch("overview").await.unwrap();
            assert_eq!(partial["tiles"][0]["value"], "Unavailable");
            assert_eq!(partial["tiles"][3], json!({"label":"Jobs","value":"Unavailable","glyph":"activity"}));
            assert_eq!(fetch("storage").await.unwrap()["rows"], json!([
                {"label":"Server disk","percent":25,"detail":"256 MiB of 1.0 GiB"},
                {"label":"alex","percent":75,"detail":"3.0 MiB of 4.0 MiB quota"},
                {"label":"sam","percent":25,"detail":"1.0 MiB · 25% of all media"}
            ]));
            assert_eq!(fetch("jobs").await.unwrap()["rows"], json!([
                {"title":"Smart search","value":"7 failed","status":"warn"},
                {"title":"Thumbnail generation","value":"2 active · 3 waiting","status":"up"},
                {"title":"Ocr","value":"Idle"}
            ]));
            assert_eq!(fetch("jobs").await.unwrap_err().class.message(), Class::Protocol.message());
            assert_test_fault(conn, &IMMICH, Class::Auth).await;
            assert_test_fault(conn, &IMMICH, Class::Protocol).await;
        }).await;
    }
}
