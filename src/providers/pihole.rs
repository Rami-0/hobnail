use super::*;

const PASSWORD: &str = "password";
const TOKEN: &str = "api_token";

pub const PI_HOLE: Provider = Provider {
    id: "pihole",
    name: "Pi-hole",
    category: "Network",
    icon: "pi-hole",
    description: "Show DNS query counts, blocking status, and top domains and clients.",
    docs: "https://docs.pi-hole.net/api/auth/",
    public_internet: false,
    contacts: "Your configured Pi-hole service. Public addresses require internet access.",
    fields: &[
        Field {
            required: true,
            max_length: Some(2048),
            ..Field::new("base_url", "Service URL", Kind::Url,
                "Enter the Pi-hole URL, including any reverse-proxy prefix. Omit /admin and /api.")
        },
        Field {
            default: Some("v6"),
            choices: &["v6", "v5"],
            ..Field::new("version", "API version", Kind::Select, "")
        },
        Field {
            max_length: Some(4096),
            ..Field::new(PASSWORD, "v6 password", Kind::Secret,
                "Use an application password or the web password. Leave blank if authentication is disabled.")
        },
        Field {
            max_length: Some(4096),
            ..Field::new(TOKEN, "v5 API token", Kind::Secret,
                "Used only with v5. Leave blank if authentication is disabled.")
        },
        Field {
            default: Some("true"),
            ..Field::new("enabled", "Enabled", Kind::Bool, "")
        },
    ],
    test,
    widgets: &[
        Widget {
            id: "dns",
            name: "DNS activity",
            description: "Show queries, blocked queries, percentage blocked, and blocking status.",
            recommended: true,
            fields: &[REFRESH],
            validate: |_| Ok(()),
            fetch,
        },
        Widget {
            id: "top_domains",
            name: "Top domains",
            description: "The ten most queried domains, or the ten most blocked.",
            recommended: false,
            fields: &[
                Field {
                    default: Some("false"),
                    ..Field::new("blocked", "Show blocked domains", Kind::Bool, "")
                },
                REFRESH,
            ],
            validate: |_| Ok(()),
            fetch: top_domains,
        },
        Widget {
            id: "top_clients",
            name: "Top clients",
            description: "The ten clients that sent the most queries.",
            recommended: false,
            fields: &[REFRESH],
            validate: |_| Ok(()),
            fetch: top_clients,
        },
    ],
};

const TOP_COUNT: usize = 10;

#[derive(Clone, Copy)]
enum Report {
    Summary,
    Domains { blocked: bool },
    Clients,
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async { activity(conn, Report::Summary).await.map(drop) })
}

fn fetch<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, widgets::StructuredBody> {
    Box::pin(async { activity(conn, Report::Summary).await })
}

fn top_domains<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, widgets::StructuredBody> {
    let blocked = options.get("blocked").and_then(Value::as_bool) == Some(true);
    Box::pin(async move { activity(conn, Report::Domains { blocked }).await })
}

fn top_clients<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, widgets::StructuredBody> {
    Box::pin(async { activity(conn, Report::Clients).await })
}

async fn activity(conn: &Conn<'_>, report: Report) -> Result<widgets::StructuredBody, Fault> {
    if conn.options().get("version").and_then(Value::as_str) == Some("v5") {
        return v5(conn, report).await;
    }
    if !conn.has_credential(PASSWORD) {
        return v6(conn, None, report).await;
    }
    if !conn.uses_session_cache() {
        let sid = login(conn).await?;
        let result = v6(conn, Some(&sid), report).await;
        let _ = conn
            .request(Step {
                verb: Verb::Delete,
                auth: &[Auth::Header("x-ftl-sid", Input::Session(&sid))],
                ..Step::get("/api/auth")
            })
            .await;
        return result;
    }
    conn.retry_session(
        || async { login(conn).await.map(Some) },
        |sid| async move { v6(conn, sid.as_ref(), report).await },
    )
    .await
}

async fn login(conn: &Conn<'_>) -> Result<Session, Fault> {
    let session = json(
        conn,
        Step {
            verb: Verb::Post,
            body: Some(Body::Json(
                Values::new(),
                &[("password", Input::Credential(PASSWORD))],
            )),
            ..Step::get("/api/auth")
        },
    )
    .await?;
    let session = &session["session"];
    if session["valid"].as_bool() != Some(true) {
        return Err(Class::Auth.into());
    }
    conn.session(session["sid"].as_str().ok_or(Class::Protocol)?)
}

async fn v6(
    conn: &Conn<'_>,
    session: Option<&Session>,
    report: Report,
) -> Result<widgets::StructuredBody, Fault> {
    let auth = session.map(|sid| [Auth::Header("x-ftl-sid", Input::Session(sid))]);
    let auth = auth.as_ref().map_or(&[][..], |auth| &auth[..]);
    let count = TOP_COUNT.to_string();
    let (path, list, name, blocked) = match report {
        Report::Summary => return v6_summary(conn, auth).await,
        Report::Domains { blocked } => ("/api/stats/top_domains", "domains", "Domain", blocked),
        Report::Clients => ("/api/stats/top_clients", "clients", "Client", false),
    };
    let query = [
        ("count", Input::Text(&count)),
        ("blocked", Input::Text("true")),
    ];
    let data = json(
        conn,
        Step {
            auth,
            query: if blocked { &query } else { &query[..1] },
            ..Step::get(path)
        },
    )
    .await?;
    let entries = data[list].as_array().ok_or(Class::Protocol)?;
    let redactor = conn.output_redactor();
    let mut rows = Vec::with_capacity(entries.len().min(TOP_COUNT));
    for entry in entries.iter().take(TOP_COUNT) {
        let label = if let Report::Clients = report {
            let ip = entry["ip"].as_str().ok_or(Class::Protocol)?;
            match entry["name"].as_str().filter(|name| !name.is_empty()) {
                // Joining can reshape a secret; check the parts first.
                Some(name) => redactor.transformed(&[name, ip], format!("{name} ({ip})")),
                None => ip.into(),
            }
        } else {
            entry["domain"].as_str().ok_or(Class::Protocol)?.into()
        };
        rows.push((label, entry["count"].as_u64().ok_or(Class::Protocol)?));
    }
    Ok(table(conn, name, rows))
}

/// A ranked name and count table, most queries first.
fn table(conn: &Conn<'_>, name: &str, rows: Vec<(String, u64)>) -> widgets::StructuredBody {
    if rows.is_empty() {
        return widgets::StructuredBody::Stat {
            value: "No queries recorded".into(),
        };
    }
    widgets::StructuredBody::Table {
        headings: vec![name.into(), "Queries".into()],
        rows: rows
            .into_iter()
            .map(|(label, count)| vec![display_text(conn, label), count.to_string()])
            .collect(),
    }
}

async fn v6_summary(conn: &Conn<'_>, auth: &[Auth<'_>]) -> Result<widgets::StructuredBody, Fault> {
    let summary = json(
        conn,
        Step {
            auth,
            ..Step::get("/api/stats/summary")
        },
    )
    .await?;
    let status = json(
        conn,
        Step {
            auth,
            ..Step::get("/api/dns/blocking")
        },
    )
    .await?;
    let queries = &summary["queries"];
    stats(
        "Queries",
        &queries["total"],
        &queries["blocked"],
        &queries["percent_blocked"],
        &status["blocking"],
    )
}

async fn v5(conn: &Conn<'_>, report: Report) -> Result<widgets::StructuredBody, Fault> {
    let count = TOP_COUNT.to_string();
    let (request, key, name) = match report {
        Report::Summary => ("summaryRaw", "", ""),
        Report::Domains { blocked: false } => ("topItems", "top_queries", "Domain"),
        Report::Domains { blocked: true } => ("topItems", "top_ads", "Domain"),
        Report::Clients => ("getQuerySources", "top_sources", "Client"),
    };
    let value = if key.is_empty() { "" } else { count.as_str() };
    let query = [
        (request, Input::Text(value)),
        ("auth", Input::Credential(TOKEN)),
    ];
    let query = if conn.has_credential(TOKEN) {
        &query[..]
    } else {
        &query[..1]
    };
    let data = json(
        conn,
        Step {
            query,
            ..Step::get("/admin/api.php")
        },
    )
    .await?;
    // The v5.21 PHP endpoint returns an empty array for an unauthorized request.
    if data.as_array().is_some_and(Vec::is_empty) {
        return Err(Class::Auth.into());
    }
    if !key.is_empty() {
        // PHP objects of name => count; clients are "hostname|ip" or "ip".
        // The JSON map loses PHP's order, so rank by count here.
        let redactor = conn.output_redactor();
        let mut rows = data[key]
            .as_object()
            .ok_or(Class::Protocol)?
            .iter()
            .map(|(label, count)| {
                // Splitting at '|' can reshape a secret; check the raw key.
                let label = match label.split_once('|') {
                    Some((name, ip)) => redactor.transformed(&[label], format!("{name} ({ip})")),
                    None => label.clone(),
                };
                Ok((label, count.as_u64().ok_or(Class::Protocol)?))
            })
            .collect::<Result<Vec<_>, Fault>>()?;
        rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        rows.truncate(TOP_COUNT);
        return Ok(table(conn, name, rows));
    }
    stats(
        "Queries today",
        &data["dns_queries_today"],
        &data["ads_blocked_today"],
        &data["ads_percentage_today"],
        &data["status"],
    )
}

async fn json(conn: &Conn<'_>, step: Step<'_>) -> Result<Value, Fault> {
    let response = success(conn.request(step).await?)?;
    serde_json::from_slice(&response.body).map_err(|_| Class::Protocol.into())
}

fn stats(
    label: &str,
    queries: &Value,
    blocked: &Value,
    percentage: &Value,
    status: &Value,
) -> Result<widgets::StructuredBody, Fault> {
    let count = |value: &Value| {
        value
            .as_u64()
            .or_else(|| {
                // v5 casts its counters to floats before JSON serialization.
                let count = value.as_f64()?;
                (count >= 0.0 && count < u64::MAX as f64 && count.fract() == 0.0)
                    .then_some(count as u64)
            })
            .ok_or(Class::Protocol)
    };
    let percentage = percentage
        .as_f64()
        .filter(|n| (0.0..=100.0).contains(n))
        .ok_or(Class::Protocol)?;
    let status = match status.as_str() {
        Some("enabled") => "Enabled",
        Some("disabled") => "Disabled",
        Some("failed") => "Failed",
        Some("unknown") => "Unknown",
        _ => return Err(Class::Protocol.into()),
    };
    Ok(widgets::StructuredBody::StatGrid {
        rows: vec![
            (label.into(), count(queries)?.to_string()),
            ("Blocked".into(), count(blocked)?.to_string()),
            ("Blocked percentage".into(), format!("{percentage:.1}%")),
            ("Blocking".into(), status.into()),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::connection::tests::{receive, respond};
    use crate::{config::HttpPolicy, http::client::BoundCredential};
    use std::sync::Arc;
    use tokio::{net::TcpListener, sync::Semaphore};

    async fn request_activity(
        addr: std::net::SocketAddr,
        version: &'static str,
        credential: (&'static str, &'static str),
        report: Report,
        fake: impl Future<Output = ()> + Send + 'static,
    ) -> widgets::StructuredBody {
        try_activity(addr, version, credential, report, fake)
            .await
            .unwrap()
    }

    async fn try_activity(
        addr: std::net::SocketAddr,
        version: &'static str,
        credential: (&'static str, &'static str),
        report: Report,
        fake: impl Future<Output = ()> + Send + 'static,
    ) -> Result<widgets::StructuredBody, Fault> {
        let origin = format!("http://{addr}");
        let mut target = Target::new(
            format!("{origin}/dns"),
            [(
                credential.0.into(),
                BoundCredential {
                    origin,
                    token: credential.1.into(),
                },
            )]
            .into_iter()
            .collect(),
            false,
        );
        target.options.insert("version".into(), version.into());
        let connections = Connections::default();
        connections.bind(&mut target, Some("pihole-test"), true);
        let policy = HttpPolicy {
            allow_hosts: vec!["127.0.0.1".into()],
        };
        let permit = Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap();
        let conn = Conn::new(&target, &policy, &connections, permit).unwrap();
        let task = tokio::spawn(fake);
        let body = conn.run(activity(&conn, report)).await;
        task.await.unwrap();
        body
    }

    #[tokio::test]
    async fn v6_wrong_password_is_an_auth_fault_without_a_stats_request() {
        // FTL answers a wrong password with 401 and an invalid session; a
        // 200 with valid:false is treated the same.
        for (status, body) in [
            (
                401,
                r#"{"session":{"valid":false,"totp":false,"sid":null,"validity":-1,"message":"password incorrect"},"took":0.003}"#,
            ),
            (
                200,
                r#"{"session":{"valid":false,"totp":false,"sid":null,"validity":-1},"took":0.003}"#,
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let fault = try_activity(
                addr,
                "v6",
                (PASSWORD, "wrong password"),
                Report::Summary,
                async move {
                    let (mut socket, wire) = receive(&listener).await;
                    assert!(wire.starts_with("POST /dns/api/auth HTTP/1.1"));
                    respond(&mut socket, status, "", body).await;
                    // No stats request follows a refused login.
                    assert!(tokio::time::timeout(
                        std::time::Duration::from_millis(200),
                        listener.accept()
                    )
                    .await
                    .is_err());
                },
            )
            .await
            .unwrap_err();
            assert!(matches!(fault.class, Class::Auth));
        }
    }

    #[tokio::test]
    async fn v5_unauthorized_empty_array_is_an_auth_fault() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fault = try_activity(addr, "v5", (TOKEN, "wrong"), Report::Summary, async move {
            let (mut socket, _) = receive(&listener).await;
            respond(&mut socket, 200, "", "[]").await;
        })
        .await
        .unwrap_err();
        assert!(matches!(fault.class, Class::Auth));
    }

    #[tokio::test]
    async fn v6_renews_an_expired_sid_and_fetches_stats_and_blocking() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = request_activity(
            addr,
            "v6",
            (PASSWORD, "test password"),
            Report::Summary,
            async move {
                for (sid, status) in [("first", 401), ("second", 200)] {
                    let (mut socket, wire) = receive(&listener).await;
                    assert!(wire.starts_with("POST /dns/api/auth HTTP/1.1"));
                    assert!(wire.ends_with(r#"{"password":"test password"}"#));
                    respond(
                        &mut socket,
                        200,
                        "",
                        &format!(r#"{{"session":{{"valid":true,"sid":"{sid}"}}}}"#),
                    )
                    .await;
                    let (mut socket, wire) = receive(&listener).await;
                    assert!(wire.starts_with("GET /dns/api/stats/summary HTTP/1.1"));
                    assert!(wire.contains(&format!("x-ftl-sid: {sid}\r\n")));
                    respond(
                        &mut socket,
                        status,
                        "",
                        r#"{"queries":{"total":42,"blocked":21,"percent_blocked":50}}"#,
                    )
                    .await;
                }
                let (mut socket, wire) = receive(&listener).await;
                assert!(wire.starts_with("GET /dns/api/dns/blocking HTTP/1.1"));
                assert!(wire.contains("x-ftl-sid: second\r\n"));
                respond(&mut socket, 200, "", r#"{"blocking":"enabled"}"#).await;
            },
        )
        .await;
        assert_eq!(
            serde_json::to_value(body).unwrap()["rows"],
            serde_json::json!([
                ["Queries", "42"],
                ["Blocked", "21"],
                ["Blocked percentage", "50.0%"],
                ["Blocking", "Enabled"],
            ])
        );
    }

    #[tokio::test]
    async fn v5_uses_an_encoded_token_and_reads_the_legacy_summary() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = request_activity(addr, "v5", (TOKEN, "test+/=&"), Report::Summary, async move {
            let (mut socket, wire) = receive(&listener).await;
            assert!(wire.starts_with("GET /dns/admin/api.php?summaryRaw=&auth=test%2B%2F%3D%26 HTTP/1.1"));
            respond(&mut socket, 200, "", r#"{"dns_queries_today":100.0,"ads_blocked_today":15,"ads_percentage_today":15.0,"status":"disabled"}"#).await;
        }).await;
        assert_eq!(
            serde_json::to_value(body).unwrap()["rows"],
            serde_json::json!([
                ["Queries today", "100"],
                ["Blocked", "15"],
                ["Blocked percentage", "15.0%"],
                ["Blocking", "Disabled"],
            ])
        );
    }

    #[tokio::test]
    async fn v6_top_lists_use_the_session_and_rank_domains_and_clients() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let login = async |listener: &TcpListener| {
            let (mut socket, wire) = receive(listener).await;
            assert!(wire.starts_with("POST /dns/api/auth HTTP/1.1"));
            respond(
                &mut socket,
                200,
                "",
                r#"{"session":{"valid":true,"sid":"top-sid","csrf":"x","validity":1800}}"#,
            )
            .await;
        };
        let body = request_activity(addr, "v6", (PASSWORD, "test password"), Report::Domains { blocked: true }, async move {
            login(&listener).await;
            let (mut socket, wire) = receive(&listener).await;
            assert!(wire.starts_with("GET /dns/api/stats/top_domains?count=10&blocked=true HTTP/1.1"));
            assert!(wire.contains("x-ftl-sid: top-sid\r\n"));
            respond(&mut socket, 200, "", r#"{"domains":[{"domain":"doubleclick.net","count":8516},{"domain":"ads.example","count":12}],"total_queries":29160,"blocked_queries":6379,"took":0.001}"#).await;
        }).await;
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            serde_json::json!({"kind":"table","headings":["Domain","Queries"],"rows":[["doubleclick.net","8516"],["ads.example","12"]]})
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = request_activity(addr, "v6", (PASSWORD, "test password"), Report::Clients, async move {
            login(&listener).await;
            let (mut socket, wire) = receive(&listener).await;
            assert!(wire.starts_with("GET /dns/api/stats/top_clients?count=10 HTTP/1.1"));
            let clients: Vec<_> = (0..12).map(|i| serde_json::json!({"ip":format!("192.168.0.{i}"),"name":if i == 0 { "raspberrypi.lan" } else { "" },"count":100 - i})).collect();
            respond(&mut socket, 200, "", &serde_json::json!({"clients":clients,"total_queries":29160,"blocked_queries":6379,"took":0.001}).to_string()).await;
        }).await;
        let body = serde_json::to_value(body).unwrap();
        assert_eq!(body["headings"], serde_json::json!(["Client", "Queries"]));
        assert_eq!(body["rows"].as_array().unwrap().len(), TOP_COUNT);
        assert_eq!(
            body["rows"][0],
            serde_json::json!(["raspberrypi.lan (192.168.0.0)", "100"])
        );
        assert_eq!(body["rows"][1], serde_json::json!(["192.168.0.1", "99"]));
    }

    #[tokio::test]
    async fn v5_top_lists_use_the_token_and_rank_the_php_maps() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = request_activity(addr, "v5", (TOKEN, "test+/=&"), Report::Domains { blocked: false }, async move {
            let (mut socket, wire) = receive(&listener).await;
            assert!(wire.starts_with("GET /dns/admin/api.php?topItems=10&auth=test%2B%2F%3D%26 HTTP/1.1"));
            respond(&mut socket, 200, "", r#"{"top_queries":{"a.example":3,"z.example":40,"m.example":40},"top_ads":{"ads.example":9}}"#).await;
        }).await;
        assert_eq!(
            serde_json::to_value(body).unwrap()["rows"],
            serde_json::json!([["m.example", "40"], ["z.example", "40"], ["a.example", "3"]])
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = request_activity(
            addr,
            "v5",
            (TOKEN, "test+/=&"),
            Report::Clients,
            async move {
                let (mut socket, wire) = receive(&listener).await;
                assert!(wire.starts_with(
                    "GET /dns/admin/api.php?getQuerySources=10&auth=test%2B%2F%3D%26 HTTP/1.1"
                ));
                respond(
                    &mut socket,
                    200,
                    "",
                    r#"{"top_sources":{"laptop.lan|192.168.0.9":70,"192.168.0.4":5}}"#,
                )
                .await;
            },
        )
        .await;
        assert_eq!(
            serde_json::to_value(body).unwrap()["rows"],
            serde_json::json!([["laptop.lan (192.168.0.9)", "70"], ["192.168.0.4", "5"]])
        );
    }

    #[tokio::test]
    async fn client_labels_withhold_secrets_reshaped_by_splitting_or_joining() {
        // v5: the token itself contains '|', so "tok|en42" would become
        // "tok (en42)" and escape the final redaction pass.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = request_activity(
            addr,
            "v5",
            (TOKEN, "tok|en42"),
            Report::Clients,
            async move {
                let (mut socket, _) = receive(&listener).await;
                respond(
                    &mut socket,
                    200,
                    "",
                    r#"{"top_sources":{"tok|en42":9,"laptop.lan|192.168.0.9":7}}"#,
                )
                .await;
            },
        )
        .await;
        assert_eq!(
            serde_json::to_value(body).unwrap()["rows"],
            serde_json::json!([["<redacted>", "9"], ["laptop.lan (192.168.0.9)", "7"]])
        );
        // v6: name and ip are joined; a name echoing the session is withheld whole.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = request_activity(addr, "v6", (PASSWORD, "test password"), Report::Clients, async move {
            let (mut socket, _) = receive(&listener).await;
            respond(&mut socket, 200, "", r#"{"session":{"valid":true,"sid":"echoed-sid-1"}}"#).await;
            let (mut socket, _) = receive(&listener).await;
            respond(&mut socket, 200, "", r#"{"clients":[{"ip":"192.168.0.2","name":"echoed-sid-1","count":3},{"ip":"192.168.0.3","name":"tv.lan","count":1}]}"#).await;
        }).await;
        assert_eq!(
            serde_json::to_value(body).unwrap()["rows"],
            serde_json::json!([["<redacted>", "3"], ["tv.lan (192.168.0.3)", "1"]])
        );
    }

    #[tokio::test]
    async fn v5_refuses_encoded_echoes_in_the_only_upstream_text_field() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        const TOKEN_VALUE: &str = "xy~*>&/!";
        let mut target = Target::new(
            origin.clone(),
            [(
                TOKEN.into(),
                BoundCredential {
                    origin,
                    token: TOKEN_VALUE.into(),
                },
            )]
            .into_iter()
            .collect(),
            false,
        );
        target.options.insert("version".into(), "v5".into());
        let policy = HttpPolicy {
            allow_hosts: vec!["127.0.0.1".into()],
        };
        let connections = Connections::default();
        let permit = Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap();
        let conn = Conn::new(&target, &policy, &connections, permit).unwrap();
        let echoes = super::super::redaction::tests::encoded_echoes(TOKEN_VALUE);
        let count = echoes.len();
        let fake = tokio::spawn(async move {
            for echo in echoes {
                let (mut socket, _) = receive(&listener).await;
                respond(&mut socket, 200, "", &serde_json::json!({
                    "dns_queries_today":100, "ads_blocked_today":15, "ads_percentage_today":15,
                    "status":echo, "secondary":echo, "link":format!("https://example.test/{echo}")
                }).to_string()).await;
            }
        });
        for _ in 0..count {
            let error = super::super::fetch(&conn, &PI_HOLE.widgets[0], &Values::new())
                .await
                .unwrap_err();
            assert!(matches!(error.class, Class::Protocol));
        }
        fake.await.unwrap();
    }
}
