use super::pihole::grouped;
use super::*;
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

const TOKEN: &str = "token";
const ZONES: &str = "api/zones/list";
const STATS: &str = "api/dashboard/stats/get";

pub const TECHNITIUM: Provider = Provider {
    id: "technitium",
    name: "Technitium DNS",
    category: "DNS",
    icon: "technitium",
    color: "#4a7fe8",
    detect: super::detect(&["technitium"], 5380),
    description: "Show DNS queries, blocks, cache hits and clients for the past hour, and the top domains and clients.",
    docs: "https://github.com/TechnitiumSoftware/DnsServer/blob/v14.3.0/APIDOCS.md",
    public_internet: false,
    contacts: "Your configured Technitium DNS web console.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(
                TOKEN,
                "API token",
                Kind::Secret,
                "Grant Dashboard View and Zones View.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "dns",
            name: "DNS activity",
            description: "Queries, blocked and cached answers and clients in the past hour with a per-minute query trend, and the number of zones.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Queries (1 h)","value":"1,824","glyph":"activity","series":[22,31,18,27,40,35,29,24,30,41,52,38,33,29,26,31,44,39,35,28,24,30,36,41]},{"label":"Blocked","value":"293","glyph":"shield"},{"label":"Cached","value":"940","glyph":"database"},{"label":"Clients","value":"17","glyph":"users"},{"label":"Zones","value":"12","glyph":"globe"}]}"#,
            fields: &[REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "top_domains",
            name: "Top domains",
            description: "The ten most queried domains in the past hour.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"api.example.com","value":"114","progress":100},{"title":"cdn.example.net","value":"61","progress":53},{"title":"time.example.org","value":"20","progress":17}]}"#,
            fields: &[REFRESH],
            fetch: top_domains,
            validate: no_widget_options,
        },
        Widget {
            id: "top_blocked",
            name: "Top blocked domains",
            description: "The ten most blocked domains in the past hour.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"ads.example.net","value":"48","progress":100,"status":"down"},{"title":"tracker.example.com","value":"12","progress":25,"status":"down"}]}"#,
            fields: &[REFRESH],
            fetch: top_blocked,
            validate: no_widget_options,
        },
        Widget {
            id: "top_clients",
            name: "Top clients",
            description: "The ten clients that sent the most queries in the past hour, by host name and address.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"server1.example.lan","secondary":"192.0.2.5","value":"463","progress":100},{"title":"laptop.example.lan","secondary":"192.0.2.12","value":"236","progress":50},{"title":"192.0.2.13","value":"165","progress":35}]}"#,
            fields: &[REFRESH],
            fetch: top_clients,
            validate: no_widget_options,
        },
    ],
};

#[derive(Deserialize)]
#[serde(tag = "status")]
enum Envelope<T> {
    #[serde(rename = "ok")]
    Ok { response: T },
    #[serde(rename = "invalid-token")]
    InvalidToken,
    #[serde(rename = "error")]
    Error {
        #[serde(rename = "errorMessage")]
        message: String,
    },
}

async fn post<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    path: &str,
    params: &[(&str, &str)],
) -> Result<T, Fault> {
    let mut form = vec![(TOKEN, Input::Credential(TOKEN))];
    form.extend(params.iter().map(|&(key, value)| (key, Input::Text(value))));
    match parse_json(
        conn.request(Step {
            verb: Verb::Post,
            body: Some(Body::Form(&form)),
            ..Step::get(path)
        })
        .await?,
    )? {
        Envelope::Ok { response } => Ok(response),
        Envelope::InvalidToken => Err(Class::Auth.into()),
        Envelope::Error { message } if message.starts_with("Access was denied") => {
            Err(Class::Auth.into())
        }
        Envelope::Error { .. } => Err(Class::Protocol.into()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Dashboard {
    stats: Stats,
    #[serde(default)]
    main_chart_data: Option<Chart>,
    #[serde(default)]
    top_clients: Vec<Top>,
    #[serde(default)]
    top_domains: Vec<Top>,
    #[serde(default)]
    top_blocked_domains: Vec<Top>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stats {
    total_queries: u64,
    total_blocked: u64,
    #[serde(default)]
    total_cached: Option<u64>,
    #[serde(default)]
    total_clients: Option<u64>,
}

#[derive(Deserialize)]
struct Chart {
    datasets: Vec<Dataset>,
}

#[derive(Deserialize)]
struct Dataset {
    label: String,
    data: Vec<f64>,
}

/// A top list entry: a domain, or a client address with its host name.
#[derive(Deserialize)]
struct Top {
    name: String,
    #[serde(default)]
    domain: Option<String>,
    hits: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Zones {
    total_zones: u64,
}

async fn zones(conn: &Conn<'_>) -> Result<Zones, Fault> {
    post(conn, ZONES, &[("pageNumber", "1"), ("zonesPerPage", "1")]).await
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { zones(conn).await.map(|_| ()) })
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let zones = zones(conn).await?;
        let dashboard: Dashboard = post(conn, STATS, &[]).await?;
        let stats = dashboard.stats;
        let series = dashboard
            .main_chart_data
            .and_then(|chart| chart.datasets.into_iter().find(|set| set.label == "Total"))
            .map(|set| set.data.into_iter().map(|n| n as f32).collect())
            .unwrap_or_default();
        let tile = |label: &str, value: u64, glyph| StatTile {
            label: label.into(),
            value: grouped(value),
            glyph: Some(glyph),
            delta: None,
            tone: None,
            series: Vec::new(),
        };
        let mut tiles = vec![
            StatTile {
                series,
                ..tile("Queries (1 h)", stats.total_queries, Glyph::Activity)
            },
            tile("Blocked", stats.total_blocked, Glyph::Shield),
        ];
        tiles.extend(
            stats
                .total_cached
                .map(|n| tile("Cached", n, Glyph::Database)),
        );
        tiles.extend(
            stats
                .total_clients
                .map(|n| tile("Clients", n, Glyph::Users)),
        );
        tiles.push(tile("Zones", zones.total_zones, Glyph::Globe));
        Ok(StructuredBody::Tiles { tiles })
    })
}

#[derive(Clone, Copy)]
enum List {
    Domains,
    Blocked,
    Clients,
}

fn top_domains<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(top(conn, List::Domains))
}

fn top_blocked<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(top(conn, List::Blocked))
}

fn top_clients<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(top(conn, List::Clients))
}

async fn top(conn: &Conn<'_>, list: List) -> Result<StructuredBody, Fault> {
    let dashboard: Dashboard = post(conn, STATS, &[]).await?;
    let entries = match list {
        List::Domains => dashboard.top_domains,
        List::Blocked => dashboard.top_blocked_domains,
        List::Clients => dashboard.top_clients,
    };
    if entries.is_empty() {
        return Ok(StructuredBody::Unavailable {
            message: "No queries in the past hour.".into(),
        });
    }
    let max = entries.iter().map(|e| e.hits).max().unwrap_or(0).max(1);
    Ok(StructuredBody::Rows {
        rows: entries
            .into_iter()
            .take(10)
            .map(|entry| {
                let (title, secondary) = match entry.domain.filter(|d| !d.is_empty()) {
                    Some(host) => (host, Some(entry.name)),
                    None => (entry.name, None),
                };
                BodyRow {
                    title: display_text(conn, title),
                    secondary: secondary.map(|text| display_text(conn, text)),
                    value: Some(grouped(entry.hits)),
                    progress: Some((entry.hits.saturating_mul(100) / max).min(100) as u8),
                    status: matches!(list, List::Blocked).then_some(Health::Down),
                    ..Default::default()
                }
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn reads_zone_total_and_maps_api_token_errors() {
        const LIST: &str = "POST /prefix/api/zones/list HTTP/1.1";
        const ZONE_BODY: &str =
            r#"{"status":"ok","response":{"totalZones":12,"zones":[{"name":"example.invalid"}]}}"#;
        // Shapes from APIDOCS.md "Get Stats", trimmed.
        const FULL: &str = r#"{"status":"ok","response":{"stats":{"totalQueries":100,"totalNoError":90,"totalCached":40,"totalBlocked":25,"totalClients":6,"zones":19},"mainChartData":{"labelFormat":"HH:mm","labels":["2024-02-04T10:38:00.0000000Z","2024-02-04T10:39:00.0000000Z"],"datasets":[{"label":"Total","data":[13,27]},{"label":"Blocked","data":[1,2]}]},"topClients":[{"name":"192.168.10.5","domain":"server1.home","hits":463,"rateLimited":false},{"name":"192.168.10.13","hits":165,"rateLimited":false}],"topDomains":[{"name":"example.com","hits":114}],"topBlockedDomains":[{"name":"ads.example","hits":20},{"name":"t.example","hits":5}]}}"#;
        exercise(&TECHNITIUM, &[(TOKEN, "fake+token")], Values::new(), &[
            Exchange { request: LIST, status: 200, body: ZONE_BODY },
            Exchange { request: LIST, status: 200, body: ZONE_BODY },
            Exchange { request: "POST /prefix/api/dashboard/stats/get HTTP/1.1", status: 200, body: FULL },
            Exchange { request: "POST /prefix/api/dashboard/stats/get HTTP/1.1", status: 200, body: FULL },
            Exchange { request: "POST /prefix/api/dashboard/stats/get HTTP/1.1", status: 200, body: FULL },
            Exchange { request: "POST /prefix/api/dashboard/stats/get HTTP/1.1", status: 200, body: FULL },
            Exchange { request: "POST /prefix/api/dashboard/stats/get HTTP/1.1", status: 200, body: r#"{"status":"ok","response":{"stats":{"totalQueries":100,"totalBlocked":25}}}"# },
            Exchange { request: "POST /prefix/api/dashboard/stats/get HTTP/1.1", status: 200, body: r#"{"status":"error","errorMessage":"Access was denied."}"# },
            Exchange { request: LIST, status: 200, body: r#"{"status":"invalid-token"}"# },
            Exchange { request: LIST, status: 200, body: r#"{"status":"ok","response":{"zones":[]}}"# },
         ], |index, wire| {
            let (headers, body) = wire.split_once("\r\n\r\n").unwrap();
            assert!(!headers.contains("fake"));
            assert!(headers.contains("content-type: application/x-www-form-urlencoded"));
            assert!(body == if (2..=7).contains(&index) {
                "token=fake%2Btoken"
            } else {
                "token=fake%2Btoken&pageNumber=1&zonesPerPage=1"
            });
            ""
        }, async |conn| {
            conn.run(test(conn)).await.unwrap();
            let body = super::super::fetch(conn, &TECHNITIUM.widgets[0], &Values::new()).await.unwrap();
            assert_eq!(serde_json::to_value(body).unwrap()["tiles"], json!([
                {"label":"Queries (1 h)","value":"100","glyph":"activity","series":[13.0,27.0]},
                {"label":"Blocked","value":"25","glyph":"shield"},
                {"label":"Cached","value":"40","glyph":"database"},
                {"label":"Clients","value":"6","glyph":"users"},
                {"label":"Zones","value":"12","glyph":"globe"},
            ]));
            let empty = Values::new();
            let fetch = |index: usize| super::super::fetch(conn, &TECHNITIUM.widgets[index], &empty);
            assert_eq!(serde_json::to_value(fetch(1).await.unwrap()).unwrap()["rows"], json!([
                {"title":"example.com","value":"114","progress":100},
            ]));
            assert_eq!(serde_json::to_value(fetch(2).await.unwrap()).unwrap()["rows"][1], json!(
                {"title":"t.example","value":"5","progress":25,"status":"down"}
            ));
            assert_eq!(serde_json::to_value(fetch(3).await.unwrap()).unwrap()["rows"], json!([
                {"title":"server1.home","secondary":"192.168.10.5","value":"463","progress":100},
                {"title":"192.168.10.13","value":"165","progress":35},
            ]));
            assert_eq!(serde_json::to_value(fetch(3).await.unwrap()).unwrap()["message"], "No queries in the past hour.");
            assert_eq!(fetch(1).await.unwrap_err().class.message(), Class::Auth.message());
            assert_test_fault(conn, &TECHNITIUM, Class::Auth).await;
            assert_test_fault(conn, &TECHNITIUM, Class::Protocol).await;
        }).await;
    }
}
