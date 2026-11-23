use super::*;
use crate::{
    logs::frame::parse_rfc3339_parts,
    util::{fmt_utc, human_bytes},
};
use widgets::{BodyItem, StructuredBody};

const PASSWORD: &str = "password";
const BACKUPS: &str = "api/v1/backups";

pub const DUPLICATI: Provider = Provider {
    id: "duplicati",
    name: "Duplicati",
    category: "Backups",
    icon: "duplicati",
    color: "#336697",
    detect: super::detect(&["duplicati"], 8200),
    description: "Show recent backup completion times and source sizes.",
    docs: "https://docs.duplicati.com/",
    public_internet: false,
    contacts: "Your configured Duplicati server.",
    fields: &[
        SERVICE_URL,
        Field {
            label: "Web interface password",
            help: "Use the password for the Duplicati web interface.",
            required: true,
            ..OPTIONAL_BASIC_PASSWORD
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "backups",
        name: "Recent backups",
        description: "Last recorded completion and source size for up to ten backup jobs.",
        recommended: true,
        default_size: size(4, 3),
        min_size: size(3, 2),
        sample: r#"{"kind":"item_list","items":[{"title":"Documents","secondary":"Last run succeeded · 2 h ago","link":null,"time":null},{"title":"Photos","secondary":"Last run succeeded · yesterday","link":null,"time":null}]}"#,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Login {
    access_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ServerState {
    #[serde(rename = "LastEventID")]
    last_event_id: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JobEntry {
    backup: Backup,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Backup {
    name: String,
    metadata: Option<Metadata>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Metadata {
    last_backup_finished: Option<String>,
    source_files_size: Option<String>,
}

async fn login(conn: &Conn<'_>) -> Result<Option<Session>, Fault> {
    let login: Login = parse_json(
        conn.request(Step {
            verb: Verb::Post,
            body: Some(Body::Json(
                Values::from_iter([("RememberMe".into(), Value::Bool(false))]),
                &[("Password", Input::Credential(PASSWORD))],
            )),
            ..Step::get("api/v1/auth/login")
        })
        .await?,
    )?;
    Ok(Some(conn.session(&login.access_token)?))
}

async fn get<T: serde::de::DeserializeOwned>(conn: &Conn<'_>, path: &str) -> Result<T, Fault> {
    conn.retry_session(
        || login(conn),
        |session| async move {
            let session = session.ok_or(Class::Auth)?;
            parse_json(
                conn.request(Step {
                    auth: &[Auth::Bearer(Input::Session(&session))],
                    ..Step::get(path)
                })
                .await?,
            )
        },
    )
    .await
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        let state: ServerState = get(conn, "api/v1/serverstate").await?;
        if state.last_event_id < 0 {
            return Err(Class::Protocol.into());
        }
        Ok(())
    })
}

fn finished(metadata: Option<&Metadata>) -> Option<u64> {
    let value = metadata?.last_backup_finished.as_deref()?;
    if value.len() != 16 || !value.is_ascii() || &value[8..9] != "T" || &value[15..] != "Z" {
        return None;
    }
    let rfc3339 = format!(
        "{}-{}-{}T{}:{}:{}Z",
        &value[..4],
        &value[4..6],
        &value[6..8],
        &value[9..11],
        &value[11..13],
        &value[13..15]
    );
    let (seconds, _) = parse_rfc3339_parts(&rfc3339)?;
    u64::try_from(seconds).ok()
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(jobs) = within_response_limit(get::<Vec<JobEntry>>(conn, BACKUPS).await)? else {
            return Ok(StructuredBody::Unavailable {
                message: "Backup list exceeds the response limit".into(),
            });
        };
        if jobs.is_empty() {
            return Ok(StructuredBody::Stat {
                value: "No backup jobs".into(),
            });
        }
        let mut backups: Vec<_> = jobs
            .into_iter()
            .map(|job| (finished(job.backup.metadata.as_ref()), job.backup))
            .collect();
        backups.sort_unstable_by(|(a, job_a), (b, job_b)| {
            b.cmp(a).then_with(|| job_a.name.cmp(&job_b.name))
        });
        let items = backups
            .into_iter()
            .take(10)
            .map(|(time, backup)| {
                let completion = time.map_or_else(
                    || "No recorded completion".into(),
                    |time| format!("Finished {}", fmt_utc(time)),
                );
                let size = backup
                    .metadata
                    .and_then(|metadata| metadata.source_files_size)
                    .and_then(|size| size.parse::<u64>().ok())
                    .map_or_else(
                        || "Source size unavailable".into(),
                        |bytes| format!("{} source", human_bytes(bytes)),
                    );
                BodyItem {
                    icon: None,
                    title: display_text(conn, backup.name),
                    secondary: Some(format!("{completion} · {size}")),
                    link: None,
                    time: None,
                }
            })
            .collect();
        Ok(StructuredBody::ItemList { items })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;

    #[tokio::test]
    async fn puts_password_in_login_json_and_bearer_in_reads_only() {
        const LOGIN: &str = "POST /prefix/api/v1/auth/login HTTP/1.1";
        const READ: &str = "GET /prefix/api/v1/backups HTTP/1.1";
        const CHECK: &str = "GET /prefix/api/v1/serverstate HTTP/1.1";
        let oversized = "x".repeat(MAX_RESPONSE + 1);
        let exchanges = [
            (LOGIN, 200, r#"{"AccessToken":"fake-jwt"}"#),
            (CHECK, 200, r#"{"LastEventID":1}"#),
            (READ, 401, ""),
            (LOGIN, 200, r#"{"AccessToken":"fake-renewed-jwt"}"#),
            (READ, 200, r#"[{"Backup":{"Name":"docs","Metadata":{"LastBackupFinished":"20261121T103000Z","SourceFilesSize":"1048576"}}}]"#),
            (READ, 200, &oversized),
            (CHECK, 401, ""),
            (LOGIN, 401, ""),
            (LOGIN, 200, r#"{"token":"wrong-shape"}"#),
        ].map(|(request, status, body)| Exchange { request, status, body });
        exercise(&DUPLICATI, &[(PASSWORD, "fake-web-password")], Values::new(), &exchanges,
            |index, wire| {
                if wire.starts_with("POST ") {
                    let (headers, body) = wire.split_once("\r\n\r\n").unwrap();
                    assert!(!headers.contains("fake-web-password") && !headers.contains("authorization:"));
                    let value: Value = serde_json::from_str(body).unwrap();
                    assert!(value == serde_json::json!({"Password":"fake-web-password","RememberMe":false}));
                    assert!(!wire.contains("fake-jwt") && !wire.contains("fake-renewed-jwt"));
                } else {
                    if index < 3 {
                        assert_header(wire, "authorization: Bearer fake-jwt", "fake-jwt");
                    } else {
                        assert_header(wire, "authorization: Bearer fake-renewed-jwt", "fake-renewed-jwt");
                    }
                    assert!(!wire.contains("fake-web-password"));
                }
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let fetch = async || super::super::fetch(conn, &DUPLICATI.widgets[0], &Values::new()).await.unwrap();
                let body = serde_json::to_value(fetch().await).unwrap();
                assert_eq!(body["items"][0]["title"], "docs");
                let details = body["items"][0]["secondary"].as_str().unwrap();
                assert!(details.contains("2026-11-21") && details.contains("1.0 MiB source"));
                assert!(matches!(fetch().await, StructuredBody::Unavailable { .. }));
                assert_test_fault(conn, &DUPLICATI, Class::Auth).await;
                assert_test_fault(conn, &DUPLICATI, Class::Protocol).await;
            }
        ).await;
    }
}
