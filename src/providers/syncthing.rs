use super::*;
use widgets::StructuredBody;

const KEY: &str = "api_key";
const FOLDER_LIMIT: usize = 10;

pub const SYNCTHING: Provider = Provider {
    id: "syncthing",
    name: "Syncthing",
    category: "Files",
    icon: "syncthing",
    color: "#0b8ac9",
    description: "Show local folder sync status and connected devices.",
    docs: "https://docs.syncthing.net/dev/rest.html",
    public_internet: false,
    contacts: "Your configured Syncthing GUI and REST server.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(256),
            ..Field::new(
                KEY,
                "API key",
                Kind::Secret,
                "Copy the API Key from Syncthing Settings → GUI.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "sync",
        name: "Synchronization",
        description: "Connected devices and local sync status for up to ten folders.",
        recommended: true,
        default_size: size(4, 2),
        min_size: size(3, 2),
        sample: r#"{"kind":"stat_grid","rows":[["Folders","5"],["Devices","3"],["Connected","2"],["Out of sync","0"]]}"#,
        fields: &[REFRESH],
        fetch: overview,
        validate: no_widget_options,
    }],
};

#[derive(Deserialize)]
struct Status {
    #[serde(rename = "myID")]
    id: String,
}

#[derive(Deserialize)]
struct Folder {
    id: String,
    label: String,
    paused: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderStatus {
    state: String,
    errors: u64,
    need_total_items: u64,
    need_deletes: u64,
    error: Option<String>,
}

#[derive(Deserialize)]
struct ConnectionsStatus {
    connections: std::collections::BTreeMap<String, Device>,
}

#[derive(Deserialize)]
struct Device {
    connected: bool,
}

async fn get<T: serde::de::DeserializeOwned>(
    conn: &Conn<'_>,
    path: &str,
    query: &[(&str, Input<'_>)],
) -> Result<T, Fault> {
    parse_json(
        conn.request(Step {
            auth: &[Auth::Header("x-api-key", Input::Credential(KEY))],
            query,
            ..Step::get(path)
        })
        .await?,
    )
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        let status: Status = get(conn, "rest/system/status", &[]).await?;
        if status.id.is_empty() {
            return Err(Class::Protocol.into());
        }
        Ok(())
    })
}

async fn folder_state(conn: &Conn<'_>, folder: &Folder) -> Result<&'static str, Fault> {
    if folder.paused {
        return Ok("Paused");
    }
    let status = get::<FolderStatus>(
        conn,
        "rest/db/status",
        &[("folder", Input::Text(&folder.id))],
    )
    .await;
    match status {
        Err(fault) if matches!(fault.class, Class::HttpStatus | Class::TooLarge) => {
            Ok("Unavailable")
        }
        Err(fault) => Err(fault),
        Ok(status)
            if status.errors > 0
                || status
                    .error
                    .as_deref()
                    .is_some_and(|error| !error.is_empty()) =>
        {
            Ok("Error")
        }
        Ok(status) if status.need_total_items > 0 || status.need_deletes > 0 => {
            Ok("Changes pending")
        }
        Ok(status) if status.state == "idle" => Ok("In sync"),
        Ok(_) => Ok("Working"),
    }
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let devices = within_response_limit(
            get::<ConnectionsStatus>(conn, "rest/system/connections", &[]).await,
        )?;
        let online = devices.map_or_else(
            || "List too large".into(),
            |devices| {
                let online = devices
                    .connections
                    .values()
                    .filter(|device| device.connected)
                    .count();
                format!("{online} / {}", devices.connections.len())
            },
        );
        let mut rows = vec![("Devices online".into(), online)];
        let folders =
            within_response_limit(get::<Vec<Folder>>(conn, "rest/config/folders", &[]).await)?;
        if let Some(mut folders) = folders {
            folders.sort_unstable_by(|a, b| a.id.cmp(&b.id));
            let checked = folders.len().min(FOLDER_LIMIT);
            let mut in_sync = 0;
            for folder in folders.iter().take(checked) {
                let state = folder_state(conn, folder).await?;
                in_sync += usize::from(state == "In sync");
                let label = if folder.label.is_empty() {
                    &folder.id
                } else {
                    &folder.label
                };
                rows.push((display_text(conn, label.clone()), state.into()));
            }
            let coverage = if checked < folders.len() {
                format!(" ({} configured)", folders.len())
            } else {
                String::new()
            };
            rows.insert(
                1,
                (
                    "Folders in sync".into(),
                    format!("{in_sync} / {checked}{coverage}"),
                ),
            );
        } else {
            rows.push(("Folders in sync".into(), "List too large".into()));
        }
        Ok(StructuredBody::StatGrid { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn reports_local_sync_and_keeps_device_counts_when_folders_are_oversized() {
        let oversized = "x".repeat(MAX_RESPONSE + 1);
        let exchanges = [
            ("GET /prefix/rest/system/status HTTP/1.1", 200, r#"{"myID":"fake-device"}"#),
            ("GET /prefix/rest/system/connections HTTP/1.1", 200, r#"{"connections":{"a":{"connected":true},"b":{"connected":false}}}"#),
            ("GET /prefix/rest/config/folders HTTP/1.1", 200, r#"[{"id":"a&b","label":"Docs","paused":false},{"id":"z","label":"","paused":true}]"#),
            ("GET /prefix/rest/db/status?folder=a%26b HTTP/1.1", 200, r#"{"state":"idle","errors":0,"needTotalItems":0,"needDeletes":0}"#),
            ("GET /prefix/rest/system/connections HTTP/1.1", 200, r#"{"connections":{}}"#),
            ("GET /prefix/rest/config/folders HTTP/1.1", 200, &oversized),
            ("GET /prefix/rest/system/status HTTP/1.1", 403, ""),
            ("GET /prefix/rest/system/status HTTP/1.1", 200, "{}"),
        ].map(|(request, status, body)| Exchange { request, status, body });
        exercise(
            &SYNCTHING,
            &[(KEY, "fake-sync-key")],
            Values::new(),
            &exchanges,
            |_, wire| {
                assert_header(wire, "x-api-key: fake-sync-key", "fake-sync-key");
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let fetch = async || {
                    serde_json::to_value(
                        super::super::fetch(conn, &SYNCTHING.widgets[0], &Values::new())
                            .await
                            .unwrap(),
                    )
                    .unwrap()
                };
                assert_eq!(
                    fetch().await["rows"],
                    json!([
                        ["Devices online", "1 / 2"],
                        ["Folders in sync", "1 / 2"],
                        ["Docs", "In sync"],
                        ["z", "Paused"]
                    ])
                );
                assert_eq!(
                    fetch().await["rows"],
                    json!([
                        ["Devices online", "0 / 0"],
                        ["Folders in sync", "List too large"]
                    ])
                );
                assert_test_fault(conn, &SYNCTHING, Class::Auth).await;
                assert_test_fault(conn, &SYNCTHING, Class::Protocol).await;
            },
        )
        .await;
    }
}
