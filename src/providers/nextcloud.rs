use super::*;
use crate::util::human_bytes;
use widgets::StructuredBody;

const PATH: &str = "ocs/v2.php/apps/serverinfo/api/v1/info";

pub const NEXTCLOUD: Provider = Provider {
    id: "nextcloud",
    name: "Nextcloud",
    category: "Files",
    icon: "nextcloud",
    description: "Show free storage, users, recent activity counts and shares.",
    docs: "https://github.com/nextcloud/serverinfo",
    public_internet: false,
    contacts: "Your configured Nextcloud server.",
    fields: &[
        SERVICE_URL,
        Field {
            label: "Admin username",
            help: "Enter the account with access to Nextcloud server information.",
            required: true,
            ..OPTIONAL_BASIC_USER
        },
        Field {
            label: "App password",
            help: "Create an app password in the account’s security settings.",
            required: true,
            ..OPTIONAL_BASIC_PASSWORD
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "server",
        name: "Storage and activity",
        description: "Free space, total and recently active users, and shares.",
        recommended: true,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
struct Envelope {
    ocs: Ocs,
}

#[derive(Deserialize)]
struct Ocs {
    meta: Meta,
    data: Value,
}

#[derive(Deserialize)]
struct Meta {
    statuscode: u16,
}

#[derive(Deserialize)]
struct Info {
    nextcloud: Nextcloud,
    #[serde(rename = "activeUsers")]
    active_users: ActiveUsers,
}

#[derive(Deserialize)]
struct Nextcloud {
    system: System,
    storage: Storage,
    shares: Shares,
}

#[derive(Deserialize)]
struct System {
    freespace: Option<u64>,
}

#[derive(Deserialize)]
struct Storage {
    num_users: u64,
}

#[derive(Deserialize)]
struct Shares {
    num_shares: u64,
}

#[derive(Deserialize)]
struct ActiveUsers {
    last5minutes: u64,
    last1hour: u64,
    last24hours: u64,
}

async fn info(conn: &Conn<'_>) -> Result<Info, Fault> {
    let result: Envelope = parse_json(
        conn.request(Step {
            auth: &[Auth::Basic(
                Input::Credential("username"),
                Input::Credential("password"),
            )],
            ocs_api_request: true,
            query: &[("format", Input::Text("json"))],
            ..Step::get(PATH)
        })
        .await?,
    )?;
    success_status(result.ocs.meta.statuscode)?;
    serde_json::from_value(result.ocs.data).map_err(|_| Class::Protocol.into())
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { info(conn).await.map(|_| ()) })
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let info = info(conn).await?;
        let active = info.active_users;
        Ok(StructuredBody::StatGrid {
            rows: vec![
                (
                    "Free space".into(),
                    info.nextcloud
                        .system
                        .freespace
                        .map_or_else(|| "Unavailable".into(), human_bytes),
                ),
                ("Users".into(), info.nextcloud.storage.num_users.to_string()),
                (
                    "Active users (5 min)".into(),
                    active.last5minutes.to_string(),
                ),
                ("Active users (1 h)".into(), active.last1hour.to_string()),
                ("Active users (24 h)".into(), active.last24hours.to_string()),
                (
                    "Shares".into(),
                    info.nextcloud.shares.num_shares.to_string(),
                ),
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
    async fn reads_ocs_activity_and_checks_transport_and_envelope_status() {
        const REQUEST: &str =
            "GET /prefix/ocs/v2.php/apps/serverinfo/api/v1/info?format=json HTTP/1.1";
        const INFO: &str = r#"{"ocs":{"meta":{"statuscode":200},"data":{"nextcloud":{"system":{"freespace":1048576},"storage":{"num_users":7},"shares":{"num_shares":6}},"activeUsers":{"last5minutes":1,"last1hour":2,"last24hours":3}}}}"#;
        exercise(
            &NEXTCLOUD,
            &[("username", "fake-user"), ("password", "fake-app-pass")],
            Values::new(),
            &[
                Exchange {
                    request: REQUEST,
                    status: 200,
                    body: INFO,
                },
                Exchange {
                    request: REQUEST,
                    status: 200,
                    body: INFO,
                },
                Exchange {
                    request: REQUEST,
                    status: 200,
                    body: r#"{"ocs":{"meta":{"statuscode":403},"data":[]}}"#,
                },
                Exchange {
                    request: REQUEST,
                    status: 401,
                    body: "",
                },
                Exchange {
                    request: REQUEST,
                    status: 200,
                    body: "{}",
                },
            ],
            |_, wire| {
                assert_header(
                    wire,
                    "authorization: Basic ZmFrZS11c2VyOmZha2UtYXBwLXBhc3M=",
                    "ZmFrZS11c2VyOmZha2UtYXBwLXBhc3M=",
                );
                assert!(wire.contains("\r\nocs-apirequest: true\r\n"));
                assert!(!wire.contains("fake-user") && !wire.contains("fake-app-pass"));
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let body = super::super::fetch(conn, &NEXTCLOUD.widgets[0], &Values::new())
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(body).unwrap()["rows"],
                    json!([
                        ["Free space", "1.0 MiB"],
                        ["Users", "7"],
                        ["Active users (5 min)", "1"],
                        ["Active users (1 h)", "2"],
                        ["Active users (24 h)", "3"],
                        ["Shares", "6"]
                    ])
                );
                assert_test_fault(conn, &NEXTCLOUD, Class::Auth).await;
                assert_test_fault(conn, &NEXTCLOUD, Class::Auth).await;
                assert_test_fault(conn, &NEXTCLOUD, Class::Protocol).await;
            },
        )
        .await;
    }
}
