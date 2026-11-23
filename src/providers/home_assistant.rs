use super::*;
use widgets::{BodyRow, Glyph, Health, StatTile, StructuredBody};

const TOKEN: &str = "token";
const ENTITIES: &str = "entities";
const ENTITY_FIELD: Field = Field {
    required: true,
    max_length: Some(128),
    ..Field::new(
        ENTITIES,
        "Entity IDs",
        Kind::List,
        "One entity ID per line, such as sensor.temperature. Choose up to 16.",
    )
};

pub const HOME_ASSISTANT: Provider = Provider {
    id: "home-assistant",
    name: "Home Assistant",
    category: "Home automation",
    icon: "home-assistant",
    color: "#18a8e0",
    description: "Show selected entity states, sensor readings and on/off counts.",
    docs: "https://contributors.home-assistant.io/core/internals/rest-api/",
    public_internet: false,
    contacts: "Your configured Home Assistant server.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(4096),
            ..Field::new(
                TOKEN,
                "Long-lived access token",
                Kind::Secret,
                "Create a token in your Home Assistant profile.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "entities",
            name: "Entity states",
            description: "The chosen entities as tiles: each friendly name with its state and unit, highlighted when on.",
            recommended: true,
            default_size: size(4, 2),
            min_size: size(3, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"Living room","value":"21.4 °C","glyph":"activity"},{"label":"Front door","value":"locked","glyph":"shield"},{"label":"Hall light","value":"on","glyph":"check","tone":"up"},{"label":"Solar power","value":"2.1 kW","glyph":"activity"}]}"#,
            fields: &[ENTITY_FIELD, REFRESH],
            fetch: overview,
            validate: validate_entities,
        },
        Widget {
            id: "entity_list",
            name: "Entity list",
            description: "The chosen entities as a list with on/off dots and their states, and how many are on, off or unavailable.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"2 on · 1 off · 0 unavailable"},{"title":"Hall light","secondary":"light.hall","value":"on","status":"up"},{"title":"Garage door","secondary":"cover.garage","value":"closed"},{"title":"Kitchen plug","secondary":"switch.kitchen","value":"off","status":"down"}]}"#,
            fields: &[ENTITY_FIELD, REFRESH],
            fetch: list,
            validate: validate_entities,
        },
    ],
};

fn entities(options: &Values) -> Result<Vec<&str>, &'static str> {
    let value = options.get(ENTITIES).ok_or(ENTITIES)?;
    if !ENTITY_FIELD.accepts(value) {
        return Err(ENTITIES);
    }
    let mut ids = Vec::new();
    for id in value.as_array().ok_or(ENTITIES)? {
        let id = id.as_str().ok_or(ENTITIES)?;
        let (domain, name) = id.split_once('.').ok_or(ENTITIES)?;
        if [domain, name].iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        }) || ids.contains(&id)
        {
            return Err(ENTITIES);
        }
        ids.push(id);
    }
    Ok(ids)
}

fn validate_entities(options: &Values) -> Result<(), &'static str> {
    entities(options).map(|_| ())
}

async fn get(conn: &Conn<'_>, path: &str) -> Result<client::Response, Fault> {
    conn.request(Step {
        auth: &[Auth::Bearer(Input::Credential(TOKEN))],
        ..Step::get(path)
    })
    .await
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        #[derive(Deserialize)]
        struct Api {
            message: String,
        }
        let api: Api = parse_json(get(conn, "api/").await?)?;
        if api.message != "API running." {
            return Err(Class::Protocol.into());
        }
        Ok(())
    })
}

#[derive(Deserialize)]
struct Entity {
    entity_id: String,
    state: String,
    attributes: Attributes,
}

#[derive(Deserialize)]
struct Attributes {
    unit_of_measurement: Option<String>,
    friendly_name: Option<String>,
}

fn unavailable(state: &str) -> bool {
    matches!(state, "unknown" | "unavailable")
}

/// A generic glyph for an entity domain.
fn glyph(id: &str, state: &str) -> Glyph {
    match id.split_once('.').map_or("", |(domain, _)| domain) {
        _ if unavailable(state) => Glyph::Alert,
        "light" | "switch" | "input_boolean" | "fan" | "binary_sensor" if state == "on" => {
            Glyph::Check
        }
        "lock" | "alarm_control_panel" => Glyph::Shield,
        "media_player" => Glyph::Play,
        "person" | "device_tracker" => Glyph::Users,
        "camera" | "image" => Glyph::Image,
        "calendar" => Glyph::Calendar,
        "update" => Glyph::Download,
        "weather" | "sun" => Glyph::Globe,
        _ => Glyph::Activity,
    }
}

/// An entity's display name, reading and on/off health.
struct Reading {
    id: String,
    name: String,
    value: String,
    health: Option<Health>,
    glyph: Glyph,
}

async fn state(conn: &Conn<'_>, id: &str) -> Result<Option<Entity>, Fault> {
    let response = within_response_limit(get(conn, &format!("api/states/{id}")).await)?;
    match response {
        None => Ok(None),
        Some(response) if response.status == 404 => Ok(None),
        Some(response) => {
            let entity: Entity = parse_json(response)?;
            if entity.entity_id != id {
                return Err(Class::Protocol.into());
            }
            Ok(Some(entity))
        }
    }
}

async fn readings(conn: &Conn<'_>, options: &Values) -> Result<Vec<Reading>, Fault> {
    let ids = entities(options).map_err(|_| Class::Protocol)?;
    let mut readings = Vec::with_capacity(ids.len());
    for id in ids {
        readings.push(match state(conn, id).await? {
            Some(entity) => {
                let state = entity.state;
                let health = match state.as_str() {
                    "on" | "open" | "unlocked" | "playing" | "home" => Some(Health::Up),
                    "off" => Some(Health::Down),
                    s if unavailable(s) => Some(Health::Warn),
                    _ => None,
                };
                let glyph = glyph(id, &state);
                let value = match entity.attributes.unit_of_measurement {
                    Some(unit) if !unavailable(&state) => format!("{state} {unit}"),
                    _ => state,
                };
                let name = entity
                    .attributes
                    .friendly_name
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| id.into());
                Reading {
                    id: id.into(),
                    name: display_text(conn, name),
                    value: display_text(conn, value),
                    health,
                    glyph,
                }
            }
            None => Reading {
                id: id.into(),
                name: id.into(),
                value: "Unavailable".into(),
                health: Some(Health::Warn),
                glyph: Glyph::Alert,
            },
        });
    }
    Ok(readings)
}

fn overview<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let tiles = readings(conn, options)
            .await?
            .into_iter()
            .map(|reading| StatTile {
                label: reading.name,
                value: reading.value,
                glyph: Some(reading.glyph),
                delta: None,
                // Only "on" and problems stand out; off is the quiet default.
                tone: reading.health.filter(|h| *h != Health::Down),
                series: Vec::new(),
            })
            .collect();
        Ok(StructuredBody::Tiles { tiles })
    })
}

fn list<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let readings = readings(conn, options).await?;
        let count = |health| readings.iter().filter(|r| r.health == Some(health)).count();
        let summary = format!(
            "{} on · {} off · {} unavailable",
            readings.iter().filter(|r| r.value == "on").count(),
            count(Health::Down),
            readings
                .iter()
                .filter(|r| r.health == Some(Health::Warn))
                .count(),
        );
        let mut rows = vec![BodyRow {
            title: summary,
            ..Default::default()
        }];
        rows.extend(readings.into_iter().map(|reading| BodyRow {
            title: reading.name,
            secondary: Some(reading.id),
            value: Some(reading.value),
            status: reading.health,
            ..Default::default()
        }));
        Ok(StructuredBody::Rows { rows })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn reads_only_selected_entities_and_keeps_other_states_when_one_is_oversize() {
        let oversized = "x".repeat(MAX_RESPONSE + 1);
        let long_state = json!({
            "entity_id": "sensor.long",
            "state": "x".repeat(MAX_RESPONSE / 2),
            "attributes": {},
        })
        .to_string();
        let exchanges = [
            Exchange {
                request: "GET /prefix/api/ HTTP/1.1",
                status: 200,
                body: r#"{"message":"API running."}"#,
            },
            Exchange {
                request: "GET /prefix/api/states/light.hall HTTP/1.1",
                status: 200,
                body: r#"{"entity_id":"light.hall","state":"on","attributes":{}}"#,
            },
            Exchange {
                request: "GET /prefix/api/states/sensor.temp HTTP/1.1",
                status: 200,
                body: r#"{"entity_id":"sensor.temp","state":"21","attributes":{"unit_of_measurement":"°C","friendly_name":"Living room"}}"#,
            },
            Exchange {
                request: "GET /prefix/api/states/sensor.large HTTP/1.1",
                status: 200,
                body: &oversized,
            },
            Exchange {
                request: "GET /prefix/api/states/sensor.long HTTP/1.1",
                status: 200,
                body: &long_state,
            },
            Exchange {
                request: "GET /prefix/api/states/light.hall HTTP/1.1",
                status: 200,
                body: r#"{"entity_id":"light.hall","state":"off","attributes":{"friendly_name":"Hall light"}}"#,
            },
            Exchange {
                request: "GET /prefix/api/states/sensor.gone HTTP/1.1",
                status: 404,
                body: r#"{"message":"Entity not found."}"#,
            },
            Exchange {
                request: "GET /prefix/api/states/light.other HTTP/1.1",
                status: 200,
                body: r#"{"entity_id":"light.hall","state":"on","attributes":{}}"#,
            },
            Exchange {
                request: "GET /prefix/api/states/light.hall HTTP/1.1",
                status: 401,
                body: "",
            },
            Exchange {
                request: "GET /prefix/api/ HTTP/1.1",
                status: 401,
                body: "",
            },
            Exchange {
                request: "GET /prefix/api/ HTTP/1.1",
                status: 200,
                body: "{}",
            },
        ];
        exercise(
            &HOME_ASSISTANT,
            &[(TOKEN, "fake-ha-token")],
            Values::new(),
            &exchanges,
            |_, wire| {
                assert_header(wire, "authorization: Bearer fake-ha-token", "fake-ha-token");
                ""
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let options = Values::from_iter([(
                    ENTITIES.into(),
                    json!(["light.hall", "sensor.temp", "sensor.large", "sensor.long"]),
                )]);
                let body = super::super::fetch(conn, &HOME_ASSISTANT.widgets[0], &options)
                    .await
                    .unwrap();
                let StructuredBody::Tiles { tiles } = &body else {
                    panic!("expected entity tiles");
                };
                assert!(tiles[3].value.capacity() < 1024);
                assert_eq!(
                    serde_json::to_value(body).unwrap()["tiles"],
                    json!([
                        {"label":"light.hall","value":"on","glyph":"check","tone":"up"},
                        {"label":"Living room","value":"21 °C","glyph":"activity"},
                        {"label":"sensor.large","value":"Unavailable","glyph":"alert","tone":"warn"},
                        {"label":"sensor.long","value":format!("{}…", "x".repeat(160)),"glyph":"activity"}
                    ])
                );
                let options = Values::from_iter([(
                    ENTITIES.into(),
                    json!(["light.hall", "sensor.gone"]),
                )]);
                let body = super::super::fetch(conn, &HOME_ASSISTANT.widgets[1], &options)
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(body).unwrap()["rows"],
                    json!([
                        {"title":"0 on · 1 off · 1 unavailable"},
                        {"title":"Hall light","secondary":"light.hall","value":"off","status":"down"},
                        {"title":"sensor.gone","secondary":"sensor.gone","value":"Unavailable","status":"warn"}
                    ])
                );
                // A state for another entity than the one asked for is refused.
                let options = Values::from_iter([(ENTITIES.into(), json!(["light.other"]))]);
                let fault = super::super::fetch(conn, &HOME_ASSISTANT.widgets[1], &options)
                    .await
                    .unwrap_err();
                assert_eq!(fault.class.message(), Class::Protocol.message());
                let options = Values::from_iter([(ENTITIES.into(), json!(["light.hall"]))]);
                let fault = super::super::fetch(conn, &HOME_ASSISTANT.widgets[0], &options)
                    .await
                    .unwrap_err();
                assert_eq!(fault.class.message(), Class::Auth.message());
                assert_test_fault(conn, &HOME_ASSISTANT, Class::Auth).await;
                assert_test_fault(conn, &HOME_ASSISTANT, Class::Protocol).await;
            },
        )
        .await;
    }

    #[test]
    fn rejects_duplicates_and_non_entity_paths() {
        for ids in [
            json!(["light.hall", "light.hall"]),
            json!(["light/../hall"]),
        ] {
            assert_eq!(
                validate_entities(&Values::from_iter([(ENTITIES.into(), ids)])),
                Err(ENTITIES)
            );
        }
    }
}
