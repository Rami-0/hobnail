use super::*;
use widgets::StructuredBody;

const URL: &str = "https://api.open-meteo.com";
const URLS: &[&str] = &[URL, "https://api.open-meteo.com/"];
const LATITUDE: Field = Field {
    required: true,
    max_length: Some(24),
    max: Some(90),
    ..Field::new(
        "latitude",
        "Latitude",
        Kind::Decimal,
        "Decimal degrees from −90 to 90.",
    )
};
const LONGITUDE: Field = Field {
    required: true,
    max_length: Some(24),
    max: Some(180),
    ..Field::new(
        "longitude",
        "Longitude",
        Kind::Decimal,
        "Decimal degrees from −180 to 180.",
    )
};

pub(super) const WEATHER: Provider = Provider {
    id: "weather",
    name: "Weather (Open-Meteo)",
    category: "Utilities",
    icon: "open-meteo",
    color: "#f59e0b",
    description: "Show the current weather and daily highs and lows for coordinates you enter.",
    docs: "https://open-meteo.com/en/docs",
    public_internet: true,
    contacts: "api.open-meteo.com receives your latitude and longitude. No key is needed for the free API.",
    fields: &[
        Field { label: "API URL", default: Some(URL), choices: URLS, help: "Use the Open-Meteo forecast API.", ..SERVICE_URL },
        LATITUDE, LONGITUDE, ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "forecast",
        name: "Weather forecast",
        description: "Current temperature, conditions and today's high and low, with an optional three-day outlook.",
        recommended: true,
        default_size: size(4, 2),
        min_size: size(3, 2),
        sample: r#"{"kind":"stat_grid","rows":[["Now","14 °C · Partly cloudy"],["Today","9–16 °C"],["Wind","12 km/h"],["Rain","20%"]]}"#,
        fields: &[
            Field { default: Some("°C"), choices: &["°C", "°F"], ..Field::new("units", "Temperature units", Kind::Select, "") },
            Field { default: Some("false"), ..Field::new("outlook", "Three-day forecast", Kind::Bool, "") },
            Field { min: Some(900), default: Some("900"), ..PUBLIC_REFRESH },
        ],
        validate: no_widget_options,
        fetch,
    }],
};

#[derive(Deserialize)]
struct Forecast {
    current: Current,
    daily: Daily,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: Option<f64>,
    weather_code: Option<u16>,
}

#[derive(Deserialize)]
struct Daily {
    time: Vec<String>,
    temperature_2m_max: Vec<Option<f64>>,
    temperature_2m_min: Vec<Option<f64>>,
    weather_code: Vec<Option<u16>>,
}

fn condition(code: Option<u16>) -> &'static str {
    match code {
        Some(0) => "Clear",
        Some(1) => "Mainly clear",
        Some(2) => "Partly cloudy",
        Some(3) => "Overcast",
        Some(45 | 48) => "Fog",
        Some(51 | 53 | 55) => "Drizzle",
        Some(56 | 57) => "Freezing drizzle",
        Some(61 | 63 | 65) => "Rain",
        Some(66 | 67) => "Freezing rain",
        Some(71 | 73 | 75 | 77) => "Snow",
        Some(80..=82) => "Rain showers",
        Some(85 | 86) => "Snow showers",
        Some(95) => "Thunderstorm",
        Some(97) => "Heavy thunderstorm",
        Some(96 | 99) => "Thunderstorm with hail",
        _ => "Conditions unavailable",
    }
}

fn coordinate<'a>(conn: &'a Conn<'_>, field: Field) -> Result<&'a str, Fault> {
    conn.options()
        .get(field.id)
        .filter(|value| field.accepts(value))
        .and_then(Value::as_str)
        .map(str::trim)
        .ok_or_else(|| Class::Protocol.into())
}

async fn forecast(
    conn: &Conn<'_>,
    fahrenheit: bool,
    outlook: bool,
) -> Result<StructuredBody, Fault> {
    let latitude = coordinate(conn, LATITUDE)?;
    let longitude = coordinate(conn, LONGITUDE)?;
    let (units, symbol) = if fahrenheit {
        ("fahrenheit", "°F")
    } else {
        ("celsius", "°C")
    };
    let days = if outlook { 3 } else { 1 };
    let count = days.to_string();
    let forecast: Forecast = parse_json(
        conn.request(Step {
            query: &[
                ("latitude", Input::Text(latitude)),
                ("longitude", Input::Text(longitude)),
                ("current", Input::Text("temperature_2m,weather_code")),
                (
                    "daily",
                    Input::Text("temperature_2m_max,temperature_2m_min,weather_code"),
                ),
                ("timezone", Input::Text("auto")),
                ("temperature_unit", Input::Text(units)),
                ("forecast_days", Input::Text(&count)),
            ],
            ..Step::get("v1/forecast")
        })
        .await?,
    )?;
    let daily = forecast.daily;
    if [
        daily.time.len(),
        daily.temperature_2m_max.len(),
        daily.temperature_2m_min.len(),
        daily.weather_code.len(),
    ]
    .iter()
    .any(|len| *len != days)
    {
        return Err(Class::Protocol.into());
    }
    let degrees = |value: Option<f64>| {
        value.map_or_else(
            || "Unavailable".into(),
            |temp| format!("{temp:.1} {symbol}"),
        )
    };
    let mut rows = vec![(
        "Now".into(),
        format!(
            "{} · {}",
            degrees(forecast.current.temperature_2m),
            condition(forecast.current.weather_code)
        ),
    )];
    for index in 0..days {
        let label = if index == 0 {
            "Today".into()
        } else {
            display_text(conn, daily.time[index].clone())
        };
        rows.push((
            label,
            format!(
                "High {} · Low {} · {}",
                degrees(daily.temperature_2m_max[index]),
                degrees(daily.temperature_2m_min[index]),
                condition(daily.weather_code[index]),
            ),
        ));
    }
    Ok(StructuredBody::StatGrid { rows })
}

fn fetch<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        if !conn.base_url_is_one_of(URLS) {
            return Err(Class::Blocked.into());
        }
        forecast(
            conn,
            options.get("units").and_then(Value::as_str) == Some("°F"),
            options.get("outlook").and_then(Value::as_bool) == Some(true),
        )
        .await
    })
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { fetch(conn, &Values::new()).await.map(|_| ()) })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn fetches_current_and_local_day_weather_in_selected_units() {
        let options = Values::from_iter([
            ("latitude".into(), json!("52.52")),
            ("longitude".into(), json!("-13.41")),
        ]);
        let wire = "GET /prefix/v1/forecast?latitude=52.52&longitude=-13.41&current=temperature_2m%2Cweather_code&daily=temperature_2m_max%2Ctemperature_2m_min%2Cweather_code&timezone=auto&temperature_unit=fahrenheit&forecast_days=3 HTTP/1.1";
        let response = r#"{"current":{"temperature_2m":62.4,"weather_code":2},"daily":{"time":["2026-11-22","2026-11-23","2026-11-24"],"temperature_2m_max":[63,64,61],"temperature_2m_min":[50,49,null],"weather_code":[2,45,95]}}"#;
        exercise(
            &WEATHER,
            &[],
            options,
            &[Exchange {
                request: wire,
                status: 200,
                body: response,
            }],
            |_, _| "",
            async |conn| {
                let body = forecast(conn, true, true).await.unwrap();
                let json = serde_json::to_value(body).unwrap();
                assert_eq!(json["rows"][0][1], "62.4 °F · Partly cloudy");
                assert_eq!(
                    json["rows"][1][1],
                    "High 63.0 °F · Low 50.0 °F · Partly cloudy"
                );
                assert_eq!(
                    json["rows"][3][1],
                    "High 61.0 °F · Low Unavailable · Thunderstorm"
                );
            },
        )
        .await;
    }

    #[tokio::test]
    async fn malformed_forecasts_are_protocol_faults() {
        let options = Values::from_iter([
            ("latitude".into(), json!("52.52")),
            ("longitude".into(), json!("13.41")),
        ]);
        let bodies = [
            // A daily series shorter than forecast_days.
            r#"{"current":{"temperature_2m":5,"weather_code":3},"daily":{"time":["2026-11-22"],"temperature_2m_max":[],"temperature_2m_min":[1],"weather_code":[3]}}"#,
            r#"{"current":{"temperature_2m":"warm","weather_code":3},"daily":{"time":["2026-11-22"],"temperature_2m_max":[6],"temperature_2m_min":[1],"weather_code":[3]}}"#,
            r#"{"daily":{"time":["2026-11-22"],"temperature_2m_max":[6],"temperature_2m_min":[1],"weather_code":[3]}}"#,
            r#"{"error":true,"reason":"Cannot initialize WeatherVariable from invalid String value"}"#,
            "<html>Bad gateway</html>",
        ];
        let exchanges = bodies.map(|body| Exchange {
            request: "GET /prefix/v1/forecast?*",
            status: 200,
            body,
        });
        exercise(
            &WEATHER,
            &[],
            options,
            &exchanges,
            |_, _| "",
            async |conn| {
                for _ in bodies {
                    let fault = forecast(conn, false, false).await.unwrap_err();
                    assert!(matches!(fault.class, Class::Protocol));
                }
            },
        )
        .await;
    }

    #[test]
    fn checks_coordinates_fixed_origin_and_refresh_floor() {
        assert!(LATITUDE.accepts(&json!("-90")));
        for invalid in ["91", "NaN", "1,2", "inf"] {
            assert!(!LATITUDE.accepts(&json!(invalid)));
        }
        assert!(!WEATHER.fields[0].accepts(&json!("https://example.test")));
        assert_eq!(
            WEATHER.widgets[0].refresh(&Values::new(), true).as_secs(),
            900
        );
    }
}
