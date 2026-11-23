use super::*;
use crate::util::human_bytes;
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

const KEY: &str = "api_key";
const FOLDER_LIMIT: usize = 10;

pub const SYNCTHING: Provider = Provider {
    id: "syncthing",
    name: "Syncthing",
    category: "Files",
    icon: "syncthing",
    color: "#0b8ac9",
    detect: super::detect(&["syncthing"], 8384),
    description: "Show folder sync status and completion, and connected devices.",
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
    widgets: &[
        Widget {
            id: "sync",
            name: "Synchronization",
            description: "Connected devices and how many of up to ten folders are in sync.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Devices online","value":"2 / 3","glyph":"users","tone":"warn"},{"label":"Folders in sync","value":"5 / 5","glyph":"check","tone":"up"}]}"#,
            fields: &[REFRESH],
            fetch: overview,
            validate: no_widget_options,
        },
        Widget {
            id: "folders",
            name: "Folders",
            description: "Up to ten folders with their state, completion and size.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"Documents","secondary":"In sync · 12,480 files","value":"18.2 GiB","status":"up","progress":100},{"title":"Photos","secondary":"Changes pending · 48,211 files","value":"2.1 GiB left","status":"warn","progress":87},{"title":"Archive","secondary":"Paused","value":"—"}]}"#,
            fields: &[REFRESH],
            fetch: folders,
            validate: no_widget_options,
        },
        Widget {
            id: "devices",
            name: "Devices",
            description: "Up to ten remote devices with their connection state and address.",
            recommended: false,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"rows","rows":[{"title":"laptop","secondary":"192.0.2.10:22000","value":"v2.0.11","status":"up"},{"title":"phone","secondary":"Disconnected","status":"down"},{"title":"backup-nas","secondary":"Paused"}]}"#,
            fields: &[REFRESH],
            fetch: devices,
            validate: no_widget_options,
        },
    ],
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
    #[serde(default)]
    global_files: u64,
    #[serde(default)]
    global_bytes: u64,
    #[serde(default)]
    need_bytes: u64,
}

#[derive(Deserialize)]
struct ConnectionsStatus {
    connections: std::collections::BTreeMap<String, Device>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    connected: bool,
    #[serde(default)]
    paused: bool,
    #[serde(default)]
    address: String,
    #[serde(default)]
    client_version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceConfig {
    #[serde(rename = "deviceID")]
    id: String,
    #[serde(default)]
    name: String,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Paused,
    Unavailable,
    Error,
    Pending,
    InSync,
    Working,
}

impl State {
    fn label(self) -> &'static str {
        match self {
            Self::Paused => "Paused",
            Self::Unavailable => "Unavailable",
            Self::Error => "Error",
            Self::Pending => "Changes pending",
            Self::InSync => "In sync",
            Self::Working => "Working",
        }
    }

    fn health(self) -> Option<Health> {
        match self {
            Self::InSync => Some(Health::Up),
            Self::Error => Some(Health::Down),
            Self::Pending | Self::Working => Some(Health::Warn),
            Self::Paused | Self::Unavailable => None,
        }
    }
}

async fn folder_state(
    conn: &Conn<'_>,
    folder: &Folder,
) -> Result<(State, Option<FolderStatus>), Fault> {
    if folder.paused {
        return Ok((State::Paused, None));
    }
    let status = get::<FolderStatus>(
        conn,
        "rest/db/status",
        &[("folder", Input::Text(&folder.id))],
    )
    .await;
    let status = match status {
        Err(fault) if matches!(fault.class, Class::HttpStatus | Class::TooLarge) => {
            return Ok((State::Unavailable, None))
        }
        Err(fault) => return Err(fault),
        Ok(status) => status,
    };
    let state = if status.errors > 0
        || status
            .error
            .as_deref()
            .is_some_and(|error| !error.is_empty())
    {
        State::Error
    } else if status.need_total_items > 0 || status.need_deletes > 0 {
        State::Pending
    } else if status.state == "idle" {
        State::InSync
    } else {
        State::Working
    };
    Ok((state, Some(status)))
}

async fn configured_folders(conn: &Conn<'_>) -> Result<Option<Vec<Folder>>, Fault> {
    let folders =
        within_response_limit(get::<Vec<Folder>>(conn, "rest/config/folders", &[]).await)?;
    Ok(folders.map(|mut folders| {
        folders.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        folders
    }))
}

fn tile(label: &str, value: String, glyph: Glyph, tone: Option<Health>) -> StatTile {
    StatTile {
        label: label.into(),
        value,
        glyph: Some(glyph),
        delta: None,
        tone,
        series: Vec::new(),
    }
}

fn share_tone(part: usize, whole: usize) -> Health {
    if part == whole {
        Health::Up
    } else if part == 0 {
        Health::Down
    } else {
        Health::Warn
    }
}

fn overview<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let devices = within_response_limit(
            get::<ConnectionsStatus>(conn, "rest/system/connections", &[]).await,
        )?;
        let devices = devices.map_or_else(
            || {
                tile(
                    "Devices online",
                    "List too large".into(),
                    Glyph::Users,
                    None,
                )
            },
            |devices| {
                let total = devices.connections.len();
                let online = devices
                    .connections
                    .values()
                    .filter(|device| device.connected)
                    .count();
                tile(
                    "Devices online",
                    format!("{online} / {total}"),
                    Glyph::Users,
                    Some(share_tone(online, total)),
                )
            },
        );
        let folders = match configured_folders(conn).await? {
            Some(folders) => {
                let checked = folders.len().min(FOLDER_LIMIT);
                let mut in_sync = 0;
                for folder in folders.iter().take(checked) {
                    in_sync += usize::from(folder_state(conn, folder).await?.0 == State::InSync);
                }
                let mut folder_tile = tile(
                    "Folders in sync",
                    format!("{in_sync} / {checked}"),
                    Glyph::Check,
                    Some(share_tone(in_sync, checked)),
                );
                if checked < folders.len() {
                    folder_tile.delta = Some(format!("{} configured", folders.len()));
                }
                folder_tile
            }
            None => tile(
                "Folders in sync",
                "List too large".into(),
                Glyph::Check,
                None,
            ),
        };
        Ok(StructuredBody::Tiles {
            tiles: vec![devices, folders],
        })
    })
}

fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn folders<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(folders) = configured_folders(conn).await? else {
            return Ok(StructuredBody::Unavailable {
                message: "Folder list too large to show.".into(),
            });
        };
        if folders.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No folders configured.".into(),
            });
        }
        let mut rows = Vec::new();
        for folder in folders.iter().take(FOLDER_LIMIT) {
            let (state, status) = folder_state(conn, folder).await?;
            let label = if folder.label.is_empty() {
                &folder.id
            } else {
                &folder.label
            };
            let mut row = BodyRow {
                title: display_text(conn, label.clone()),
                secondary: Some(state.label().into()),
                value: Some("—".into()),
                status: state.health(),
                ..BodyRow::default()
            };
            if let Some(status) = status {
                row.secondary = Some(format!(
                    "{} · {} files",
                    state.label(),
                    count(status.global_files)
                ));
                let need = status.need_bytes.min(status.global_bytes);
                row.progress = Some(if status.global_bytes == 0 {
                    100
                } else {
                    ((status.global_bytes - need) as f64 * 100.0 / status.global_bytes as f64)
                        .floor() as u8
                });
                row.value = Some(if need > 0 {
                    format!("{} left", human_bytes(need))
                } else {
                    human_bytes(status.global_bytes)
                });
            }
            rows.push(row);
        }
        Ok(StructuredBody::Rows { rows })
    })
}

fn devices<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let Some(status) = within_response_limit(
            get::<ConnectionsStatus>(conn, "rest/system/connections", &[]).await,
        )?
        else {
            return Ok(StructuredBody::Unavailable {
                message: "Device list too large to show.".into(),
            });
        };
        // Names are optional: without them the device ID's first block shows.
        let names: std::collections::BTreeMap<String, String> = within_response_limit(
            get::<Vec<DeviceConfig>>(conn, "rest/config/devices", &[]).await,
        )?
        .unwrap_or_default()
        .into_iter()
        .filter(|device| !device.name.is_empty())
        .map(|device| (device.id, device.name))
        .collect();
        let mut list: Vec<_> = status.connections.into_iter().collect();
        list.sort_by_key(|(_, device)| (!device.connected, device.paused));
        if list.is_empty() {
            return Ok(StructuredBody::Unavailable {
                message: "No remote devices configured.".into(),
            });
        }
        let rows = list
            .into_iter()
            .take(FOLDER_LIMIT)
            .map(|(id, device)| {
                let title = names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| id.split('-').next().unwrap_or(&id).to_owned());
                let (secondary, status) = if device.paused {
                    ("Paused".to_owned(), None)
                } else if device.connected {
                    let address = Some(device.address).filter(|a| !a.is_empty());
                    (
                        address.map_or_else(|| "Connected".into(), |a| display_text(conn, a)),
                        Some(Health::Up),
                    )
                } else {
                    ("Disconnected".to_owned(), Some(Health::Down))
                };
                BodyRow {
                    title: display_text(conn, title),
                    secondary: Some(secondary),
                    value: Some(device.client_version)
                        .filter(|version| device.connected && !version.is_empty())
                        .map(|version| display_text(conn, version)),
                    status,
                    ..BodyRow::default()
                }
            })
            .collect();
        Ok(StructuredBody::Rows { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    fn key(_: usize, wire: &str) -> &'static str {
        assert_header(wire, "x-api-key: fake-sync-key", "fake-sync-key");
        ""
    }

    async fn fetch(conn: &Conn<'_>, widget: usize) -> Result<Value, Fault> {
        super::super::fetch(conn, &SYNCTHING.widgets[widget], &Values::new())
            .await
            .map(|body| serde_json::to_value(body).unwrap())
    }

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
        exercise(&SYNCTHING, &[(KEY, "fake-sync-key")], Values::new(), &exchanges, key, async |conn| {
            conn.run(test(conn)).await.unwrap();
            assert_eq!(
                fetch(conn, 0).await.unwrap()["tiles"],
                json!([
                    {"label": "Devices online", "value": "1 / 2", "glyph": "users", "tone": "warn"},
                    {"label": "Folders in sync", "value": "1 / 2", "glyph": "check", "tone": "warn"},
                ])
            );
            assert_eq!(
                fetch(conn, 0).await.unwrap()["tiles"],
                json!([
                    {"label": "Devices online", "value": "0 / 0", "glyph": "users", "tone": "up"},
                    {"label": "Folders in sync", "value": "List too large", "glyph": "check"},
                ])
            );
            assert_test_fault(conn, &SYNCTHING, Class::Auth).await;
            assert_test_fault(conn, &SYNCTHING, Class::Protocol).await;
        })
        .await;
    }

    #[tokio::test]
    async fn lists_folders_with_completion_and_named_devices() {
        // Shapes from docs.syncthing.net (system/connections) and v2.0.11 folder_summary.go.
        const CONNECTIONS: &str = r#"{"connections":{
            "AAAAAAA-BBBBBBB-CCCCCCC-DDDDDDD-EEEEEEE-FFFFFFF-GGGGGGG-HHHHHHH":{"address":"","at":"0001-01-01T00:00:00Z","clientVersion":"","connected":false,"inBytesTotal":0,"isLocal":false,"outBytesTotal":0,"paused":false,"type":""},
            "ZZZZZZZ-YYYYYYY-XXXXXXX-WWWWWWW-VVVVVVV-UUUUUUU-TTTTTTT-SSSSSSS":{"address":"192.0.2.10:22000","clientVersion":"v2.0.11","connected":true,"isLocal":true,"paused":false,"type":"tcp-client"},
            "PPPPPPP-QQQQQQQ-RRRRRRR-SSSSSSS-TTTTTTT-UUUUUUU-VVVVVVV-WWWWWWW":{"connected":false,"paused":true}},
            "total":{"inBytesTotal":1479,"outBytesTotal":1318}}"#;
        const DEVICES: &str = r#"[{"deviceID":"ZZZZZZZ-YYYYYYY-XXXXXXX-WWWWWWW-VVVVVVV-UUUUUUU-TTTTTTT-SSSSSSS","name":"laptop","addresses":["dynamic"]},{"deviceID":"PPPPPPP-QQQQQQQ-RRRRRRR-SSSSSSS-TTTTTTT-UUUUUUU-VVVVVVV-WWWWWWW","name":"backup-nas"}]"#;
        let exchanges = [
            ("GET /prefix/rest/config/folders HTTP/1.1", 200, r#"[{"id":"photos","label":"Photos","paused":false},{"id":"docs","label":"Documents","paused":false},{"id":"zarchive","label":"Archive","paused":true}]"#),
            ("GET /prefix/rest/db/status?folder=docs HTTP/1.1", 200, r#"{"globalBytes":19542101196,"globalFiles":12480,"needBytes":0,"needTotalItems":0,"needDeletes":0,"errors":0,"error":"","state":"idle"}"#),
            ("GET /prefix/rest/db/status?folder=photos HTTP/1.1", 200, r#"{"globalBytes":17179869184,"globalFiles":48211,"needBytes":2254857830,"needTotalItems":120,"needDeletes":0,"errors":0,"state":"syncing"}"#),
            ("GET /prefix/rest/system/connections HTTP/1.1", 200, CONNECTIONS),
            ("GET /prefix/rest/config/devices HTTP/1.1", 200, DEVICES),
            ("GET /prefix/rest/system/connections HTTP/1.1", 200, r#"{"connections":[]}"#),
            ("GET /prefix/rest/config/folders HTTP/1.1", 401, ""),
        ].map(|(request, status, body)| Exchange { request, status, body });
        exercise(&SYNCTHING, &[(KEY, "fake-sync-key")], Values::new(), &exchanges, key, async |conn| {
            assert_eq!(
                fetch(conn, 1).await.unwrap()["rows"],
                json!([
                    {"title": "Documents", "secondary": "In sync · 12,480 files", "value": "18.2 GiB", "status": "up", "progress": 100},
                    {"title": "Photos", "secondary": "Changes pending · 48,211 files", "value": "2.1 GiB left", "status": "warn", "progress": 86},
                    {"title": "Archive", "secondary": "Paused", "value": "—"},
                ])
            );
            assert_eq!(
                fetch(conn, 2).await.unwrap()["rows"],
                json!([
                    {"title": "laptop", "secondary": "192.0.2.10:22000", "value": "v2.0.11", "status": "up"},
                    {"title": "AAAAAAA", "secondary": "Disconnected", "status": "down"},
                    {"title": "backup-nas", "secondary": "Paused"},
                ])
            );
            assert!(matches!(fetch(conn, 2).await.unwrap_err().class, Class::Protocol));
            assert!(matches!(fetch(conn, 1).await.unwrap_err().class, Class::Auth));
        })
        .await;
    }
}
