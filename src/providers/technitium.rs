use super::*;
use widgets::StructuredBody;

const TOKEN: &str = "token";
const ZONES: &str = "api/zones/list";
const STATS: &str = "api/dashboard/stats/get";

pub const TECHNITIUM: Provider = Provider {
    id: "technitium",
    name: "Technitium DNS",
    category: "DNS",
    icon: "technitium",
    color: "#4a7fe8",
    description: "Show DNS query and block counts and the number of hosted zones.",
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
    widgets: &[Widget {
        id: "dns",
        name: "DNS activity",
        description: "Queries and blocks in the past hour, and the number of visible zones.",
        recommended: true,
        default_size: size(4, 2),
        min_size: size(3, 2),
        sample: r#"{"kind":"stat_grid","rows":[["Queries","18,240"],["Blocked","2,931"],["Cached","9,402"],["Clients","17"]]}"#,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
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
struct Dashboard {
    stats: Stats,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stats {
    total_queries: u64,
    total_blocked: u64,
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
        Ok(StructuredBody::StatGrid {
            rows: vec![
                (
                    "Queries (last hour)".into(),
                    dashboard.stats.total_queries.to_string(),
                ),
                (
                    "Blocked (last hour)".into(),
                    dashboard.stats.total_blocked.to_string(),
                ),
                ("Zones".into(), zones.total_zones.to_string()),
            ],
        })
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
        exercise(&TECHNITIUM, &[(TOKEN, "fake+token")], Values::new(), &[
            Exchange { request: LIST, status: 200, body: ZONE_BODY },
            Exchange { request: LIST, status: 200, body: ZONE_BODY },
            Exchange { request: "POST /prefix/api/dashboard/stats/get HTTP/1.1", status: 200, body: r#"{"status":"ok","response":{"stats":{"totalQueries":100,"totalBlocked":25}}}"# },
            Exchange { request: LIST, status: 200, body: r#"{"status":"invalid-token"}"# },
            Exchange { request: LIST, status: 200, body: r#"{"status":"ok","response":{"zones":[]}}"# },
         ], |index, wire| {
            let (headers, body) = wire.split_once("\r\n\r\n").unwrap();
            assert!(!headers.contains("fake"));
            assert!(headers.contains("content-type: application/x-www-form-urlencoded"));
            assert!(body == if index == 2 {
                "token=fake%2Btoken"
            } else {
                "token=fake%2Btoken&pageNumber=1&zonesPerPage=1"
            });
            ""
        }, async |conn| {
            conn.run(test(conn)).await.unwrap();
            let body = super::super::fetch(conn, &TECHNITIUM.widgets[0], &Values::new()).await.unwrap();
            assert_eq!(serde_json::to_value(body).unwrap()["rows"], json!([
                ["Queries (last hour)", "100"], ["Blocked (last hour)", "25"], ["Zones", "12"],
            ]));
            assert_test_fault(conn, &TECHNITIUM, Class::Auth).await;
            assert_test_fault(conn, &TECHNITIUM, Class::Protocol).await;
        }).await;
    }
}
