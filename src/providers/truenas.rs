use super::*;
use crate::util::human_bytes;
use widgets::StructuredBody;

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
    description: "Show pool status, active alerts and recent disk self-test results (REST v2.0).",
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
    widgets: &[Widget {
        id: "storage",
        name: "Storage health",
        description:
            "Pool status and free space, active alerts and recent SMART self-test results.",
        recommended: true,
        fields: &[Field {
            min: Some(60),
            default: Some("300"),
            ..REFRESH
        }],
        fetch: overview,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
struct Pool {
    name: String,
    status: String,
    free: Option<u64>,
}

#[derive(Deserialize)]
struct Alert {
    dismissed: bool,
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

fn pool_rows(conn: &Conn<'_>, pools: Option<Vec<Pool>>) -> Vec<(String, String)> {
    let Some(pools) = pools else {
        return vec![("Pools".into(), "List too large".into())];
    };
    let mut rows = vec![(
        "Pools".into(),
        match pools.len() {
            n if n > DISPLAY_LIMIT => "Showing first 10".into(),
            n => n.to_string(),
        },
    )];
    rows.extend(pools.into_iter().take(DISPLAY_LIMIT).map(|pool| {
        let status = display_text(conn, pool.status);
        let value = match pool.free {
            Some(bytes) => format!("{status} · {} free", human_bytes(bytes)),
            None => status,
        };
        (display_text(conn, pool.name), value)
    }));
    rows
}

async fn disk_rows(conn: &Conn<'_>) -> Result<Vec<(String, String)>, Fault> {
    let disks = within_response_limit(
        get::<Disk>(
            conn,
            DISKS,
            &[("limit", Input::Text("11")), ("sort", Input::Text("name"))],
        )
        .await,
    )?;
    let Some(disks) = disks else {
        return Ok(vec![("Disks".into(), "List too large".into())]);
    };
    let mut rows = vec![(
        "Disks".into(),
        match disks.len() {
            n if n > DISPLAY_LIMIT => "Showing first 10".into(),
            n => n.to_string(),
        },
    )];
    if disks.is_empty() {
        return Ok(rows);
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
    rows.extend(disks.into_iter().take(DISPLAY_LIMIT).map(|disk| {
        let status = match &tests {
            None => Err("SMART results too large"),
            // A release without the SMART test route keeps the disk rows.
            Some(None) => Err("SMART results unavailable"),
            Some(Some(tests)) => Ok(tests),
        };
        let status = status.map_or_else(
            |message| message,
            |tests| match tests
                .iter()
                .find(|test| test.disk == disk.name)
                .and_then(|result| result.tests.first())
                .map(|test| test.status.as_str())
            {
                Some("SUCCESS") => "Last SMART test passed",
                Some("FAILED") => "Last SMART test failed",
                Some("RUNNING") => "SMART test running",
                Some("ABORTED") => "Last SMART test aborted",
                _ => "No reported SMART result",
            },
        );
        (display_text(conn, disk.name), status.into())
    }));
    Ok(rows)
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let alerts = within_response_limit(get::<Alert>(conn, ALERTS, &[]).await)?;
        let mut rows = vec![(
            "Active alerts".into(),
            alerts.map_or_else(
                || "List too large".into(),
                |alerts| {
                    alerts
                        .iter()
                        .filter(|alert| !alert.dismissed)
                        .count()
                        .to_string()
                },
            ),
        )];
        let pools = within_response_limit(
            get::<Pool>(
                conn,
                POOLS,
                &[("limit", Input::Text("11")), ("sort", Input::Text("name"))],
            )
            .await,
        )?;
        rows.extend(pool_rows(conn, pools));
        rows.extend(disk_rows(conn).await?);
        Ok(StructuredBody::StatGrid { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn reads_legacy_storage_and_preserves_other_metrics_when_alerts_are_oversized() {
        const CHECK: &str = "GET /prefix/api/v2.0/disk?limit=1 HTTP/1.1";
        const ALERT_LIST: &str = "GET /prefix/api/v2.0/alert/list HTTP/1.1";
        const POOL_LIST: &str = "GET /prefix/api/v2.0/pool?limit=11&sort=name HTTP/1.1";
        const DISK_LIST: &str = "GET /prefix/api/v2.0/disk?limit=11&sort=name HTTP/1.1";
        const TESTS: &str = "GET /prefix/api/v2.0/smart/test/results?limit=10&sort=disk HTTP/1.1";
        const POOL: &str = r#"[{"name":"tank","status":"ONLINE","free":1048576}]"#;
        const DISK: &str = r#"[{"name":"sda"}]"#;
        let oversized = "x".repeat(MAX_RESPONSE + 1);
        let exchanges = [
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
            (POOL_LIST, 200, POOL),
            (DISK_LIST, 200, "[]"),
            (ALERT_LIST, 200, "[]"),
            (POOL_LIST, 200, POOL),
            (DISK_LIST, 200, DISK),
            (TESTS, 404, r#"{"message":"Not Found"}"#),
            (CHECK, 403, ""),
            (CHECK, 200, "{}"),
        ]
        .map(|(request, status, body)| Exchange {
            request,
            status,
            body,
        });
        exercise(
            &TRUENAS,
            &[(KEY, "fake-nas-key")],
            Values::new(),
            &exchanges,
            |_, wire| {
                assert_header(wire, "authorization: Bearer fake-nas-key", "fake-nas-key");
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let fetch = async || {
                    serde_json::to_value(
                        super::super::fetch(conn, &TRUENAS.widgets[0], &Values::new())
                            .await
                            .unwrap(),
                    )
                    .unwrap()
                };
                assert_eq!(
                    fetch().await["rows"],
                    json!([
                        ["Active alerts", "1"],
                        ["Pools", "1"],
                        ["tank", "ONLINE · 1.0 MiB free"],
                        ["Disks", "1"],
                        ["sda", "Last SMART test failed"]
                    ])
                );
                let degraded = fetch().await;
                assert_eq!(
                    degraded["rows"][0],
                    json!(["Active alerts", "List too large"])
                );
                assert_eq!(
                    degraded["rows"][2],
                    json!(["tank", "ONLINE · 1.0 MiB free"])
                );
                assert_eq!(
                    fetch().await["rows"],
                    json!([
                        ["Active alerts", "0"],
                        ["Pools", "1"],
                        ["tank", "ONLINE · 1.0 MiB free"],
                        ["Disks", "1"],
                        ["sda", "SMART results unavailable"]
                    ])
                );
                assert_test_fault(conn, &TRUENAS, Class::Auth).await;
                assert_test_fault(conn, &TRUENAS, Class::Protocol).await;
            },
        )
        .await;
    }
}
