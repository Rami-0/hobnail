use super::json_scan::{Path, Scalar, Scanner, Visit};
use super::pihole::grouped;
use super::*;
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

pub const UPTIME_KUMA: Provider = Provider {
    id: "uptime-kuma",
    name: "Uptime Kuma",
    category: "Monitoring",
    icon: "uptime-kuma",
    color: "#3cc07a",
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
    widgets: &[
        Widget {
            id: "monitors",
            name: "Monitor status",
            description: "Each monitor's state, latest response time, 24-hour uptime and recent heartbeats, with the active pinned incident.",
            recommended: true,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"Website","secondary":"Up · 84 ms","value":"99.9%","status":"up","bars":["up","up","up","up","up","up","up","up","up","up","up","up","up","up","up","up","up","up","up","up"]},{"title":"Mail server","secondary":"Up · 31 ms","value":"99.2%","status":"up","bars":["up","up","up","up","down","up","up","up","up","up","up","up","up","up","up","up","up","up","up","up"]},{"title":"Backup NAS","secondary":"Pending","value":"97.5%","status":"warn","bars":["up","up","up","up","up","up","up","up","up","up","up","up","up","up","up","up","up","up","warn","warn"]},{"title":"VPN gateway","secondary":"Down","value":"88.0%","status":"down","bars":["up","up","up","up","up","up","up","up","up","up","up","up","up","up","up","up","down","down","down","down"]}]}"#,
            fields: &[REFRESH],
            fetch: monitors,
            validate: no_widget_options,
        },
        Widget {
            id: "summary",
            name: "Status summary",
            description: "How many monitors are up, down or pending, and their average 24-hour uptime.",
            recommended: false,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Up","value":"11","glyph":"check","tone":"up"},{"label":"Down","value":"1","glyph":"alert","tone":"down"},{"label":"Pending","value":"0","glyph":"clock"},{"label":"Uptime (24 h)","value":"99.1%","glyph":"activity"}]}"#,
            fields: &[REFRESH],
            fetch: summary,
            validate: no_widget_options,
        },
    ],
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

const MAX_MONITORS: usize = 24;

/// The latest heartbeat and 24-hour uptime of each displayed monitor, read
/// while the heartbeat response streams. Kuma returns up to 100 beats per
/// monitor with no size option, so the response passes 128 KiB at about
/// 20 monitors.
struct Heartbeats {
    monitors: Vec<(u64, Beat)>,
    lists: bool,
    uptimes: bool,
    not_object: bool,
}

#[derive(Clone, Default)]
struct Beat {
    latest: Option<u64>,
    uptime: Option<f64>,
    /// Milliseconds of the latest beat that reported a ping.
    ping: Option<f64>,
    /// The newest [`BARS`] beats, oldest first.
    bars: Vec<Health>,
}

/// Heartbeats shown as bars per monitor.
const BARS: usize = 30;

fn health(status: Option<u64>) -> Health {
    match status {
        Some(1) => Health::Up,
        Some(0) => Health::Down,
        _ => Health::Warn,
    }
}

impl Heartbeats {
    fn new(ids: impl Iterator<Item = u64>) -> Self {
        Self {
            monitors: ids.map(|id| (id, Beat::default())).collect(),
            lists: false,
            uptimes: false,
            not_object: false,
        }
    }

    fn get(&self, id: u64) -> Option<&Beat> {
        self.monitors
            .iter()
            .find(|(key, _)| *key == id)
            .map(|(_, beat)| beat)
    }

    fn monitor(&mut self, key: Option<&str>) -> Option<&mut Beat> {
        let id: u64 = key?.parse().ok()?;
        self.monitors
            .iter_mut()
            .find(|(key, _)| *key == id)
            .map(|(_, beat)| beat)
    }
}

impl Visit for Heartbeats {
    fn value(&mut self, path: &Path<'_>, value: Scalar<'_>) {
        let object = matches!(value, Scalar::Container { object: true });
        match (path.depth(), path.key(1)) {
            (0, _) => self.not_object |= !object,
            (1, Some("heartbeatList")) => self.lists |= object,
            (1, Some("uptimeList")) => self.uptimes |= object,
            // heartbeatList.<id> is in time order, so the last status wins.
            (4, Some("heartbeatList")) if path.key(4) == Some("status") => {
                let status = match value {
                    Scalar::Number {
                        text,
                        complete: true,
                    } => text.parse().ok(),
                    _ => None,
                };
                if let Some(beat) = self.monitor(path.key(2)) {
                    beat.latest = status.or(Some(u64::MAX));
                    if beat.bars.len() == BARS {
                        beat.bars.remove(0);
                    }
                    beat.bars.push(health(status));
                }
            }
            (4, Some("heartbeatList")) if path.key(4) == Some("ping") => {
                if let Scalar::Number {
                    text,
                    complete: true,
                } = value
                {
                    let ping = text.parse().ok().filter(|ms: &f64| ms.is_finite());
                    if let (Some(beat), Some(ping)) = (self.monitor(path.key(2)), ping) {
                        beat.ping = Some(ping);
                    }
                }
            }
            (2, Some("uptimeList")) => {
                let uptime = match value {
                    Scalar::Number {
                        text,
                        complete: true,
                    } => text.parse().ok(),
                    _ => None,
                };
                let id = path.key(2).and_then(|key| key.strip_suffix("_24"));
                if let Some(beat) = self.monitor(id) {
                    beat.uptime = uptime;
                }
            }
            _ => {}
        }
    }
}

async fn heartbeats(
    conn: &Conn<'_>,
    slug: &str,
    ids: impl Iterator<Item = u64>,
) -> Result<Heartbeats, Fault> {
    let mut scanner = Scanner::new(Heartbeats::new(ids));
    let mut sink = |chunk: &[u8]| scanner.feed(chunk).map(|()| true).map_err(drop);
    let path = format!("api/status-page/heartbeat/{slug}");
    success(conn.scan(Step::get(&path), &mut sink).await?)?;
    scanner.finish().map_err(|_| Class::Protocol)?;
    let data = scanner.visitor;
    if data.not_object || !data.lists || !data.uptimes {
        return Err(Class::Protocol.into());
    }
    Ok(data)
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

/// The page's incident, its first monitors with their heartbeats (None when
/// the heartbeat response is too large) and how many monitors were left out.
struct Board {
    incident: Option<Incident>,
    shown: Vec<Monitor>,
    data: Option<Heartbeats>,
    remaining: usize,
}

async fn board(conn: &Conn<'_>) -> Result<Board, Fault> {
    let slug = slug(conn)?;
    let page = page(conn, slug).await?;
    let mut monitors = page
        .public_group_list
        .into_iter()
        .flat_map(|group| group.monitor_list);
    let shown: Vec<Monitor> = monitors.by_ref().take(MAX_MONITORS).collect();
    let data = within_response_limit(
        heartbeats(conn, slug, shown.iter().map(|monitor| monitor.id)).await,
    )?;
    Ok(Board {
        incident: page.incident,
        shown,
        data,
        remaining: monitors.count(),
    })
}

fn monitors<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Board {
            incident,
            shown,
            data,
            remaining,
        } = board(conn).await?;
        let mut rows = Vec::new();
        if let Some(incident) = incident {
            let title = display_text(conn, incident.title);
            rows.push(BodyRow {
                title: format!("Incident: {title}"),
                secondary: Some(display_text(conn, incident.content)),
                status: Some(Health::Warn),
                ..Default::default()
            });
        }
        for monitor in shown {
            let beat = data.as_ref().and_then(|data| data.get(monitor.id));
            let latest = beat.and_then(|beat| beat.latest);
            let (label, health) = match latest {
                Some(0) => ("Down", Health::Down),
                Some(1) => ("Up", Health::Up),
                Some(2) => ("Pending", Health::Warn),
                Some(3) => ("Maintenance", Health::Warn),
                _ if data.is_none() => ("Heartbeat unavailable", Health::Warn),
                _ => ("Unknown", Health::Warn),
            };
            let uptime = beat
                .and_then(|beat| beat.uptime)
                .filter(|uptime| (0.0..=1.0).contains(uptime));
            let uptime = match uptime {
                Some(0.0) if latest.is_none() => None,
                Some(uptime) => Some(format!("{:.1}%", uptime * 100.0)),
                None => None,
            };
            let secondary = match beat.and_then(|beat| beat.ping) {
                Some(ms) if latest == Some(1) => format!("{label} · {} ms", ms.round() as u64),
                _ => label.into(),
            };
            rows.push(BodyRow {
                title: display_text(conn, monitor.name),
                secondary: Some(secondary),
                value: uptime,
                status: Some(health),
                bars: beat.map(|beat| beat.bars.clone()).unwrap_or_default(),
                ..Default::default()
            });
        }
        if remaining > 0 {
            rows.push(BodyRow {
                title: format!("{remaining} more monitors on the status page"),
                ..Default::default()
            });
        }
        if rows.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No monitors on this status page.".into(),
            });
        }
        Ok(StructuredBody::Rows { rows })
    })
}

fn summary<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Board { shown, data, .. } = board(conn).await?;
        if shown.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No monitors on this status page.".into(),
            });
        }
        let Some(data) = data else {
            return Ok(StructuredBody::Unavailable {
                message: "Heartbeat response too large to summarise.".into(),
            });
        };
        let (mut up, mut down, mut other, mut sum, mut counted) = (0u64, 0u64, 0u64, 0.0, 0u32);
        for monitor in &shown {
            let beat = data.get(monitor.id);
            match beat.and_then(|beat| beat.latest) {
                Some(1) => up += 1,
                Some(0) => down += 1,
                _ => other += 1,
            }
            if let Some(uptime) = beat
                .and_then(|beat| beat.uptime)
                .filter(|uptime| (0.0..=1.0).contains(uptime))
            {
                sum += uptime;
                counted += 1;
            }
        }
        let tile = |label: &str, value: String, glyph, tone| StatTile {
            label: label.into(),
            value,
            glyph: Some(glyph),
            delta: None,
            tone,
            series: Vec::new(),
        };
        let mut tiles = vec![
            tile("Up", grouped(up), Glyph::Check, Some(Health::Up)),
            tile(
                "Down",
                grouped(down),
                Glyph::Alert,
                (down > 0).then_some(Health::Down),
            ),
            tile(
                "Pending",
                grouped(other),
                Glyph::Clock,
                (other > 0).then_some(Health::Warn),
            ),
        ];
        if counted > 0 {
            tiles.push(tile(
                "Uptime (24 h)",
                format!("{:.1}%", sum / f64::from(counted) * 100.0),
                Glyph::Activity,
                None,
            ));
        }
        Ok(StructuredBody::Tiles { tiles })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{exercise, Exchange};

    #[tokio::test]
    async fn public_page_and_heartbeat_map_status_and_incident() {
        const PAGE: &str = "GET /prefix/api/status-page/home HTTP/1.1";
        const SMALL: &str = r#"{"heartbeatList":{"1":[{"status":0,"ping":null},{"status":1,"ping":12.4}],"2":[{"status":0}],"3":[{"status":2}],"4":[{"status":3}],"5":[]},"uptimeList":{"1_24":0.995,"2_24":0,"3_24":null,"4_24":1,"5_24":0}}"#;
        const PAGE_BODY: &str = r#"{"config":{"slug":"home"},"incident":{"title":"Service outage","content":"Investigating"},"publicGroupList":[{"monitorList":[{"id":1,"name":"Web"},{"id":2,"name":"DNS"},{"id":3,"name":"Mail"},{"id":4,"name":"Storage"},{"id":5,"name":"New monitor"}]}]}"#;
        // 30 monitors with Kuma's 100 beats each, about 200 KiB. Monitor 3
        // went down on its last beat; monitor 5 has no heartbeat list.
        let beat = |status| {
            format!(
                r#"{{"status":{status},"time":"2026-11-22 12:00:00.000","msg":"200 - OK","ping":23}}"#
            )
        };
        let lists = (1..=30)
            .filter(|id| *id != 5)
            .map(|id| {
                let mut beats = vec![beat(1); 99];
                beats.push(beat(if id == 3 { 0 } else { 1 }));
                format!(r#""{id}":[{}]"#, beats.join(","))
            })
            .collect::<Vec<_>>()
            .join(",");
        let uptimes = (1..=30)
            .map(|id| format!(r#""{id}_24":0.98"#))
            .collect::<Vec<_>>()
            .join(",");
        let large = format!(r#"{{"heartbeatList":{{{lists}}},"uptimeList":{{{uptimes}}}}}"#);
        assert!(large.len() > MAX_RESPONSE);
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
                    body: SMALL,
                },
                Exchange { request: PAGE, status: 200, body: PAGE_BODY },
                Exchange { request: "GET /prefix/api/status-page/heartbeat/home HTTP/1.1", status: 200, body: &large },
                Exchange { request: PAGE, status: 200, body: PAGE_BODY },
                Exchange { request: "GET /prefix/api/status-page/heartbeat/home HTTP/1.1", status: 200, body: r#"{"heartbeatList":{"1":[{"status":1}"# },
                Exchange { request: PAGE, status: 200, body: PAGE_BODY },
                Exchange { request: "GET /prefix/api/status-page/heartbeat/home HTTP/1.1", status: 200, body: SMALL },
                Exchange { request: PAGE, status: 200, body: PAGE_BODY },
                Exchange { request: "GET /prefix/api/status-page/heartbeat/home HTTP/1.1", status: 200, body: &large },
                Exchange { request: PAGE, status: 403, body: "" },
                Exchange { request: PAGE, status: 200, body: "{}" },
            ],
            |_, wire| {
                assert!(!wire.contains("authorization:"));
                assert!(!wire.contains("cookie:"));
                ""
            },
            async |conn| {
                conn.run((UPTIME_KUMA.test)(conn)).await.unwrap();
                let body = super::super::fetch(conn, &UPTIME_KUMA.widgets[0], &Values::new()).await.unwrap();
                assert_eq!(serde_json::to_value(body).unwrap(), serde_json::json!({"kind":"rows","rows":[
                    {"title":"Incident: Service outage","secondary":"Investigating","status":"warn"},
                    {"title":"Web","secondary":"Up · 12 ms","value":"99.5%","status":"up","bars":["down","up"]},
                    {"title":"DNS","secondary":"Down","value":"0.0%","status":"down","bars":["down"]},
                    {"title":"Mail","secondary":"Pending","status":"warn","bars":["warn"]},
                    {"title":"Storage","secondary":"Maintenance","value":"100.0%","status":"warn","bars":["warn"]},
                    {"title":"New monitor","secondary":"Unknown","status":"warn"}
                ]}));
                let large = super::super::fetch(conn, &UPTIME_KUMA.widgets[0], &Values::new()).await.unwrap();
                let large = serde_json::to_value(large).unwrap();
                assert_eq!(large["rows"][1]["secondary"], "Up · 23 ms");
                assert_eq!(large["rows"][1]["value"], "98.0%");
                assert_eq!(large["rows"][1]["bars"].as_array().unwrap().len(), BARS);
                assert_eq!(large["rows"][3]["status"], "down");
                assert_eq!(large["rows"][3]["bars"][BARS - 1], "down");
                assert_eq!(large["rows"][3]["bars"][BARS - 2], "up");
                assert_eq!(large["rows"][5]["status"], "warn");
                assert!(large["rows"][5].get("bars").is_none());
                let truncated = super::super::fetch(conn, &UPTIME_KUMA.widgets[0], &Values::new()).await.unwrap_err();
                assert!(matches!(truncated.class, Class::Protocol));
                let summary = super::super::fetch(conn, &UPTIME_KUMA.widgets[1], &Values::new()).await.unwrap();
                assert_eq!(serde_json::to_value(summary).unwrap(), serde_json::json!({"kind":"tiles","tiles":[
                    {"label":"Up","value":"1","glyph":"check","tone":"up"},
                    {"label":"Down","value":"1","glyph":"alert","tone":"down"},
                    {"label":"Pending","value":"3","glyph":"clock","tone":"warn"},
                    {"label":"Uptime (24 h)","value":"49.9%","glyph":"activity"}
                ]}));
                let summary = super::super::fetch(conn, &UPTIME_KUMA.widgets[1], &Values::new()).await.unwrap();
                assert_eq!(serde_json::to_value(summary).unwrap()["tiles"], serde_json::json!([
                    {"label":"Up","value":"3","glyph":"check","tone":"up"},
                    {"label":"Down","value":"1","glyph":"alert","tone":"down"},
                    {"label":"Pending","value":"1","glyph":"clock","tone":"warn"},
                    {"label":"Uptime (24 h)","value":"98.0%","glyph":"activity"}
                ]));
                for class in [Class::Auth, Class::Protocol] {
                    let fault = conn.run((UPTIME_KUMA.test)(conn)).await.unwrap_err();
                    assert_eq!(fault.class.message(), class.message());
                }
            },
        ).await;
    }
}
