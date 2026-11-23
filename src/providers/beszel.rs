use super::*;
use crate::util::human_duration;
use widgets::{BodyRow, Glyph, Health, MeterRow, StatTile, StructuredBody};

const IDENTITY: &str = "identity";
const SECRET: &str = "secret";
/// Systems read per refresh; the widgets show fewer.
const PAGE: &str = "50";
const MAX_ROWS: usize = 12;
const MAX_METER_SYSTEMS: usize = 4;

pub const BESZEL: Provider = Provider {
    id: "beszel",
    name: "Beszel",
    category: "System",
    icon: "beszel",
    color: "#747bff",
    // The agent image (`henrygd/beszel-agent`) has no web API.
    detect: super::detect_except(&["beszel"], &["beszel-agent"], 8090),
    description: "Show every monitored system's status with its CPU, memory and disk use, and a fleet summary.",
    docs: "https://beszel.dev/guide/what-is-beszel",
    public_internet: false,
    contacts: "Your configured Beszel hub (its PocketBase REST API).",
    fields: &[
        Field {
            help: "The Beszel hub address, e.g. http://192.168.1.10:8090.",
            ..SERVICE_URL
        },
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(
                IDENTITY,
                "Account email",
                Kind::Secret,
                "The email of a Beszel user who can see the systems to show. A read-only user (Settings → Users) is enough.",
            )
        },
        Field {
            required: true,
            max_length: Some(4096),
            ..Field::new(
                SECRET,
                "Account password",
                Kind::Secret,
                "That user's Beszel password. Hobnail exchanges it for a short-lived token.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "systems",
            name: "Systems",
            description: "Up to twelve systems with their status, CPU, memory and disk use and uptime.",
            recommended: true,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"rows","rows":[{"title":"nas","secondary":"CPU 12% · RAM 43% · Disk 61%","value":"12 d","status":"up","progress":61},{"title":"pi","secondary":"CPU 4% · RAM 38% · Disk 22%","value":"3 d","status":"up","progress":38},{"title":"backup","secondary":"Last seen offline","value":"Down","status":"down"}]}"#,
            fields: &[REFRESH],
            fetch: systems,
            validate: no_widget_options,
        },
        Widget {
            id: "resources",
            name: "System resources",
            description: "CPU, memory and disk meters for up to four systems that are up.",
            recommended: false,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"meters","rows":[{"label":"CPU","percent":12,"detail":"12.4% of 8 cores"},{"label":"Memory","percent":43,"detail":"43.0%"},{"label":"Disk","percent":61,"detail":"61.2%"}]}"#,
            fields: &[REFRESH],
            fetch: resources,
            validate: no_widget_options,
        },
        Widget {
            id: "summary",
            name: "Fleet summary",
            description: "Systems up, down and paused, and the average CPU use of those up.",
            recommended: false,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Up","value":"4","glyph":"check","tone":"up"},{"label":"Down","value":"1","glyph":"server","tone":"down"},{"label":"Paused","value":"0","glyph":"clock"},{"label":"Avg CPU","value":"9%","glyph":"cpu"}]}"#,
            fields: &[REFRESH],
            fetch: summary,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
struct Token {
    token: String,
}

#[derive(Deserialize)]
struct Page {
    items: Vec<System>,
    #[serde(rename = "totalItems")]
    total: Option<u64>,
}

#[derive(Deserialize)]
struct System {
    name: String,
    /// `up`, `down`, `paused` or `pending`.
    status: String,
    #[serde(default)]
    info: Info,
}

/// Beszel's `system.Info`, abbreviated JSON keys as the hub stores them.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Info {
    /// Uptime, seconds.
    u: u64,
    cpu: f64,
    /// Memory used, percent.
    mp: f64,
    /// Root disk used, percent.
    dp: f64,
    /// CPU cores.
    c: u32,
}

fn percent(value: f64) -> u8 {
    if value.is_finite() {
        value.round().clamp(0.0, 100.0) as u8
    } else {
        0
    }
}

async fn login(conn: &Conn<'_>) -> Result<Option<Session>, Fault> {
    let response = conn
        .request(Step {
            verb: Verb::Post,
            body: Some(Body::Json(
                Values::new(),
                &[
                    ("identity", Input::Credential(IDENTITY)),
                    ("password", Input::Credential(SECRET)),
                ],
            )),
            accept_json: true,
            ..Step::get("api/collections/users/auth-with-password")
        })
        .await?;
    // PocketBase answers a wrong email or password with 400.
    if response.status == 400 {
        return Err(Class::Auth.into());
    }
    let token: Token = parse_json(response)?;
    Ok(Some(conn.session(&token.token)?))
}

async fn list(conn: &Conn<'_>, session: &Session) -> Result<Page, Fault> {
    parse_json(
        conn.request(Step {
            // PocketBase documents the token as the bare header value.
            auth: &[Auth::Header("authorization", Input::Session(session))],
            query: &[
                ("perPage", Input::Text(PAGE)),
                ("sort", Input::Text("name")),
                ("fields", Input::Text("name,status,info")),
            ],
            accept_json: true,
            ..Step::get("api/collections/systems/records")
        })
        .await?,
    )
}

async fn with_systems(conn: &Conn<'_>) -> Result<Page, Fault> {
    conn.retry_session(
        || login(conn),
        |session| async move {
            let session = session.ok_or(Class::Auth)?;
            list(conn, &session).await
        },
    )
    .await
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { with_systems(conn).await.map(|_| ()) })
}

fn health(status: &str) -> Health {
    match status {
        "up" => Health::Up,
        "down" => Health::Down,
        _ => Health::Warn,
    }
}

fn systems<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let page = with_systems(conn).await?;
        if page.items.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No systems are visible to this Beszel user.".into(),
            });
        }
        let rows = page
            .items
            .into_iter()
            .take(MAX_ROWS)
            .map(|system| {
                let up = system.status == "up";
                let info = &system.info;
                BodyRow {
                    secondary: Some(if up {
                        format!(
                            "CPU {}% · RAM {}% · Disk {}%",
                            percent(info.cpu),
                            percent(info.mp),
                            percent(info.dp)
                        )
                    } else {
                        match system.status.as_str() {
                            "down" => "Not reporting".into(),
                            "paused" => "Monitoring paused".into(),
                            _ => "Waiting for the agent".into(),
                        }
                    }),
                    value: Some(if up {
                        human_duration(info.u)
                    } else {
                        let mut status = system.status.clone();
                        if let Some(first) = status.get_mut(0..1) {
                            first.make_ascii_uppercase();
                        }
                        display_text(conn, status)
                    }),
                    status: Some(health(&system.status)),
                    progress: up.then(|| percent(info.cpu.max(info.mp).max(info.dp))),
                    title: display_text(conn, system.name),
                    ..Default::default()
                }
            })
            .collect();
        Ok(StructuredBody::Rows { rows })
    })
}

fn resources<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let page = with_systems(conn).await?;
        let up: Vec<System> = page
            .items
            .into_iter()
            .filter(|system| system.status == "up")
            .take(MAX_METER_SYSTEMS)
            .collect();
        if up.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No system is up.".into(),
            });
        }
        let single = up.len() == 1;
        let mut rows = Vec::new();
        for system in up {
            let name = display_text(conn, system.name);
            let label = |what: &str| {
                if single {
                    what.to_string()
                } else {
                    format!("{name} {}", what.to_ascii_lowercase())
                }
            };
            let info = system.info;
            let cores = if info.c > 0 {
                format!("{:.1}% of {} cores", info.cpu, info.c)
            } else {
                format!("{:.1}%", info.cpu)
            };
            rows.push(MeterRow {
                label: label("CPU"),
                percent: percent(info.cpu),
                detail: Some(cores),
            });
            rows.push(MeterRow {
                label: label("Memory"),
                percent: percent(info.mp),
                detail: Some(format!("{:.1}%", info.mp)),
            });
            rows.push(MeterRow {
                label: label("Disk"),
                percent: percent(info.dp),
                detail: Some(format!("{:.1}%", info.dp)),
            });
        }
        Ok(StructuredBody::Meters { rows })
    })
}

fn summary<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let page = with_systems(conn).await?;
        let count = |status: &str| page.items.iter().filter(|s| s.status == status).count();
        let (up, down, paused) = (count("up"), count("down"), count("paused"));
        let cpu: Vec<f64> = page
            .items
            .iter()
            .filter(|s| s.status == "up")
            .map(|s| s.info.cpu)
            .collect();
        let average = if cpu.is_empty() {
            "–".to_string()
        } else {
            format!("{}%", percent(cpu.iter().sum::<f64>() / cpu.len() as f64))
        };
        let shown = page.items.len() as u64;
        let mut tiles = vec![
            StatTile {
                label: "Up".into(),
                value: up.to_string(),
                glyph: Some(Glyph::Check),
                delta: page
                    .total
                    .filter(|total| *total > shown)
                    .map(|total| format!("{shown} of {total} read")),
                tone: Some(Health::Up),
                series: Vec::new(),
            },
            StatTile {
                label: "Down".into(),
                value: down.to_string(),
                glyph: Some(Glyph::Server),
                delta: None,
                tone: (down > 0).then_some(Health::Down),
                series: Vec::new(),
            },
            StatTile {
                label: "Paused".into(),
                value: paused.to_string(),
                glyph: Some(Glyph::Clock),
                delta: None,
                tone: None,
                series: Vec::new(),
            },
        ];
        tiles.push(StatTile {
            label: "Avg CPU".into(),
            value: average,
            glyph: Some(Glyph::Cpu),
            delta: None,
            tone: None,
            series: Vec::new(),
        });
        Ok(StructuredBody::Tiles { tiles })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    const LOGIN: &str = "POST /prefix/api/collections/users/auth-with-password HTTP/1.1";
    const LIST: &str =
        "GET /prefix/api/collections/systems/records?perPage=50&sort=name&fields=name%2Cstatus%2Cinfo HTTP/1.1";
    const SYSTEMS: &str = r#"{"page":1,"perPage":50,"totalItems":3,"totalPages":1,"items":[
        {"name":"nas","status":"up","info":{"h":"nas","c":8,"u":1036800,"cpu":12.44,"mp":43.0,"dp":61.2,"v":"0.12.0"}},
        {"name":"pi","status":"paused","info":{}},
        {"name":"backup","status":"down","info":{"cpu":0,"mp":0,"dp":0}}]}"#;

    #[tokio::test]
    async fn signs_in_renews_the_token_and_reads_systems() {
        exercise(
            &BESZEL,
            &[(IDENTITY, "fake@example.invalid"), (SECRET, "fake-beszel-pass")],
            Values::new(),
            &[
                Exchange {
                    request: LOGIN,
                    status: 200,
                    body: r#"{"token":"fake-first-token","record":{"id":"u1"}}"#,
                },
                Exchange {
                    request: LIST,
                    status: 200,
                    body: SYSTEMS,
                },
                // The token expired: sign in again and retry once.
                Exchange {
                    request: LIST,
                    status: 401,
                    body: "",
                },
                Exchange {
                    request: LOGIN,
                    status: 200,
                    body: r#"{"token":"fake-next-token"}"#,
                },
                Exchange {
                    request: LIST,
                    status: 200,
                    body: SYSTEMS,
                },
                Exchange {
                    request: LIST,
                    status: 200,
                    body: SYSTEMS,
                },
                Exchange {
                    request: LIST,
                    status: 200,
                    body: SYSTEMS,
                },
                Exchange {
                    request: LIST,
                    status: 200,
                    body: "not json",
                },
                Exchange {
                    request: LIST,
                    status: 401,
                    body: "",
                },
                Exchange {
                    request: LOGIN,
                    status: 400,
                    body: r#"{"code":400,"message":"Failed to authenticate."}"#,
                },
            ],
            |index, wire| {
                let (head, body) = wire.split_once("\r\n\r\n").unwrap();
                assert!(
                    !head.contains("fake@example.invalid") && !head.contains("fake-beszel-pass")
                );
                if [0, 3, 9].contains(&index) {
                    assert!(!head.contains("authorization:"));
                    assert_eq!(
                        serde_json::from_str::<Value>(body).unwrap(),
                        json!({"identity":"fake@example.invalid","password":"fake-beszel-pass"})
                    );
                } else {
                    let token = if index < 3 {
                        "fake-first-token"
                    } else {
                        "fake-next-token"
                    };
                    assert_header(wire, &format!("authorization: {token}"), token);
                    assert!(body.is_empty());
                }
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let rows = super::super::fetch(conn, &BESZEL.widgets[0], &Values::new())
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(rows).unwrap()["rows"],
                    json!([
                        {"title":"nas","secondary":"CPU 12% · RAM 43% · Disk 61%","value":human_duration(1036800),"status":"up","progress":61},
                        {"title":"pi","secondary":"Monitoring paused","value":"Paused","status":"warn"},
                        {"title":"backup","secondary":"Not reporting","value":"Down","status":"down"},
                    ])
                );
                let meters = super::super::fetch(conn, &BESZEL.widgets[1], &Values::new())
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(meters).unwrap()["rows"],
                    json!([
                        {"label":"CPU","percent":12,"detail":"12.4% of 8 cores"},
                        {"label":"Memory","percent":43,"detail":"43.0%"},
                        {"label":"Disk","percent":61,"detail":"61.2%"},
                    ])
                );
                let tiles = super::super::fetch(conn, &BESZEL.widgets[2], &Values::new())
                    .await
                    .unwrap();
                let tiles = serde_json::to_value(tiles).unwrap();
                let values: Vec<_> = tiles["tiles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| (t["label"].clone(), t["value"].clone()))
                    .collect();
                assert_eq!(
                    values,
                    [
                        (json!("Up"), json!("1")),
                        (json!("Down"), json!("1")),
                        (json!("Paused"), json!("1")),
                        (json!("Avg CPU"), json!("12%")),
                    ]
                );
                assert_eq!(tiles["tiles"][1]["tone"], "down");
                assert_test_fault(conn, &BESZEL, Class::Protocol).await;
                assert_test_fault(conn, &BESZEL, Class::Auth).await;
            },
        )
        .await;
    }
}
