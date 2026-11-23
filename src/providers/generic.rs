use super::*;
use crate::widgets::{shorten, template, Path, StructuredBody};

pub const HTTP: Provider = Provider {
    id: "http",
    name: "Generic HTTP",
    category: "Utilities",
    icon: "http",
    color: "#64748b",
    description: "Display values from a JSON endpoint you configure.",
    docs: "https://github.com/Rami-0/hobnail/blob/main/docs/features/integrations.md",
    public_internet: false,
    contacts: "The hostname and port you enter. Public addresses require internet access.",
    fields: &[
        Field {
            required: true,
            max_length: Some(2048),
            ..Field::new(
                "base_url",
                "Service URL",
                Kind::Url,
                "Enter the JSON endpoint reachable from Hobnail.",
            )
        },
        Field {
            max_length: Some(4096),
            ..Field::new(
                "secret",
                "Bearer credential",
                Kind::Secret,
                "Bound to the scheme, hostname, and port.",
            )
        },
        Field {
            default: Some("true"),
            ..Field::new("enabled", "Enabled", Kind::Bool, "")
        },
    ],
    test,
    widgets: &[Widget {
        id: "json",
        name: "JSON values",
        description: "Show selected fields from the response.",
        recommended: true,
        default_size: size(4, 2),
        min_size: size(2, 2),
        sample: r#"{"kind":"stat_grid","rows":[["Status","healthy"],["Version","2.4.1"],["Uptime","6 days"]]}"#,
        fields: &[
            Field {
                max_length: Some(160),
                ..Field::new(
                    "paths",
                    "JSON paths",
                    Kind::List,
                    "One per line, for example stats.users. Credentials require explicit paths.",
                )
            },
            REFRESH,
        ],
        validate: options,
        fetch,
    }, Widget {
        id: "status",
        name: "HTTP status",
        description: "Show whether the service returns a successful HTTP status.",
        recommended: false,
        default_size: size(3, 2),
        min_size: size(2, 2),
        sample: r#"{"kind":"stat","value":"200 OK"}"#,
        fields: &[REFRESH],
        validate: |_| Ok(()),
        fetch: status,
    }],
};

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async { response(conn, true).await.map(drop) })
}

fn status<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async {
        let response = response(conn, true).await?;
        Ok(StructuredBody::Stat {
            value: format!("HTTP {}", response.status),
        })
    })
}

async fn response(conn: &Conn<'_>, status_only: bool) -> Result<client::Response, Fault> {
    let mut step = Step::get("");
    step.status_only = status_only;
    let auth = [Auth::Bearer(Input::Credential("secret"))];
    if conn.has_credential("secret") {
        step.auth = &auth;
    }
    success(conn.request(step).await?)
}

fn options(values: &Values) -> Result<(), &'static str> {
    if values
        .get("paths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|path| Path::parse(path.as_str().expect("validated paths")).is_err())
    {
        return Err("paths");
    }
    Ok(())
}

pub(super) fn explicit_paths(options: &Values) -> bool {
    options
        .get("paths")
        .and_then(Value::as_array)
        .is_some_and(|paths| {
            !paths.is_empty()
                && paths.iter().all(|path| {
                    path.as_str()
                        .and_then(|path| Path::parse(path).ok())
                        .is_some_and(|path| !path.0.is_empty())
                })
        })
}

fn fetch<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async {
        if conn.has_credentials() && !explicit_paths(options) {
            return Ok(StructuredBody::Unavailable {
                message: "Choose specific JSON paths for this credentialed connection.".into(),
            });
        }
        let response = response(conn, false).await?;
        let json = serde_json::from_slice(&response.body).map_err(|_| Class::Protocol)?;
        let redact = conn.output_redactor();
        let rows: Vec<_> = options
            .get("paths")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|path| {
                let mut value = Path::parse(path).ok()?.eval(&json);
                // Scrub before serializing nested values or shortening their display.
                redact.json(&mut value);
                Some((path.to_owned(), template::display(&value)))
            })
            .collect();
        Ok(if rows.is_empty() {
            StructuredBody::Stat {
                value: shorten(&json.to_string(), 512),
            }
        } else {
            StructuredBody::StatGrid { rows }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::connection::tests::{receive, respond};
    use std::sync::Arc;
    use tokio::{net::TcpListener, sync::Semaphore};

    #[tokio::test]
    async fn connection_tests_and_status_widgets_accept_non_json_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = Target::new(
            format!("http://{}", listener.local_addr().unwrap()),
            Credentials::new(),
            false,
        );
        let fake = tokio::spawn(async move {
            for (code, body) in [(204, ""), (200, "plain text")] {
                for _ in 0..3 {
                    let (mut socket, _) = receive(&listener).await;
                    respond(&mut socket, code, "", body).await;
                }
            }
        });
        let policy = crate::config::HttpPolicy {
            allow_hosts: vec!["127.0.0.1".into()],
        };
        let connections = Connections::default();
        let permit = Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap();
        let conn = Conn::new(&target, &policy, &connections, permit).unwrap();
        let values = Values::new();
        for code in [204, 200] {
            conn.run(test(&conn)).await.unwrap();
            let result = super::super::fetch(&conn, &HTTP.widgets[1], &values)
                .await
                .unwrap();
            assert!(
                matches!(result, StructuredBody::Stat { value } if value == format!("HTTP {code}"))
            );
            let error = super::super::fetch(&conn, &HTTP.widgets[0], &values)
                .await
                .unwrap_err();
            assert!(matches!(error.class, Class::Protocol));
        }
        fake.await.unwrap();
    }

    #[tokio::test]
    async fn encoded_echoes_are_removed_before_display_shortening() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        const SECRET: &str = "xy~*>&/!";
        let target = Target::new(
            origin.clone(),
            [(
                "secret".into(),
                client::BoundCredential {
                    origin,
                    token: SECRET.into(),
                },
            )]
            .into_iter()
            .collect(),
            false,
        );
        let fake = tokio::spawn(async move {
            for echo in super::super::redaction::tests::encoded_echoes(SECRET) {
                let (mut socket, _) = receive(&listener).await;
                let value = format!("{}{}", "x".repeat(template::MAX_TEXT - 4), echo);
                respond(
                    &mut socket,
                    200,
                    "",
                    &serde_json::json!({"value":value}).to_string(),
                )
                .await;
            }
        });
        let policy = crate::config::HttpPolicy {
            allow_hosts: vec!["127.0.0.1".into()],
        };
        let connections = Connections::default();
        let permit = Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap();
        let conn = Conn::new(&target, &policy, &connections, permit).unwrap();
        let options = serde_json::from_str(r#"{"paths":["value"]}"#).unwrap();
        for _ in super::super::redaction::tests::encoded_echoes(SECRET) {
            let StructuredBody::StatGrid { rows } =
                super::super::fetch(&conn, &HTTP.widgets[0], &options)
                    .await
                    .unwrap()
            else {
                panic!("unexpected widget shape");
            };
            assert_eq!(
                rows[0].1,
                template::display(
                    &format!("{}<redacted>", "x".repeat(template::MAX_TEXT - 4)).into()
                )
            );
        }
        fake.await.unwrap();
    }
}
