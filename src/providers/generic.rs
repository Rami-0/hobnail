use super::*;
use crate::widgets::{shorten, template, Path, StructuredBody};

pub const HTTP: Provider = Provider {
    id: "http",
    name: "Generic HTTP",
    category: "Utilities",
    icon: "http",
    description: "Display values from a JSON endpoint you configure.",
    docs: "https://github.com/Rami-0/hobnail/blob/main/docs/features/integrations.md",
    public_internet: false,
    contacts: "The hostname and port you enter. Public addresses require internet access.",
    fields: &[
        Field {
            required: true,
            max_length: Some(2048),
            ..Field::new(
                "base_url",
                "Service URL",
                Kind::Url,
                "Enter the JSON endpoint reachable from Hobnail.",
            )
        },
        Field {
            max_length: Some(4096),
            ..Field::new(
                "secret",
                "Bearer credential",
                Kind::Secret,
                "Bound to the scheme, hostname, and port.",
            )
        },
        Field {
            default: Some("true"),
            ..Field::new("enabled", "Enabled", Kind::Bool, "")
        },
    ],
    test,
    widgets: &[Widget {
        id: "json",
        name: "JSON values",
        description: "Show selected fields from the response.",
        recommended: true,
        fields: &[
            Field {
                max_length: Some(160),
                ..Field::new(
                    "paths",
                    "JSON paths",
                    Kind::List,
                    "One per line, for example stats.users. Leave empty for a response preview.",
                )
            },
            REFRESH,
        ],
        validate: options,
        fetch,
    }],
};

fn test<'a>(target: &'a Target, policy: &'a HttpPolicy) -> Job<'a, ()> {
    Box::pin(async { target.json(policy).await.map(drop) })
}

fn options(values: &Values) -> Result<(), &'static str> {
    if values
        .get("paths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|path| Path::parse(path.as_str().expect("validated paths")).is_err())
    {
        return Err("paths");
    }
    Ok(())
}

fn fetch<'a>(
    target: &'a Target,
    policy: &'a HttpPolicy,
    options: &'a Values,
) -> Job<'a, StructuredBody> {
    Box::pin(async {
        let json = target.json(policy).await?;
        let rows: Vec<_> = options
            .get("paths")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|path| {
                Path::parse(path)
                    .ok()
                    .map(|parsed| (path.to_owned(), template::display(&parsed.eval(&json))))
            })
            .collect();
        Ok(if rows.is_empty() {
            StructuredBody::Stat {
                value: shorten(&json.to_string(), 512),
            }
        } else {
            StructuredBody::StatGrid { rows }
        })
    })
}
