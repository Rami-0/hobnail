use super::*;
use widgets::StructuredBody;

const TOKEN: &str = "token";

pub const PAPERLESS: Provider = Provider {
    id: "paperless-ngx",
    name: "Paperless-ngx",
    category: "Files",
    icon: "paperless-ngx",
    color: "#2e8b3c",
    description: "Show the total document count and inbox.",
    docs: "https://paperless-ngx-docs.readthedocs.io/en/latest/api.html",
    public_internet: false,
    contacts: "Your configured Paperless-ngx server.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(4096),
            ..Field::new(TOKEN, "API token", Kind::Secret, "In Paperless-ngx open My profile (top right) and generate the API auth token, for an account that can view documents and inbox tags.")
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "documents",
        name: "Documents and inbox",
        description: "Total visible documents and documents assigned to inbox tags.",
        recommended: true,
        default_size: size(4, 2),
        min_size: size(3, 2),
        sample: r#"{"kind":"stat_grid","rows":[["Documents","3,412"],["Inbox","6"],["Tags","48"],["Correspondents","120"]]}"#,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
struct ApiRoot {
    documents: String,
}

#[derive(Deserialize)]
struct Statistics {
    documents_total: u64,
    documents_inbox: Option<u64>,
}

async fn get<T: serde::de::DeserializeOwned>(conn: &Conn<'_>, path: &str) -> Result<T, Fault> {
    parse_json(
        conn.request(Step {
            auth: &[Auth::Authorization("Token ", Input::Credential(TOKEN))],
            accept_json: true,
            ..Step::get(path)
        })
        .await?,
    )
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        let root: ApiRoot = get(conn, "api/").await?;
        if !root.documents.ends_with("/api/documents/") {
            return Err(Class::Protocol.into());
        }
        Ok(())
    })
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let stats: Statistics = get(conn, "api/statistics/").await?;
        let inbox = stats
            .documents_inbox
            .map_or_else(|| "No visible inbox tag".into(), |count| count.to_string());
        Ok(StructuredBody::StatGrid {
            rows: vec![
                ("Documents".into(), stats.documents_total.to_string()),
                ("Inbox".into(), inbox),
            ],
        })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn reads_document_and_inbox_counts_and_handles_missing_inbox_tag() {
        const ROOT: &str = "GET /prefix/api/ HTTP/1.1";
        const STATS: &str = "GET /prefix/api/statistics/ HTTP/1.1";
        let exchanges = [
            (
                ROOT,
                200,
                r#"{"documents":"http://fixture/prefix/api/documents/"}"#,
            ),
            (STATS, 200, r#"{"documents_total":37,"documents_inbox":4}"#),
            (
                STATS,
                200,
                r#"{"documents_total":37,"documents_inbox":null}"#,
            ),
            (ROOT, 401, ""),
            (ROOT, 200, "{}"),
        ]
        .map(|(request, status, body)| Exchange {
            request,
            status,
            body,
        });
        exercise(
            &PAPERLESS,
            &[(TOKEN, "fake-paperless-token")],
            Values::new(),
            &exchanges,
            |_, wire| {
                assert_header(
                    wire,
                    "authorization: Token fake-paperless-token",
                    "fake-paperless-token",
                );
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let fetch = async || {
                    serde_json::to_value(
                        super::super::fetch(conn, &PAPERLESS.widgets[0], &Values::new())
                            .await
                            .unwrap(),
                    )
                    .unwrap()
                };
                assert_eq!(
                    fetch().await["rows"],
                    json!([["Documents", "37"], ["Inbox", "4"]])
                );
                assert_eq!(
                    fetch().await["rows"][1],
                    json!(["Inbox", "No visible inbox tag"])
                );
                assert_test_fault(conn, &PAPERLESS, Class::Auth).await;
                assert_test_fault(conn, &PAPERLESS, Class::Protocol).await;
            },
        )
        .await;
    }
}
