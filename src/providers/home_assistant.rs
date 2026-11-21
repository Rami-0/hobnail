use super::*;
use widgets::StructuredBody;

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
    widgets: &[Widget {
        id: "entities",
        name: "Entity states",
        description: "See selected entities and how many are on or off.",
        recommended: true,
        fields: &[ENTITY_FIELD, REFRESH],
        fetch: overview,
        validate: validate_entities,
    }],
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

fn overview<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let ids = entities(options).map_err(|_| Class::Protocol)?;
        let mut rows = Vec::with_capacity(ids.len() + 1);
        let (mut on, mut off, mut unavailable) = (0, 0, 0);
        for id in ids {
            let value = if let Some(entity) = state(conn, id).await? {
                on += usize::from(entity.state == "on");
                off += usize::from(entity.state == "off");
                unavailable +=
                    usize::from(matches!(entity.state.as_str(), "unknown" | "unavailable"));
                let reading = match entity.attributes.unit_of_measurement {
                    Some(unit) if !matches!(entity.state.as_str(), "unknown" | "unavailable") => {
                        format!("{} {unit}", entity.state)
                    }
                    _ => entity.state,
                };
                display_text(conn, reading)
            } else {
                unavailable += 1;
                "Unavailable".into()
            };
            rows.push((id.into(), value));
        }
        rows.insert(
            0,
            (
                "Selected entities".into(),
                format!("{on} on · {off} off · {unavailable} unavailable"),
            ),
        );
        Ok(StructuredBody::StatGrid { rows })
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
                body: r#"{"entity_id":"sensor.temp","state":"21","attributes":{"unit_of_measurement":"°C"}}"#,
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
                let StructuredBody::StatGrid { rows } = &body else {
                    panic!("expected entity rows");
                };
                assert!(rows[4].1.capacity() < 1024);
                assert_eq!(
                    serde_json::to_value(body).unwrap()["rows"],
                    json!([
                        ["Selected entities", "1 on · 0 off · 1 unavailable"],
                        ["light.hall", "on"],
                        ["sensor.temp", "21 °C"],
                        ["sensor.large", "Unavailable"],
                        ["sensor.long", &format!("{}…", "x".repeat(160))]
                    ])
                );
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
