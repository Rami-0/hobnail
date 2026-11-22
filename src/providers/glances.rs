use super::*;
use crate::util::human_bytes;
use widgets::StructuredBody;

pub const GLANCES: Provider = Provider {
    id: "glances",
    name: "Glances",
    category: "System",
    icon: "glances",
    description: "Show CPU, memory, filesystem usage and sensors from a Glances host.",
    docs: "https://glances.readthedocs.io/en/latest/api/restful.html",
    public_internet: false,
    contacts: "Your configured Glances web server.",
    fields: &[
        SERVICE_URL,
        OPTIONAL_BASIC_USER,
        OPTIONAL_BASIC_PASSWORD,
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "resources",
        name: "Host resources",
        description: "CPU, memory, filesystems and hardware sensors.",
        recommended: true,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
struct Cpu {
    total: f64,
}

#[derive(Deserialize)]
struct Memory {
    percent: f64,
    total: u64,
}

#[derive(Deserialize)]
struct Disk {
    mnt_point: String,
    percent: f64,
}

#[derive(Deserialize)]
struct Sensor {
    label: String,
    value: Value,
    unit: String,
}

async fn get<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    version: u8,
    plugin: &str,
) -> Result<Option<T>, Fault> {
    let optional = matches!(plugin, "fs" | "sensors");
    let suffix = if version == 4 && optional { "/top/5" } else { "" };
    let path = format!("api/{version}/{plugin}{suffix}");
    let basic = optional_basic(conn)?;
    let response = conn
        .request(Step {
            auth: basic.as_slice(),
            ..Step::get(&path)
        })
        .await?;
    if response.status == 404 {
        return Ok(None);
    }
    if version == 4 && optional && response.status == 400 {
        #[derive(Deserialize)]
        struct Error {
            detail: String,
        }
        if serde_json::from_slice::<Error>(&response.body).is_ok_and(|error| {
            error.detail.starts_with(&format!("Unknown plugin {plugin} (available plugins:"))
        }) {
            return Ok(None);
        }
    }
    parse_json(response).map(Some)
}

async fn cpu(conn: &Conn<'_>) -> Result<(u8, Cpu), Fault> {
    if let Some(cpu) = get(conn, 4, "cpu").await? {
        return Ok((4, cpu));
    }
    let cpu = get(conn, 3, "cpu").await?.ok_or(Class::HttpStatus)?;
    Ok((3, cpu))
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { cpu(conn).await.map(|_| ()) })
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let (version, cpu) = cpu(conn).await?;
        let memory: Option<Memory> = get(conn, version, "mem").await?;
        let disks = within_response_limit(get::<Vec<Disk>>(conn, version, "fs").await)?.flatten();
        let sensors =
            within_response_limit(get::<Vec<Sensor>>(conn, version, "sensors").await)?.flatten();
        let mut rows = vec![
            ("CPU".into(), format!("{:.1}%", cpu.total)),
            (
                "Memory".into(),
                memory.map_or_else(
                    || "Unavailable".into(),
                    |mem| format!("{:.1}% of {}", mem.percent, human_bytes(mem.total)),
                ),
            ),
        ];
        match disks.filter(|disks| !disks.is_empty()) {
            Some(disks) => rows.extend(disks.into_iter().take(5).map(|disk| {
                (
                    display_text(conn, disk.mnt_point),
                    format!("{:.1}% used", disk.percent),
                )
            })),
            None => rows.push(("Filesystems".into(), "Unavailable".into())),
        }
        let mut sensors = sensors
            .into_iter()
            .flatten()
            .filter_map(|sensor| {
                let value = sensor.value.as_f64()?;
                Some((
                    display_text(conn, sensor.label),
                    format!("{value:.1} {}", display_text(conn, sensor.unit)),
                ))
            })
            .take(5)
            .peekable();
        if sensors.peek().is_none() {
            rows.push(("Sensors".into(), "Unavailable".into()));
        }
        rows.extend(sensors);
        Ok(StructuredBody::StatGrid { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn reads_v4_resources_with_optional_basic_credentials() {
        const CPU: &str = "GET /prefix/api/4/cpu HTTP/1.1";
        const CPU_BODY: &str = r#"{"total":4.5}"#;
        exercise(
            &GLANCES,
            &[("username", "fake-user"), ("password", "fake-pass")],
            Values::new(),
            &[
                Exchange {
                    request: CPU,
                    status: 200,
                    body: CPU_BODY,
                },
                Exchange {
                    request: CPU,
                    status: 200,
                    body: CPU_BODY,
                },
                Exchange {
                    request: "GET /prefix/api/4/mem HTTP/1.1",
                    status: 200,
                    body: r#"{"percent":50,"total":1073741824}"#,
                },
                Exchange {
                    request: "GET /prefix/api/4/fs/top/5 HTTP/1.1",
                    status: 200,
                    body: r#"[{"mnt_point":"/data","percent":44.5}]"#,
                },
                Exchange {
                    request: "GET /prefix/api/4/sensors/top/5 HTTP/1.1",
                    status: 400,
                    body: r#"{"detail":"Unknown plugin sensors (available plugins: ['cpu', 'mem', 'fs'])"}"#,
                },
                Exchange {
                    request: CPU,
                    status: 401,
                    body: "",
                },
                Exchange {
                    request: CPU,
                    status: 200,
                    body: "{}",
                },
            ],
            |_, wire| {
                const ENCODED: &str = "ZmFrZS11c2VyOmZha2UtcGFzcw==";
                assert_header(wire, &format!("authorization: Basic {ENCODED}"), ENCODED);
                assert!(!wire.contains("fake-user") && !wire.contains("fake-pass"));
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let body = super::super::fetch(conn, &GLANCES.widgets[0], &Values::new())
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(body).unwrap()["rows"],
                    json!([
                        ["CPU", "4.5%"],
                        ["Memory", "50.0% of 1.0 GiB"],
                        ["/data", "44.5% used"],
                        ["Sensors", "Unavailable"],
                    ])
                );
                assert_test_fault(conn, &GLANCES, Class::Auth).await;
                assert_test_fault(conn, &GLANCES, Class::Protocol).await;
            },
        )
        .await;
    }

    #[tokio::test]
    async fn tries_v3_only_when_v4_cpu_is_missing() {
        exercise(
            &GLANCES,
            &[],
            Values::new(),
            &[
                Exchange {
                    request: "GET /prefix/api/4/cpu HTTP/1.1",
                    status: 404,
                    body: "",
                },
                Exchange {
                    request: "GET /prefix/api/3/cpu HTTP/1.1",
                    status: 200,
                    body: r#"{"total":5}"#,
                },
            ],
            |_, wire| {
                assert!(!wire.contains("authorization:"));
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
            },
        )
        .await;
    }
}
