use super::*;
use crate::util::human_bytes;
use widgets::{BodyRow, Glyph, Health, MeterRow, StatTile, StructuredBody};

pub const GLANCES: Provider = Provider {
    id: "glances",
    name: "Glances",
    category: "System",
    icon: "glances",
    color: "#4cb860",
    description:
        "Show CPU, memory, swap and disk meters, trends and load, top processes and sensors.",
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
    widgets: &[
        Widget {
            id: "resources",
            name: "Host resources",
            description: "CPU, memory, swap and the three fullest filesystems as meters.",
            recommended: true,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"meters","rows":[{"label":"CPU","percent":18,"detail":"18.4%"},{"label":"Memory","percent":42,"detail":"6.7 GiB of 16.0 GiB"},{"label":"Swap","percent":3,"detail":"61.4 MiB of 2.0 GiB"},{"label":"/","percent":54,"detail":"507 GiB of 935 GiB"},{"label":"/srv/media","percent":81,"detail":"3.2 TiB of 4.0 TiB"}]}"#,
            fields: &[REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "activity",
            name: "CPU, memory and load",
            description: "CPU and memory use with recent trend lines, and the load averages.",
            recommended: false,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"CPU","value":"18%","glyph":"cpu","series":[12,15,22,18,31,24,19,17,21,18]},{"label":"Memory","value":"42%","glyph":"memory","series":[40,41,41,42,42,43,42,42,42,42]},{"label":"Load","value":"0.84 · 0.92 · 1.03","glyph":"activity","delta":"8 cores"}]}"#,
            fields: &[REFRESH],
            fetch: activity,
            validate: no_widget_options,
        },
        Widget {
            id: "processes",
            name: "Top processes",
            description: "The ten busiest processes with their CPU and memory use.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"example-db","secondary":"PID 1204 · postgres","value":"CPU 12.1% · MEM 4.2%"},{"title":"media-server","secondary":"PID 881 · media","value":"CPU 7.4% · MEM 9.8%"},{"title":"backup-agent","secondary":"PID 2210 · root","value":"CPU 2.0% · MEM 0.6%"}]}"#,
            fields: &[REFRESH],
            fetch: processes,
            validate: no_widget_options,
        },
        Widget {
            id: "sensors",
            name: "Sensors",
            description: "Up to ten temperature, fan and battery readings, with warning levels.",
            recommended: false,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"rows","rows":[{"title":"Package id 0","value":"54.0 °C","status":"up"},{"title":"Composite","value":"71.0 °C","status":"warn"},{"title":"fan1","value":"1180 RPM"}]}"#,
            fields: &[REFRESH],
            fetch: sensors,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
struct Cpu {
    total: f64,
}

#[derive(Deserialize)]
struct Memory {
    percent: f64,
    total: u64,
    used: Option<u64>,
}

#[derive(Deserialize)]
struct Disk {
    mnt_point: String,
    percent: f64,
    size: Option<u64>,
    used: Option<u64>,
}

#[derive(Deserialize)]
struct Load {
    min1: f64,
    min5: f64,
    min15: f64,
    cpucore: Option<u64>,
}

#[derive(Deserialize)]
struct Process {
    name: Option<String>,
    pid: Option<u64>,
    username: Option<String>,
    cpu_percent: Option<f64>,
    memory_percent: Option<f64>,
}

#[derive(Deserialize)]
struct Sensor {
    label: String,
    value: Value,
    unit: String,
    warning: Option<f64>,
    critical: Option<f64>,
}

/// List plugins read only their first items on v4 (`/top/<n>`).
fn top(plugin: &str) -> Option<&'static str> {
    match plugin {
        "fs" | "sensors" => Some("/top/10"),
        "processlist" => Some("/top/10"),
        _ => None,
    }
}

async fn get<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    version: u8,
    plugin: &str,
) -> Result<Option<T>, Fault> {
    let optional = top(plugin).is_some();
    let suffix = match top(plugin) {
        Some(suffix) if version == 4 => suffix,
        _ => "",
    };
    request(
        conn,
        version,
        &format!("{plugin}{suffix}"),
        plugin,
        optional,
    )
    .await
}

async fn request<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    version: u8,
    resource: &str,
    plugin: &str,
    optional: bool,
) -> Result<Option<T>, Fault> {
    let path = format!("api/{version}/{resource}");
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
            error
                .detail
                .starts_with(&format!("Unknown plugin {plugin} (available plugins:"))
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

fn share(percent: f64) -> u8 {
    percent.clamp(0.0, 100.0).round() as u8
}

fn meter(label: String, percent: f64, used: Option<u64>, total: Option<u64>) -> MeterRow {
    let detail = match used.zip(total) {
        Some((used, total)) if total > 0 => {
            format!("{} of {}", human_bytes(used), human_bytes(total))
        }
        _ => format!("{percent:.1}%"),
    };
    MeterRow {
        label,
        percent: share(percent),
        detail: Some(detail),
    }
}

const FILESYSTEMS: usize = 3;

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let (version, cpu) = cpu(conn).await?;
        let memory: Option<Memory> = get(conn, version, "mem").await?;
        let swap: Option<Memory> = get(conn, version, "memswap").await?;
        let disks = within_response_limit(get::<Vec<Disk>>(conn, version, "fs").await)?.flatten();
        let mut rows = vec![MeterRow {
            label: "CPU".into(),
            percent: share(cpu.total),
            detail: Some(format!("{:.1}%", cpu.total)),
        }];
        if let Some(mem) = memory {
            rows.push(meter(
                "Memory".into(),
                mem.percent,
                mem.used,
                Some(mem.total),
            ));
        }
        if let Some(swap) = swap.filter(|swap| swap.total > 0) {
            rows.push(meter(
                "Swap".into(),
                swap.percent,
                swap.used,
                Some(swap.total),
            ));
        }
        let mut disks = disks.unwrap_or_default();
        disks.sort_by(|a, b| b.percent.total_cmp(&a.percent));
        rows.extend(
            disks
                .into_iter()
                .filter(|disk| disk.percent.is_finite())
                .take(FILESYSTEMS)
                .map(|disk| {
                    meter(
                        display_text(conn, disk.mnt_point),
                        disk.percent,
                        disk.used,
                        disk.size,
                    )
                }),
        );
        Ok(StructuredBody::Meters { rows })
    })
}

/// Points in a trend line.
const HISTORY: usize = 24;

/// A plugin field's recent values, oldest first (`/<plugin>/<field>/history/<n>`
/// answers `{"<field>": [[time, value], …]}`). A server that keeps no history
/// or refuses the request only loses the trend line; an authentication
/// failure still fails the widget.
async fn history(
    conn: &Conn<'_>,
    version: u8,
    plugin: &str,
    field: &str,
) -> Result<Vec<f32>, Fault> {
    let resource = format!("{plugin}/{field}/history/{HISTORY}");
    let answer = request::<Map<String, Value>>(conn, version, &resource, plugin, false).await;
    let mut points = match answer {
        Ok(Some(mut fields)) => fields.remove(field).unwrap_or_default(),
        Ok(None) => return Ok(Vec::new()),
        Err(fault) if matches!(fault.class, Class::Auth) => return Err(fault),
        Err(_) => return Ok(Vec::new()),
    };
    let points = points
        .as_array_mut()
        .map(std::mem::take)
        .unwrap_or_default();
    Ok(points
        .iter()
        .filter_map(|point| point.get(1)?.as_f64())
        .filter(|value| value.is_finite())
        .map(|value| value as f32)
        .collect())
}

fn tile(label: &str, value: String, glyph: Glyph, series: Vec<f32>) -> StatTile {
    StatTile {
        label: label.into(),
        value,
        glyph: Some(glyph),
        delta: None,
        tone: None,
        series,
    }
}

fn activity<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let (version, cpu) = cpu(conn).await?;
        let memory: Option<Memory> = get(conn, version, "mem").await?;
        let load: Option<Load> = get(conn, version, "load").await?;
        let mut tiles = vec![tile(
            "CPU",
            format!("{}%", share(cpu.total)),
            Glyph::Cpu,
            history(conn, version, "cpu", "total").await?,
        )];
        if let Some(mem) = memory {
            tiles.push(tile(
                "Memory",
                format!("{}%", share(mem.percent)),
                Glyph::Memory,
                history(conn, version, "mem", "percent").await?,
            ));
        }
        if let Some(load) = load {
            let mut load_tile = tile(
                "Load",
                format!("{:.2} · {:.2} · {:.2}", load.min1, load.min5, load.min15),
                Glyph::Activity,
                Vec::new(),
            );
            load_tile.delta = load.cpucore.map(|cores| format!("{cores} cores"));
            load_tile.tone = load
                .cpucore
                .filter(|cores| *cores > 0 && load.min5 > *cores as f64)
                .map(|_| Health::Warn);
            tiles.push(load_tile);
        }
        Ok(StructuredBody::Tiles { tiles })
    })
}

const ROWS: usize = 10;

fn processes<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let (version, _) = cpu(conn).await?;
        let Some(list) =
            within_response_limit(get::<Vec<Process>>(conn, version, "processlist").await)?
        else {
            return Ok(StructuredBody::Unavailable {
                message: "Process list too large to show.".into(),
            });
        };
        let mut list = list.unwrap_or_default();
        list.sort_by(|a, b| {
            b.cpu_percent
                .unwrap_or(0.0)
                .total_cmp(&a.cpu_percent.unwrap_or(0.0))
        });
        if list.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No processes reported.".into(),
            });
        }
        let rows = list
            .into_iter()
            .take(ROWS)
            .map(|process| {
                let mut secondary = process
                    .pid
                    .map_or_else(String::new, |pid| format!("PID {pid}"));
                if let Some(user) = process.username.filter(|user| !user.is_empty()) {
                    if !secondary.is_empty() {
                        secondary.push_str(" · ");
                    }
                    secondary.push_str(&display_text(conn, user));
                }
                BodyRow {
                    title: display_text(
                        conn,
                        process.name.unwrap_or_else(|| "Unnamed process".into()),
                    ),
                    secondary: Some(secondary).filter(|text| !text.is_empty()),
                    value: Some(format!(
                        "CPU {:.1}% · MEM {:.1}%",
                        process.cpu_percent.unwrap_or(0.0),
                        process.memory_percent.unwrap_or(0.0)
                    )),
                    ..BodyRow::default()
                }
            })
            .collect();
        Ok(StructuredBody::Rows { rows })
    })
}

fn unit(unit: &str) -> &str {
    match unit {
        "C" => "°C",
        "F" => "°F",
        "R" => "RPM",
        other => other,
    }
}

fn sensors<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let (version, _) = cpu(conn).await?;
        let list = within_response_limit(get::<Vec<Sensor>>(conn, version, "sensors").await)?
            .flatten()
            .unwrap_or_default();
        let rows: Vec<BodyRow> = list
            .into_iter()
            .filter_map(|sensor| {
                let value = sensor.value.as_f64().filter(|value| value.is_finite())?;
                // Glances reports 0 or null for a threshold it does not know.
                let critical = sensor.critical.filter(|limit| *limit > 0.0);
                let warning = sensor.warning.filter(|limit| *limit > 0.0);
                let status = match (warning, critical) {
                    (_, Some(critical)) if value >= critical => Some(Health::Down),
                    (Some(warning), _) if value >= warning => Some(Health::Warn),
                    (None, None) => None,
                    _ => Some(Health::Up),
                };
                Some(BodyRow {
                    title: display_text(conn, sensor.label),
                    value: Some(format!(
                        "{value:.1} {}",
                        display_text(conn, unit(&sensor.unit).to_owned())
                    )),
                    status,
                    ..BodyRow::default()
                })
            })
            .take(ROWS)
            .collect();
        if rows.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No sensors reported.".into(),
            });
        }
        Ok(StructuredBody::Rows { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    const CPU: &str = "GET /prefix/api/4/cpu HTTP/1.1";
    const CPU_BODY: &str = r#"{"total":4.5}"#;
    const ENCODED: &str = "ZmFrZS11c2VyOmZha2UtcGFzcw==";

    fn basic(_: usize, wire: &str) -> &'static str {
        assert_header(wire, &format!("authorization: Basic {ENCODED}"), ENCODED);
        assert!(!wire.contains("fake-user") && !wire.contains("fake-pass"));
        ""
    }

    async fn fetch(conn: &Conn<'_>, widget: usize) -> Result<Value, Fault> {
        super::super::fetch(conn, &GLANCES.widgets[widget], &Values::new())
            .await
            .map(|body| serde_json::to_value(body).unwrap())
    }

    const SECRETS: &[(&str, &str)] = &[("username", "fake-user"), ("password", "fake-pass")];

    #[tokio::test]
    async fn reads_v4_resources_with_optional_basic_credentials() {
        exercise(
            &GLANCES,
            SECRETS,
            Values::new(),
            &[
                Exchange { request: CPU, status: 200, body: CPU_BODY },
                Exchange { request: CPU, status: 200, body: CPU_BODY },
                Exchange {
                    request: "GET /prefix/api/4/mem HTTP/1.1",
                    status: 200,
                    body: r#"{"percent":50,"total":1073741824,"used":536870912}"#,
                },
                Exchange {
                    request: "GET /prefix/api/4/memswap HTTP/1.1",
                    status: 200,
                    body: r#"{"free":0,"percent":0,"total":0,"used":0}"#,
                },
                Exchange {
                    request: "GET /prefix/api/4/fs/top/10 HTTP/1.1",
                    status: 200,
                    body: r#"[{"device_name":"/dev/sda1","fs_type":"ext4","key":"mnt_point","mnt_point":"/","percent":20.8,"size":1003736440832,"used":198017622016},{"mnt_point":"/data","percent":44.5}]"#,
                },
                Exchange { request: CPU, status: 401, body: "" },
                Exchange { request: CPU, status: 200, body: "{}" },
            ],
            basic,
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let body = fetch(conn, 0).await.unwrap();
                assert_eq!(body["kind"], "meters");
                assert_eq!(
                    body["rows"],
                    json!([
                        {"label": "CPU", "percent": 5, "detail": "4.5%"},
                        {"label": "Memory", "percent": 50, "detail": "512 MiB of 1.0 GiB"},
                        {"label": "/data", "percent": 45, "detail": "44.5%"},
                        {"label": "/", "percent": 21, "detail": "184 GiB of 935 GiB"},
                    ])
                );
                assert_test_fault(conn, &GLANCES, Class::Auth).await;
                assert_test_fault(conn, &GLANCES, Class::Protocol).await;
            },
        )
        .await;
    }

    #[tokio::test]
    async fn shows_trends_load_processes_and_sensors() {
        exercise(
            &GLANCES,
            SECRETS,
            Values::new(),
            &[
                Exchange { request: CPU, status: 200, body: CPU_BODY },
                Exchange {
                    request: "GET /prefix/api/4/mem HTTP/1.1",
                    status: 200,
                    body: r#"{"percent":42.4,"total":1073741824}"#,
                },
                Exchange {
                    request: "GET /prefix/api/4/load HTTP/1.1",
                    status: 200,
                    body: r#"{"cpucore": 2, "min1": 1.51904296875, "min15": 1.1748046875, "min5": 2.5}"#,
                },
                Exchange {
                    request: "GET /prefix/api/4/cpu/total/history/24 HTTP/1.1",
                    status: 200,
                    body: r#"{"total": [["2025-07-09T15:33:27.996914+00:00", 0.5], ["2025-07-09T15:33:29.021528+00:00", 2.5]]}"#,
                },
                // A server without history loses only the trend line.
                Exchange {
                    request: "GET /prefix/api/4/mem/percent/history/24 HTTP/1.1",
                    status: 500,
                    body: "",
                },
                Exchange { request: CPU, status: 200, body: CPU_BODY },
                Exchange {
                    request: "GET /prefix/api/4/processlist/top/10 HTTP/1.1",
                    status: 200,
                    body: r#"[{"cmdline":["/usr/bin/agent","--flag"],"cpu_percent":2.0,"memory_percent":0.6,"name":"backup-agent","pid":2210,"username":"root","status":"S"},{"cpu_percent":12.1,"memory_percent":4.2,"name":"example-db","pid":1204,"username":"postgres"}]"#,
                },
                Exchange { request: CPU, status: 200, body: CPU_BODY },
                Exchange {
                    request: "GET /prefix/api/4/sensors/top/10 HTTP/1.1",
                    status: 200,
                    body: r#"[{"critical":null,"key":"label","label":"Ambient","type":"temperature_core","unit":"C","value":40,"warning":0},{"critical":100,"key":"label","label":"Package id 0","type":"temperature_core","unit":"C","value":84,"warning":80},{"critical":null,"label":"fan1","type":"fan_speed","unit":"R","value":1180,"warning":null},{"label":"broken","unit":"C","value":"n/a"}]"#,
                },
                Exchange { request: CPU, status: 200, body: CPU_BODY },
                Exchange {
                    request: "GET /prefix/api/4/processlist/top/10 HTTP/1.1",
                    status: 200,
                    body: r#"{"not":"a list"}"#,
                },
                Exchange { request: CPU, status: 200, body: CPU_BODY },
                Exchange {
                    request: "GET /prefix/api/4/mem HTTP/1.1",
                    status: 200,
                    body: r#"{"percent":42.4,"total":1073741824}"#,
                },
                Exchange {
                    request: "GET /prefix/api/4/load HTTP/1.1",
                    status: 200,
                    body: r#"{"cpucore": 2, "min1": 1.0, "min15": 1.0, "min5": 1.0}"#,
                },
                Exchange {
                    request: "GET /prefix/api/4/cpu/total/history/24 HTTP/1.1",
                    status: 401,
                    body: "",
                },
            ],
            basic,
            async |conn| {
                let tiles = fetch(conn, 1).await.unwrap();
                assert_eq!(tiles["kind"], "tiles");
                assert_eq!(
                    tiles["tiles"],
                    json!([
                        {"label": "CPU", "value": "5%", "glyph": "cpu", "series": [0.5, 2.5]},
                        {"label": "Memory", "value": "42%", "glyph": "memory"},
                        {"label": "Load", "value": "1.52 · 2.50 · 1.17", "glyph": "activity", "delta": "2 cores", "tone": "warn"},
                    ])
                );
                let processes = fetch(conn, 2).await.unwrap();
                assert_eq!(
                    processes["rows"],
                    json!([
                        {"title": "example-db", "secondary": "PID 1204 · postgres", "value": "CPU 12.1% · MEM 4.2%"},
                        {"title": "backup-agent", "secondary": "PID 2210 · root", "value": "CPU 2.0% · MEM 0.6%"},
                    ])
                );
                let sensors = fetch(conn, 3).await.unwrap();
                assert_eq!(
                    sensors["rows"],
                    json!([
                        {"title": "Ambient", "value": "40.0 °C"},
                        {"title": "Package id 0", "value": "84.0 °C", "status": "warn"},
                        {"title": "fan1", "value": "1180.0 RPM"},
                    ])
                );
                assert!(matches!(fetch(conn, 2).await.unwrap_err().class, Class::Protocol));
                assert!(matches!(fetch(conn, 1).await.unwrap_err().class, Class::Auth));
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
