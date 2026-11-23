use super::*;
use widgets::StructuredBody;

const IDENTITY: &str = "identity";
const SECRET: &str = "secret";

pub const NGINX_PROXY_MANAGER: Provider = Provider {
    id: "nginx_proxy_manager",
    name: "Nginx Proxy Manager",
    category: "Network",
    icon: "nginx-proxy-manager",
    color: "#f15933",
    description: "Show the number of visible proxy hosts and certificate expiration dates.",
    docs:
        "https://github.com/NginxProxyManager/nginx-proxy-manager/tree/v2.12.3/backend/schema/paths",
    public_internet: false,
    contacts: "Your configured Nginx Proxy Manager admin UI.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(
                IDENTITY,
                "Account email",
                Kind::Secret,
                "Use an account with view permission for proxy hosts and certificates.",
            )
        },
        Field {
            required: true,
            max_length: Some(4096),
            ..Field::new(SECRET, "Account password", Kind::Secret, "")
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "proxy",
        name: "Proxy hosts and certificates",
        description: "Proxy host total and expiration dates for up to ten visible certificates.",
        recommended: true,
        default_size: size(4, 2),
        min_size: size(3, 2),
        sample: r#"{"kind":"stat_grid","rows":[["Proxy hosts","14"],["Enabled","13"],["Certificates","9"],["Expiring soon","1"]]}"#,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
struct Token {
    token: String,
}

#[derive(Deserialize)]
struct User {
    #[serde(rename = "id")]
    _id: u64,
}

#[derive(Deserialize)]
struct Certificate {
    nice_name: String,
    expires_on: String,
}

async fn login(conn: &Conn<'_>) -> Result<Option<Session>, Fault> {
    let token: Token = parse_json(
        conn.request(Step {
            verb: Verb::Post,
            body: Some(Body::Json(
                Values::new(),
                &[
                    (IDENTITY, Input::Credential(IDENTITY)),
                    (SECRET, Input::Credential(SECRET)),
                ],
            )),
            ..Step::get("api/tokens")
        })
        .await?,
    )?;
    Ok(Some(conn.session(&token.token)?))
}

async fn get<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    session: &Session,
    path: &str,
) -> Result<T, Fault> {
    parse_json(
        conn.request(Step {
            auth: &[Auth::Bearer(Input::Session(session))],
            ..Step::get(path)
        })
        .await?,
    )
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        conn.retry_session(
            || login(conn),
            |session| async move {
                let session = session.ok_or(Class::Auth)?;
                get::<User>(conn, &session, "api/users/me")
                    .await
                    .map(|_| ())
            },
        )
        .await
    })
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        conn.retry_session(
            || login(conn),
            |session| async move {
                let session = session.ok_or(Class::Auth)?;
                let hosts = within_response_limit(
                    get::<Vec<serde::de::IgnoredAny>>(conn, &session, "api/nginx/proxy-hosts")
                        .await,
                )?;
                let certificates = within_response_limit(
                    get::<Vec<Certificate>>(conn, &session, "api/nginx/certificates").await,
                )?;
                let mut rows = vec![(
                    "Proxy hosts".into(),
                    hosts.map_or_else(|| "Unavailable".into(), |hosts| hosts.len().to_string()),
                )];
                match certificates {
                    Some(mut certificates) => {
                        rows.push(("Certificates".into(), certificates.len().to_string()));
                        certificates.sort_by(|a, b| a.expires_on.cmp(&b.expires_on));
                        rows.extend(certificates.into_iter().take(10).map(|certificate| {
                            (
                                display_text(conn, certificate.nice_name),
                                display_text(conn, certificate.expires_on),
                            )
                        }));
                    }
                    None => rows.push(("Certificates".into(), "Unavailable".into())),
                }
                Ok(StructuredBody::StatGrid { rows })
            },
        )
        .await
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn renews_rejected_bearer_and_preserves_certificate_data_after_large_host_list() {
        const LOGIN: &str = "POST /prefix/api/tokens HTTP/1.1";
        const WHO: &str = "GET /prefix/api/users/me HTTP/1.1";
        const HOSTS: &str = "GET /prefix/api/nginx/proxy-hosts HTTP/1.1";
        let oversized = format!(r#"[{{"padding":"{}"}}]"#, "x".repeat(MAX_RESPONSE));
        exercise(
            &NGINX_PROXY_MANAGER,
            &[
                (IDENTITY, "fake@example.invalid"),
                (SECRET, "fake-npm-pass"),
            ],
            Values::new(),
            &[
                Exchange {
                    request: LOGIN,
                    status: 200,
                    body: r#"{"token":"fake-first-token"}"#,
                },
                Exchange {
                    request: WHO,
                    status: 200,
                    body: r#"{"id":3}"#,
                },
                Exchange {
                    request: HOSTS,
                    status: 401,
                    body: "",
                },
                Exchange {
                    request: LOGIN,
                    status: 200,
                    body: r#"{"token":"fake-next-token"}"#,
                },
                Exchange {
                    request: HOSTS,
                    status: 200,
                    body: &oversized,
                },
                Exchange {
                    request: "GET /prefix/api/nginx/certificates HTTP/1.1",
                    status: 200,
                    body: r#"[{"nice_name":"lab","expires_on":"2027-01-01T00:00:00Z"}]"#,
                },
                Exchange {
                    request: WHO,
                    status: 200,
                    body: "{}",
                },
                Exchange {
                    request: WHO,
                    status: 401,
                    body: "",
                },
                Exchange {
                    request: LOGIN,
                    status: 401,
                    body: "",
                },
            ],
            |index, wire| {
                let (head, body) = wire.split_once("\r\n\r\n").unwrap();
                assert!(!head.contains("fake@example.invalid") && !head.contains("fake-npm-pass"));
                if [0, 3, 8].contains(&index) {
                    assert!(!head.contains("authorization:"));
                    assert_eq!(
                        serde_json::from_str::<Value>(body).unwrap(),
                        json!({"identity":"fake@example.invalid","secret":"fake-npm-pass"})
                    );
                } else {
                    let token = if index < 3 {
                        "fake-first-token"
                    } else {
                        "fake-next-token"
                    };
                    assert_header(wire, &format!("authorization: Bearer {token}"), token);
                    assert!(body.is_empty());
                }
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let body =
                    super::super::fetch(conn, &NGINX_PROXY_MANAGER.widgets[0], &Values::new())
                        .await
                        .unwrap();
                assert_eq!(
                    serde_json::to_value(body).unwrap()["rows"],
                    json!([
                        ["Proxy hosts", "Unavailable"],
                        ["Certificates", "1"],
                        ["lab", "2027-01-01T00:00:00Z"],
                    ])
                );
                assert_test_fault(conn, &NGINX_PROXY_MANAGER, Class::Protocol).await;
                assert_test_fault(conn, &NGINX_PROXY_MANAGER, Class::Auth).await;
            },
        )
        .await;
    }
}
