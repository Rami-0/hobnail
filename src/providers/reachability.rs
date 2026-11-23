use super::*;
use std::net::IpAddr;
use widgets::{Health, StructuredBody};

pub(super) const REACHABILITY: Provider = Provider {
    id: "reachability",
    name: "Reachability",
    category: "Network",
    icon: "network",
    color: "#0ea5e9",
    description: "Check an endpoint with TCP, HTTP or DNS. TCP connect replaces ICMP ping.",
    docs: "https://www.rfc-editor.org/rfc/rfc9293.html",
    public_internet: false,
    contacts: "The configured hostname and port, or the system resolver for DNS. Public destinations require internet access.",
    fields: &[
        Field {
            label: "Target URL",
            help: "Use http://host:port for TCP, an HTTP(S) endpoint for HTTP, or http://host for DNS.",
            ..SERVICE_URL
        },
        Field {
            default: Some("TCP"),
            choices: &["TCP", "HTTP", "DNS"],
            ..Field::new("probe", "Check type", Kind::Select, "")
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "status",
        name: "Reachability status",
        description: "Reports TCP connect time, HTTP status, or DNS addresses. TCP connect replaces ICMP ping.",
        recommended: true,
        default_size: size(3, 2),
        min_size: size(2, 2),
        sample: r#"{"kind":"status_list","rows":[["example.com:443","up"]]}"#,
        fields: &[
            Field { min: Some(100), max: Some(599), default: Some("200"), ..Field::new("status_min", "Lowest expected HTTP status", Kind::Number, "Used for HTTP checks.") },
            Field { min: Some(100), max: Some(599), default: Some("299"), ..Field::new("status_max", "Highest expected HTTP status", Kind::Number, "Used for HTTP checks.") },
            Field { max_length: Some(45), ..Field::new("address", "Expected DNS address", Kind::Text, "Optional IPv4 or IPv6 address, used for DNS checks.") },
            REFRESH,
        ],
        fetch,
        validate,
    }],
};

fn statuses(values: &Values) -> (u64, u64) {
    (
        values
            .get("status_min")
            .and_then(Value::as_u64)
            .unwrap_or(200),
        values
            .get("status_max")
            .and_then(Value::as_u64)
            .unwrap_or(299),
    )
}

fn expected_ip(values: &Values) -> Result<Option<IpAddr>, &'static str> {
    values
        .get("address")
        .map(|value| value.as_str().and_then(|s| s.parse().ok()).ok_or("address"))
        .transpose()
}

fn validate(values: &Values) -> Result<(), &'static str> {
    let (min, max) = statuses(values);
    if min > max {
        return Err("status_max");
    }
    expected_ip(values).map(|_| ())
}

async fn measure(conn: &Conn<'_>, values: &Values) -> Result<(String, Health), Fault> {
    let (label, up) = match conn
        .options()
        .get("probe")
        .and_then(Value::as_str)
        .unwrap_or("TCP")
    {
        "TCP" => {
            let result = conn.tcp_probe().await?;
            (
                format!(
                    "TCP {} · {} ms",
                    if result.reachable {
                        "connected"
                    } else {
                        "unreachable"
                    },
                    result.latency.as_millis()
                ),
                result.reachable,
            )
        }
        "HTTP" => {
            let (min, max) = statuses(values);
            let result = conn.http_status().await?;
            (
                format!(
                    "HTTP {} · {} ms (expected {min}–{max})",
                    result.status,
                    result.latency.as_millis()
                ),
                (min..=max).contains(&u64::from(result.status)),
            )
        }
        "DNS" => {
            let addresses = conn.dns_lookup().await?;
            let expected = expected_ip(values).map_err(|_| Class::Protocol)?;
            let answer = addresses
                .iter()
                .take(4)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let suffix = if addresses.len() > 4 { ", …" } else { "" };
            let expectation = expected.map_or(String::new(), |ip| format!(" (expected {ip})"));
            (
                format!("DNS {answer}{suffix}{expectation}"),
                !addresses.is_empty() && expected.is_none_or(|ip| addresses.contains(&ip)),
            )
        }
        _ => return Err(Class::Protocol.into()),
    };
    Ok((label, if up { Health::Up } else { Health::Down }))
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        if matches!(measure(conn, &Values::new()).await?.1, Health::Down)
            && conn
                .options()
                .get("probe")
                .and_then(Value::as_str)
                .unwrap_or("TCP")
                == "TCP"
        {
            return Err(Class::Connect.into());
        }
        Ok(())
    })
}

fn fetch<'a>(conn: &'a Conn<'_>, values: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let row = match measure(conn, values).await {
            Err(fault) if matches!(fault.class, Class::Dns | Class::Connect | Class::Tls) => {
                (fault.class.message().into(), Health::Down)
            }
            result => result?,
        };
        Ok(StructuredBody::StatusList { rows: vec![row] })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn reports_http_status_outside_the_expected_range() {
        exercise(
            &REACHABILITY,
            &[],
            Values::from_iter([("probe".into(), json!("HTTP"))]),
            &[
                Exchange {
                    request: "GET /prefix HTTP/1.1",
                    status: 302,
                    body: "",
                },
                Exchange {
                    request: "GET /prefix HTTP/1.1",
                    status: 302,
                    body: "",
                },
            ],
            |_, _| "Location: http://127.0.0.1/unused\r\n",
            async |conn| {
                let body = super::super::fetch(conn, &REACHABILITY.widgets[0], &Values::new())
                    .await
                    .unwrap();
                assert_eq!(serde_json::to_value(body).unwrap()["rows"][0][1], "down");
                let values = Values::from_iter([("status_max".into(), json!(399))]);
                let body = super::super::fetch(conn, &REACHABILITY.widgets[0], &values)
                    .await
                    .unwrap();
                assert_eq!(serde_json::to_value(body).unwrap()["rows"][0][1], "up");
            },
        )
        .await;
    }

    #[tokio::test]
    async fn dns_match_checks_the_expected_address() {
        exercise(
            &REACHABILITY,
            &[],
            Values::from_iter([("probe".into(), json!("DNS"))]),
            &[],
            |_, _| "",
            async |conn| {
                for (ip, health) in [("127.0.0.1", "up"), ("::1", "down")] {
                    let values = Values::from_iter([("address".into(), json!(ip))]);
                    let body = super::super::fetch(conn, &REACHABILITY.widgets[0], &values)
                        .await
                        .unwrap();
                    assert_eq!(serde_json::to_value(body).unwrap()["rows"][0][1], health);
                }
                assert_eq!(
                    validate(&Values::from_iter([("address".into(), json!("localhost"))])),
                    Err("address")
                );
            },
        )
        .await;
    }
}
