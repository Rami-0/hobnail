use super::*;
use widgets::StructuredBody;

pub const JELLYSEERR: Provider = Provider {
    id: "jellyseerr",
    name: "Jellyseerr / Overseerr",
    category: "Media",
    icon: "jellyseerr",
    color: "#7c4ddb",
    description: "Show how many media requests are pending and approved.",
    docs: "https://api-docs.overseerr.dev/",
    public_internet: false,
    contacts:
        "Your configured Jellyseerr or Overseerr service. Public addresses require internet access.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(
                "api_key",
                "API key",
                Kind::Secret,
                "Use the instance API key from Settings → General.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "requests",
        name: "Media requests",
        description: "Pending and approved request totals.",
        recommended: true,
        default_size: size(4, 2),
        min_size: size(3, 2),
        sample: r#"{"kind":"stat_grid","rows":[["Pending","4"],["Approved","18"],["Processing","2"],["Available","130"]]}"#,
        fields: &[REFRESH],
        fetch: requests,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
struct Counts {
    pending: u64,
    approved: u64,
}

async fn counts(conn: &Conn<'_>) -> Result<Counts, Fault> {
    parse_json(
        conn.request(Step {
            auth: &[Auth::Header("x-api-key", Input::Credential("api_key"))],
            ..Step::get("api/v1/request/count")
        })
        .await?,
    )
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { counts(conn).await.map(|_| ()) })
}

fn requests<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let counts = counts(conn).await?;
        Ok(StructuredBody::StatGrid {
            rows: vec![
                ("Pending".into(), counts.pending.to_string()),
                ("Approved".into(), counts.approved.to_string()),
            ],
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use serde_json::json;

    #[tokio::test]
    async fn reads_counts_using_the_shared_request_api() {
        const REQUEST: &str = "GET /prefix/api/v1/request/count HTTP/1.1";
        const KEY: &str = "fake-requests-api-key";
        const COUNTS: &str = r#"{"total":28,"movie":18,"tv":10,"pending":5,"approved":20,"declined":3,"processing":7,"available":13}"#;
        exercise(&JELLYSEERR, &[("api_key", KEY)], Values::new(), &[
            Exchange { request: REQUEST, status: 200, body: COUNTS },
            Exchange { request: REQUEST, status: 200, body: COUNTS },
            Exchange { request: REQUEST, status: 403, body: r#"{"status":403,"error":"You do not have permission to access this endpoint"}"# },
            Exchange { request: REQUEST, status: 200, body: r#"{"pending":5,"approved":"20"}"# },
        ], |_, wire| {
            assert_header(wire, &format!("x-api-key: {KEY}"), KEY);
            ""
        }, async |conn| {
            conn.run((JELLYSEERR.test)(conn)).await.unwrap();
            let body = super::super::fetch(conn, &JELLYSEERR.widgets[0], &Values::new()).await.unwrap();
            assert_eq!(serde_json::to_value(body).unwrap()["rows"], json!([["Pending", "5"], ["Approved", "20"]]));
            assert_test_fault(conn, &JELLYSEERR, Class::Auth).await;
            assert_test_fault(conn, &JELLYSEERR, Class::Protocol).await;
        }).await;
    }
}
