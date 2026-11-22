use super::*;
use widgets::StructuredBody;

const ZONES: Field = Field {
    required: true,
    max: Some(8),
    max_length: Some(120),
    default: Some(r#"["UTC | UTC"]"#),
    ..Field::new("zones", "Time zones", Kind::List, "Up to eight lines. Enter an IANA zone, optionally followed by | and a label, e.g. Europe/Paris | Paris.")
};

pub(super) const CLOCK: Provider = Provider {
    id: "clock",
    name: "Clock and calendar",
    category: "Utilities",
    icon: "clock",
    description: "Show local times for multiple zones and an optional month calendar.",
    docs: "https://www.iana.org/time-zones",
    public_internet: false,
    contacts: "None. Times and calendar are computed by your browser.",
    fields: &[ENABLED],
    test: super::local_test,
    widgets: &[Widget {
        id: "clock",
        name: "World clock",
        description: "Up to eight zones. The optional calendar follows the first valid zone.",
        recommended: true,
        fields: &[
            ZONES,
            Field {
                default: Some("false"),
                ..Field::new("hour12", "12-hour clock", Kind::Bool, "")
            },
            Field {
                default: Some("false"),
                ..Field::new("calendar", "Month calendar", Kind::Bool, "")
            },
        ],
        validate,
        fetch,
    }],
};

fn zones(options: &Values) -> Result<Vec<(String, String)>, &'static str> {
    let Some(value) = options.get(ZONES.id) else {
        return Ok(vec![("UTC".into(), "UTC".into())]);
    };
    if !ZONES.accepts(value) {
        return Err(ZONES.id);
    }
    value
        .as_array()
        .ok_or(ZONES.id)?
        .iter()
        .map(|line| {
            let line = line.as_str().ok_or(ZONES.id)?;
            let (zone, label) = line.split_once('|').unwrap_or((line, line));
            let (zone, label) = (zone.trim(), label.trim());
            if zone.len() > 64
                || !zone.split('/').all(|part| {
                    part.starts_with(|c: char| c.is_ascii_alphabetic())
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_+-".contains(&b))
                })
                || label.is_empty()
                || label.len() > 48
                || label.chars().any(char::is_control)
            {
                return Err(ZONES.id);
            }
            Ok((zone.into(), label.into()))
        })
        .collect()
}

fn validate(options: &Values) -> Result<(), &'static str> {
    zones(options).map(|_| ())
}

fn fetch<'a>(_: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        Ok(StructuredBody::Clock {
            zones: zones(options).map_err(|_| Class::Protocol)?,
            hour12: options
                .get("hour12")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            calendar: options
                .get("calendar")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_zone_paths_labels_and_count() {
        let options = |value| Values::from_iter([("zones".into(), value)]);
        assert_eq!(
            zones(&options(json!(["America/New_York | Home", "Etc/GMT+1"]))).unwrap(),
            [
                ("America/New_York".into(), "Home".into()),
                ("Etc/GMT+1".into(), "Etc/GMT+1".into())
            ]
        );
        for invalid in [
            json!(["../UTC"]),
            json!(["UTC | "]),
            json!(["Europe//Paris"]),
            json!(vec!["UTC"; 9]),
        ] {
            assert_eq!(validate(&options(invalid)), Err("zones"));
        }
    }
}
