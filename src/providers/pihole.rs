use super::*;

const PASSWORD: &str = "password";
const TOKEN: &str = "api_token";

pub const PI_HOLE: Provider = Provider {
    id: "pihole",
    name: "Pi-hole",
    category: "Network",
    icon: "pi-hole",
    description: "Show DNS query counts and blocking status.",
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
    widgets: &[Widget {
        id: "dns",
        name: "DNS activity",
        description: "Show queries, blocked queries, percentage blocked, and blocking status.",
        recommended: true,
        fields: &[REFRESH],
        validate: |_| Ok(()),
        fetch,
    }],
};

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async { activity(conn).await.map(drop) })
}

fn fetch<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, widgets::StructuredBody> {
    Box::pin(async { activity(conn).await })
}

async fn activity(conn: &Conn<'_>) -> Result<widgets::StructuredBody, Fault> {
    if conn.options().get("version").and_then(Value::as_str) == Some("v5") {
        return v5(conn).await;
    }
    if !conn.has_credential(PASSWORD) {
        return v6(conn, None).await;
    }
    if !conn.uses_session_cache() {
        let sid = login(conn).await?;
        let result = v6(conn, Some(&sid)).await;
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
        |sid| async move { v6(conn, sid.as_ref()).await },
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

async fn v6(conn: &Conn<'_>, session: Option<&Session>) -> Result<widgets::StructuredBody, Fault> {
    let auth = session.map(|sid| [Auth::Header("x-ftl-sid", Input::Session(sid))]);
    let auth = auth.as_ref().map_or(&[][..], |auth| &auth[..]);
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

async fn v5(conn: &Conn<'_>) -> Result<widgets::StructuredBody, Fault> {
    let query = [
        ("summaryRaw", Input::Text("")),
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
    // The v5.21 PHP endpoint returns an empty array for an unauthorized summary.
    if data.as_array().is_some_and(Vec::is_empty) {
        return Err(Class::Auth.into());
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
        fake: impl Future<Output = ()> + Send + 'static,
    ) -> widgets::StructuredBody {
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
        let body = conn.run(activity(&conn)).await.unwrap();
        task.await.unwrap();
        body
    }

    #[tokio::test]
    async fn v6_renews_an_expired_sid_and_fetches_stats_and_blocking() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = request_activity(addr, "v6", (PASSWORD, "test password"), async move {
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
        })
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
        let body = request_activity(addr, "v5", (TOKEN, "test+/=&"), async move {
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
}
