use super::*;
use crate::util::human_bytes;
use widgets::{BodyRow, Health, MeterRow, StructuredBody};

const KEY: &str = "api_key";
const DISKS: &str = "api/v2.0/disk";
const POOLS: &str = "api/v2.0/pool";
const ALERTS: &str = "api/v2.0/alert/list";
const SMART: &str = "api/v2.0/smart/test/results";
const DISPLAY_LIMIT: usize = 10;

pub const TRUENAS: Provider = Provider {
    id: "truenas-scale",
    name: "TrueNAS SCALE",
    category: "Storage",
    icon: "truenas",
    color: "#0095d5",
    detect: None,
    description:
        "Show pool status and use, active alerts and recent disk self-test results (REST v2.0).",
    docs: "https://www.truenas.com/docs/scale/24.10/api/",
    public_internet: false,
    contacts: "Your configured TrueNAS SCALE server with REST v2.0.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(4096),
            ..Field::new(
                KEY,
                "API key",
                Kind::Secret,
                "Use a key with access to pools, alerts, disks and SMART results.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "storage",
            name: "Storage health",
            description:
                "Active alerts, pool status and use, and the latest SMART self-test of each disk.",
            recommended: true,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"Active alerts","value":"1","status":"warn"},{"title":"tank","secondary":"ONLINE · 1.5 TiB free","value":"62%","status":"up","progress":62},{"title":"backup","secondary":"DEGRADED · 2.8 TiB free","value":"31%","status":"warn","progress":31},{"title":"sda","secondary":"Last SMART test passed","status":"up"},{"title":"sdb","secondary":"Last SMART test failed","status":"down"}]}"#,
            fields: &[SLOW_REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "pools",
            name: "Pool usage",
            description: "Used space of up to ten pools as meters.",
            recommended: false,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"meters","rows":[{"label":"tank","percent":62,"detail":"2.5 TiB of 4.0 TiB"},{"label":"backup","percent":31,"detail":"1.2 TiB of 4.0 TiB"}]}"#,
            fields: &[SLOW_REFRESH],
            fetch: pools,
            validate: no_widget_options,
        },
        Widget {
            id: "alerts",
            name: "Alerts",
            description: "Up to ten active alerts, most severe first.",
            recommended: false,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"rows","rows":[{"title":"Pool backup state is DEGRADED: One or more devices has been removed.","secondary":"Critical","status":"down"},{"title":"A system update is available.","secondary":"Info"}]}"#,
            fields: &[SLOW_REFRESH],
            fetch: alerts,
            validate: no_widget_options,
        },
    ],
};

const SLOW_REFRESH: Field = Field {
    min: Some(60),
    default: Some("300"),
    ..REFRESH
};

#[derive(Deserialize)]
struct Pool {
    name: String,
    status: String,
    free: Option<u64>,
    size: Option<u64>,
    allocated: Option<u64>,
}

impl Pool {
    fn used(&self) -> Option<(u64, u64)> {
        self.allocated
            .zip(self.size)
            .filter(|(_, size)| *size > 0)
            .map(|(used, size)| (used.min(size), size))
    }
}

#[derive(Deserialize)]
struct Alert {
    dismissed: bool,
    #[serde(default)]
    level: String,
    formatted: Option<String>,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct Disk {
    name: String,
}

#[derive(Deserialize)]
struct SmartResults {
    disk: String,
    tests: Vec<SmartTest>,
}

#[derive(Deserialize)]
struct SmartTest {
    status: String,
}

async fn request(
    conn: &Conn<'_>,
    path: &str,
    query: &[(&str, Input<'_>)],
) -> Result<client::Response, Fault> {
    conn.request(Step {
        auth: &[Auth::Bearer(Input::Credential(KEY))],
        query,
        ..Step::get(path)
    })
    .await
}

async fn get<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    path: &str,
    query: &[(&str, Input<'_>)],
) -> Result<Vec<T>, Fault> {
    parse_json(request(conn, path, query).await?)
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        get::<Disk>(conn, DISKS, &[("limit", Input::Text("1"))])
            .await
            .map(|_| ())
    })
}

fn row(title: String, secondary: Option<String>, status: Option<Health>) -> BodyRow {
    BodyRow {
        title,
        secondary,
        status,
        ..BodyRow::default()
    }
}

fn pool_health(status: &str) -> Option<Health> {
    match status {
        "ONLINE" => Some(Health::Up),
        "DEGRADED" => Some(Health::Warn),
        "FAULTED" | "OFFLINE" | "UNAVAIL" | "REMOVED" => Some(Health::Down),
        _ => None,
    }
}

async fn list_pools(conn: &Conn<'_>) -> Result<Option<Vec<Pool>>, Fault> {
    within_response_limit(
        get::<Pool>(
            conn,
            POOLS,
            &[("limit", Input::Text("11")), ("sort", Input::Text("name"))],
        )
        .await,
    )
}

fn pool_rows(conn: &Conn<'_>, pools: Option<Vec<Pool>>) -> Vec<BodyRow> {
    let Some(pools) = pools else {
        return vec![row("Pools".into(), Some("List too large".into()), None)];
    };
    pools
        .into_iter()
        .take(DISPLAY_LIMIT)
        .map(|pool| {
            let used = pool.used();
            let status = display_text(conn, pool.status.clone());
            let secondary = match pool.free {
                Some(bytes) => format!("{status} · {} free", human_bytes(bytes)),
                None => status,
            };
            let percent = used.map(|(used, size)| percent(used, size));
            BodyRow {
                title: display_text(conn, pool.name),
                secondary: Some(secondary),
                value: percent.map(|percent| format!("{percent}%")),
                status: pool_health(&pool.status),
                progress: percent,
                bars: Vec::new(),
            }
        })
        .collect()
}

fn percent(used: u64, total: u64) -> u8 {
    (used as f64 * 100.0 / total as f64).round() as u8
}

async fn disk_rows(conn: &Conn<'_>) -> Result<Vec<BodyRow>, Fault> {
    let disks = within_response_limit(
        get::<Disk>(
            conn,
            DISKS,
            &[("limit", Input::Text("11")), ("sort", Input::Text("name"))],
        )
        .await,
    )?;
    let Some(disks) = disks else {
        return Ok(vec![row(
            "Disks".into(),
            Some("List too large".into()),
            None,
        )]);
    };
    if disks.is_empty() {
        return Ok(Vec::new());
    }
    let tests = within_response_limit(
        request(
            conn,
            SMART,
            &[("limit", Input::Text("10")), ("sort", Input::Text("disk"))],
        )
        .await
        .and_then(parse_optional::<Vec<SmartResults>>),
    )?;
    Ok(disks
        .into_iter()
        .take(DISPLAY_LIMIT)
        .map(|disk| {
            let (message, status) = match &tests {
                None => ("SMART results too large", None),
                // A release without the SMART test route keeps the disk rows.
                Some(None) => ("SMART results unavailable", None),
                Some(Some(tests)) => match tests
                    .iter()
                    .find(|test| test.disk == disk.name)
                    .and_then(|result| result.tests.first())
                    .map(|test| test.status.as_str())
                {
                    Some("SUCCESS") => ("Last SMART test passed", Some(Health::Up)),
                    Some("FAILED") => ("Last SMART test failed", Some(Health::Down)),
                    Some("RUNNING") => ("SMART test running", None),
                    Some("ABORTED") => ("Last SMART test aborted", Some(Health::Warn)),
                    _ => ("No reported SMART result", None),
                },
            };
            row(display_text(conn, disk.name), Some(message.into()), status)
        })
        .collect())
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let alerts = within_response_limit(get::<Alert>(conn, ALERTS, &[]).await)?;
        let active = alerts.map(|alerts| alerts.iter().filter(|alert| !alert.dismissed).count());
        let mut rows = vec![BodyRow {
            title: "Active alerts".into(),
            value: Some(active.map_or_else(|| "List too large".into(), |n| n.to_string())),
            status: active.map(|n| if n == 0 { Health::Up } else { Health::Warn }),
            ..BodyRow::default()
        }];
        rows.extend(pool_rows(conn, list_pools(conn).await?));
        rows.extend(disk_rows(conn).await?);
        Ok(StructuredBody::Rows { rows })
    })
}

fn pools<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(pools) = list_pools(conn).await? else {
            return Ok(StructuredBody::Unavailable {
                message: "Pool list too large to show.".into(),
            });
        };
        let rows: Vec<MeterRow> = pools
            .into_iter()
            .take(DISPLAY_LIMIT)
            .filter_map(|pool| {
                let (used, size) = pool.used()?;
                Some(MeterRow {
                    label: display_text(conn, pool.name),
                    percent: percent(used, size),
                    detail: Some(format!("{} of {}", human_bytes(used), human_bytes(size))),
                })
            })
            .collect();
        if rows.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No pools with a known size.".into(),
            });
        }
        Ok(StructuredBody::Meters { rows })
    })
}

/// Severity order of `AlertLevel`, most severe first.
fn severity(level: &str) -> (u8, &'static str, Option<Health>) {
    match level {
        "EMERGENCY" => (0, "Emergency", Some(Health::Down)),
        "ALERT" => (1, "Alert", Some(Health::Down)),
        "CRITICAL" => (2, "Critical", Some(Health::Down)),
        "ERROR" => (3, "Error", Some(Health::Down)),
        "WARNING" => (4, "Warning", Some(Health::Warn)),
        "NOTICE" => (5, "Notice", None),
        "INFO" => (6, "Info", None),
        _ => (7, "Alert", None),
    }
}

fn alerts<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(mut alerts) = within_response_limit(get::<Alert>(conn, ALERTS, &[]).await)? else {
            return Ok(StructuredBody::Unavailable {
                message: "Alert list too large to show.".into(),
            });
        };
        alerts.retain(|alert| !alert.dismissed);
        if alerts.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No active alerts.".into(),
            });
        }
        alerts.sort_by_key(|alert| severity(&alert.level).0);
        let rows = alerts
            .into_iter()
            .take(DISPLAY_LIMIT)
            .map(|alert| {
                let (_, label, status) = severity(&alert.level);
                let text = alert
                    .formatted
                    .filter(|text| !text.is_empty())
                    .unwrap_or(alert.text);
                row(display_text(conn, text), Some(label.into()), status)
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

    const CHECK: &str = "GET /prefix/api/v2.0/disk?limit=1 HTTP/1.1";
    const ALERT_LIST: &str = "GET /prefix/api/v2.0/alert/list HTTP/1.1";
    const POOL_LIST: &str = "GET /prefix/api/v2.0/pool?limit=11&sort=name HTTP/1.1";
    const DISK_LIST: &str = "GET /prefix/api/v2.0/disk?limit=11&sort=name HTTP/1.1";
    const TESTS: &str = "GET /prefix/api/v2.0/smart/test/results?limit=10&sort=disk HTTP/1.1";

    fn bearer(_: usize, wire: &str) -> &'static str {
        assert_header(wire, "authorization: Bearer fake-nas-key", "fake-nas-key");
        ""
    }

    fn exchanges<'a>(list: &[(&'static str, u16, &'a str)]) -> Vec<Exchange<'a>> {
        list.iter()
            .map(|&(request, status, body)| Exchange {
                request,
                status,
                body,
            })
            .collect()
    }

    async fn fetch(conn: &Conn<'_>, widget: usize) -> Result<Value, Fault> {
        super::super::fetch(conn, &TRUENAS.widgets[widget], &Values::new())
            .await
            .map(|body| serde_json::to_value(body).unwrap())
    }

    #[tokio::test]
    async fn reads_legacy_storage_and_preserves_other_metrics_when_alerts_are_oversized() {
        const POOL: &str = r#"[{"name":"tank","status":"ONLINE","healthy":true,"free":1048576,"size":4194304,"allocated":3145728}]"#;
        const DISK: &str = r#"[{"name":"sda"}]"#;
        let oversized = "x".repeat(MAX_RESPONSE + 1);
        let list = exchanges(&[
            (CHECK, 200, DISK),
            (
                ALERT_LIST,
                200,
                r#"[{"dismissed":false},{"dismissed":true}]"#,
            ),
            (POOL_LIST, 200, POOL),
            (DISK_LIST, 200, DISK),
            (
                TESTS,
                200,
                r#"[{"disk":"sda","tests":[{"status":"FAILED"},{"status":"SUCCESS"}]}]"#,
            ),
            (ALERT_LIST, 200, &oversized),
            (
                POOL_LIST,
                200,
                r#"[{"name":"tank","status":"DEGRADED","free":null,"size":null,"allocated":null}]"#,
            ),
            (DISK_LIST, 200, "[]"),
            (ALERT_LIST, 200, "[]"),
            (POOL_LIST, 200, POOL),
            (DISK_LIST, 200, DISK),
            (TESTS, 404, r#"{"message":"Not Found"}"#),
            (CHECK, 403, ""),
            (CHECK, 200, "{}"),
        ]);
        exercise(&TRUENAS, &[(KEY, "fake-nas-key")], Values::new(), &list, bearer, async |conn| {
            conn.run(test(conn)).await.unwrap();
            assert_eq!(
                fetch(conn, 0).await.unwrap()["rows"],
                json!([
                    {"title": "Active alerts", "value": "1", "status": "warn"},
                    {"title": "tank", "secondary": "ONLINE · 1.0 MiB free", "value": "75%", "status": "up", "progress": 75},
                    {"title": "sda", "secondary": "Last SMART test failed", "status": "down"},
                ])
            );
            assert_eq!(
                fetch(conn, 0).await.unwrap()["rows"],
                json!([
                    {"title": "Active alerts", "value": "List too large"},
                    {"title": "tank", "secondary": "DEGRADED", "status": "warn"},
                ])
            );
            let rows = fetch(conn, 0).await.unwrap()["rows"].clone();
            assert_eq!(rows[0], json!({"title": "Active alerts", "value": "0", "status": "up"}));
            assert_eq!(rows[2], json!({"title": "sda", "secondary": "SMART results unavailable"}));
            assert_test_fault(conn, &TRUENAS, Class::Auth).await;
            assert_test_fault(conn, &TRUENAS, Class::Protocol).await;
        })
        .await;
    }

    #[tokio::test]
    async fn shows_pool_meters_and_active_alerts_by_severity() {
        // pool.query and alert.list shapes from middleware TS-24.10.2.
        const POOLS: &str = r#"[
            {"id":1,"name":"backup","guid":"1","status":"ONLINE","healthy":true,"size":4398046511104,"allocated":1319413953331,"free":3078632557773},
            {"id":2,"name":"offline","status":"OFFLINE","healthy":false,"size":null,"allocated":null,"free":null},
            {"id":3,"name":"tank","status":"ONLINE","healthy":true,"size":4398046511104,"allocated":2726788837376,"free":1671257673728}]"#;
        const ALERTS: &str = r#"[
            {"uuid":"a","klass":"UpdateAvailable","level":"INFO","formatted":"A system update is available.","text":"A system update is available.","dismissed":false,"datetime":{"$date":1700000000000}},
            {"uuid":"b","klass":"PoolStatus","level":"CRITICAL","formatted":"Pool backup state is DEGRADED.","text":"Pool %(volume)s state is %(state)s.","dismissed":false},
            {"uuid":"c","klass":"Old","level":"WARNING","formatted":"Dismissed one.","dismissed":true}]"#;
        let list = exchanges(&[
            (POOL_LIST, 200, POOLS),
            (ALERT_LIST, 200, ALERTS),
            (ALERT_LIST, 200, r#"[{"dismissed":true,"level":"INFO"}]"#),
            (POOL_LIST, 200, r#"{"message":"not a list"}"#),
            (ALERT_LIST, 401, ""),
        ]);
        exercise(&TRUENAS, &[(KEY, "fake-nas-key")], Values::new(), &list, bearer, async |conn| {
            assert_eq!(
                fetch(conn, 1).await.unwrap()["rows"],
                json!([
                    {"label": "backup", "percent": 30, "detail": "1.2 TiB of 4.0 TiB"},
                    {"label": "tank", "percent": 62, "detail": "2.5 TiB of 4.0 TiB"},
                ])
            );
            assert_eq!(
                fetch(conn, 2).await.unwrap()["rows"],
                json!([
                    {"title": "Pool backup state is DEGRADED.", "secondary": "Critical", "status": "down"},
                    {"title": "A system update is available.", "secondary": "Info"},
                ])
            );
            assert_eq!(fetch(conn, 2).await.unwrap()["message"], "No active alerts.");
            assert!(matches!(fetch(conn, 1).await.unwrap_err().class, Class::Protocol));
            assert!(matches!(fetch(conn, 2).await.unwrap_err().class, Class::Auth));
        })
        .await;
    }
}
