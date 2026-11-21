use super::*;
use std::collections::HashMap;
use widgets::{Health, StructuredBody};

pub const UPTIME_KUMA: Provider = Provider {
    id: "uptime-kuma",
    name: "Uptime Kuma",
    category: "Monitoring",
    icon: "uptime-kuma",
    description: "Show monitor health, 24-hour uptime and the pinned incident from a status page.",
    docs: "https://github.com/louislam/uptime-kuma/wiki",
    public_internet: false,
    contacts: "Your configured Uptime Kuma service. Public addresses require internet access.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(100),
            ..Field::new(
                "slug",
                "Status page slug",
                Kind::Text,
                "Enter the final part of the published status page URL, for example home from /status/home.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "monitors",
        name: "Monitor status",
        description: "Current state, 24-hour uptime and the active pinned incident.",
        recommended: true,
        fields: &[REFRESH],
        fetch: monitors,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    public_group_list: Vec<Group>,
    incident: Option<Incident>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Group {
    monitor_list: Vec<Monitor>,
}

#[derive(Deserialize)]
struct Monitor {
    id: u64,
    name: String,
}

#[derive(Deserialize)]
struct Incident {
    title: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Heartbeats {
    heartbeat_list: HashMap<String, Vec<Heartbeat>>,
    uptime_list: HashMap<String, Option<f64>>,
}

#[derive(Deserialize)]
struct Heartbeat {
    status: u8,
}

fn slug<'a>(conn: &'a Conn<'_>) -> Result<&'a str, Fault> {
    conn.options()
        .get("slug")
        .and_then(Value::as_str)
        .filter(|slug| {
            !slug.is_empty() && slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .ok_or_else(|| Class::Protocol.into())
}

async fn page(conn: &Conn<'_>, slug: &str) -> Result<Page, Fault> {
    parse_json(
        conn.request(Step::get(&format!("api/status-page/{slug}")))
            .await?,
    )
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { page(conn, slug(conn)?).await.map(|_| ()) })
}

fn monitors<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let slug = slug(conn)?;
        let page = page(conn, slug).await?;
        let data: Heartbeats = parse_json(
            conn.request(Step::get(&format!("api/status-page/heartbeat/{slug}")))
                .await?,
        )?;
        let mut rows = Vec::new();
        if let Some(incident) = page.incident {
            let title = display_text(conn, incident.title);
            let content = display_text(conn, incident.content);
            rows.push((format!("Incident: {title} — {content}"), Health::Warn));
        }
        let mut monitors = page
            .public_group_list
            .into_iter()
            .flat_map(|group| group.monitor_list);
        for monitor in monitors.by_ref().take(24) {
            let (label, health) = match data
                .heartbeat_list
                .get(&monitor.id.to_string())
                .and_then(|beats| beats.last())
                .map(|beat| beat.status)
            {
                Some(0) => ("Down", Health::Down),
                Some(1) => ("Up", Health::Up),
                Some(2) => ("Pending", Health::Warn),
                Some(3) => ("Maintenance", Health::Warn),
                _ => ("Unknown", Health::Warn),
            };
            let uptime = data
                .uptime_list
                .get(&format!("{}_24", monitor.id))
                .copied()
                .flatten()
                .filter(|uptime| (0.0..=1.0).contains(uptime))
                .map_or_else(
                    || "24h uptime unavailable".into(),
                    |uptime| format!("{:.1}% over 24h", uptime * 100.0),
                );
            let name = display_text(conn, monitor.name);
            rows.push((format!("{name} — {label}, {uptime}"), health));
        }
        let remaining = monitors.count();
        if remaining > 0 {
            rows.push((
                format!("{remaining} more monitors on the status page"),
                Health::Warn,
            ));
        }
        if rows.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No monitors on this status page.".into(),
            });
        }
        Ok(StructuredBody::StatusList { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{exercise, Exchange};

    #[tokio::test]
    async fn public_page_and_heartbeat_map_status_and_incident() {
        const PAGE: &str = "GET /prefix/api/status-page/home HTTP/1.1";
        const PAGE_BODY: &str = r#"{"config":{"slug":"home"},"incident":{"title":"Service outage","content":"Investigating"},"publicGroupList":[{"monitorList":[{"id":1,"name":"Web"},{"id":2,"name":"DNS"},{"id":3,"name":"Mail"},{"id":4,"name":"Storage"}]}]}"#;
        exercise(
            &UPTIME_KUMA,
            &[],
            [("slug".into(), "home".into())].into_iter().collect(),
            &[
                Exchange { request: PAGE, status: 200, body: PAGE_BODY },
                Exchange { request: PAGE, status: 200, body: PAGE_BODY },
                Exchange {
                    request: "GET /prefix/api/status-page/heartbeat/home HTTP/1.1",
                    status: 200,
                    body: r#"{"heartbeatList":{"1":[{"status":0},{"status":1}],"2":[{"status":0}],"3":[{"status":2}],"4":[{"status":3}]},"uptimeList":{"1_24":0.995,"2_24":0,"3_24":null,"4_24":1}}"#,
                },
                Exchange { request: PAGE, status: 403, body: "" },
                Exchange { request: PAGE, status: 200, body: "{}" },
            ],
            |_, wire| {
                assert!(!wire.contains("authorization:"));
                assert!(!wire.contains("cookie:"));
            },
            async |conn| {
                conn.run((UPTIME_KUMA.test)(conn)).await.unwrap();
                let body = super::super::fetch(conn, &UPTIME_KUMA.widgets[0], &Values::new()).await.unwrap();
                assert_eq!(serde_json::to_value(body).unwrap()["rows"], serde_json::json!([
                    ["Incident: Service outage — Investigating", "warn"],
                    ["Web — Up, 99.5% over 24h", "up"],
                    ["DNS — Down, 0.0% over 24h", "down"],
                    ["Mail — Pending, 24h uptime unavailable", "warn"],
                    ["Storage — Maintenance, 100.0% over 24h", "warn"]
                ]));
                for class in [Class::Auth, Class::Protocol] {
                    let fault = conn.run((UPTIME_KUMA.test)(conn)).await.unwrap_err();
                    assert_eq!(fault.class.message(), class.message());
                }
            },
        ).await;
    }
}
