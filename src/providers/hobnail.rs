//! Hobnail's own widgets: drawn by the browser from data Hobnail already
//! collects and streams (host vitals, the container list, the history
//! database and container logs). The server only validates the options and
//! names the view; nothing is contacted and nothing extra is kept in memory.

use super::*;
use widgets::{HobnailOptions, StructuredBody};

const SORT: Field = Field {
    default: Some("cpu"),
    choices: &["cpu", "memory"],
    ..Field::new("sort", "Sort by", Kind::Select, "")
};
const COUNT: Field = Field {
    default: Some("6"),
    min: Some(3),
    max: Some(12),
    ..Field::new("count", "Containers", Kind::Number, "How many to list.")
};
const CONTAINER: Field = Field {
    max_length: Some(128),
    ..Field::new(
        "container",
        "Container",
        Kind::Text,
        "The container name. Leave empty for the busiest running container.",
    )
};
const LINES: Field = Field {
    default: Some("12"),
    min: Some(5),
    max: Some(30),
    ..Field::new("lines", "Lines", Kind::Number, "")
};
const METRIC: Field = Field {
    default: Some("cpu"),
    choices: &[
        "cpu", "mem", "disk", "load1", "net_rx", "net_tx", "temp", "swap",
    ],
    ..Field::new("metric", "Metric", Kind::Select, "")
};
const RANGE: Field = Field {
    default: Some("6h"),
    choices: &["1h", "6h", "24h"],
    ..Field::new("range", "Range", Kind::Select, "")
};

pub(super) const HOBNAIL: Provider = Provider {
    id: "hobnail",
    name: "Hobnail",
    category: "This host",
    icon: "hobnail",
    color: "#f77848",
    detect: None,
    description: "Live widgets from what Hobnail already watches: host vitals, containers, stacks, events, logs and history.",
    docs: "https://github.com/Rami-0/hobnail/blob/main/docs/features/boards.md#hobnail-widgets",
    public_internet: false,
    contacts: "None. Drawn from Hobnail's own host, container, history and log data.",
    fields: &[ENABLED],
    test: super::local_test,
    widgets: &[
        Widget {
            id: "host",
            name: "Host overview",
            description: "CPU, memory and network with live trends, load and uptime, and the fullest disks as meters.",
            recommended: true,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"meters","rows":[{"label":"CPU","percent":18,"detail":"4 cores · load 0.84"},{"label":"Memory","percent":42,"detail":"6.7 of 16 GiB"},{"label":"/","percent":54,"detail":"507 of 935 GiB"},{"label":"/srv","percent":81,"detail":"3.2 of 4 TiB"}]}"#,
            fields: &[],
            fetch: host,
            validate: no_widget_options,
        },
        Widget {
            id: "containers",
            name: "Top containers",
            description: "The busiest running containers by CPU or memory, with their status.",
            recommended: false,
            default_size: size(4, 4),
            min_size: size(3, 3),
            sample: r#"{"kind":"rows","rows":[{"title":"media-server","secondary":"media · 1.2 GiB","value":"23%","status":"up","progress":23},{"title":"database","secondary":"apps · 640 MiB","value":"9%","status":"up","progress":9},{"title":"proxy","secondary":"edge · 48 MiB","value":"2%","status":"up","progress":2}]}"#,
            fields: &[SORT, COUNT],
            fetch: containers,
            validate: no_widget_options,
        },
        Widget {
            id: "stacks",
            name: "Stack health",
            description: "Each compose stack with how many of its containers run.",
            recommended: false,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"rows","rows":[{"title":"media","secondary":"4 of 4 running","status":"up","progress":100},{"title":"apps","secondary":"2 of 3 running","status":"warn","progress":66},{"title":"backup","secondary":"0 of 1 running","status":"down","progress":0}]}"#,
            fields: &[],
            fetch: stacks,
            validate: no_widget_options,
        },
        Widget {
            id: "events",
            name: "Recent events",
            description: "Container starts, stops, crashes and health changes of the last 24 hours, newest first.",
            recommended: false,
            default_size: size(4, 3),
            min_size: size(3, 2),
            sample: r#"{"kind":"rows","rows":[{"title":"database","secondary":"Restarted","value":"5 min ago","status":"warn"},{"title":"worker","secondary":"Exited with code 1","value":"1 h ago","status":"down"},{"title":"proxy","secondary":"Started","value":"3 h ago","status":"up"}]}"#,
            fields: &[],
            fetch: events,
            validate: no_widget_options,
        },
        Widget {
            id: "logs",
            name: "Log tail",
            description: "The latest lines of one container's log, refreshed every 20 seconds.",
            recommended: false,
            default_size: size(6, 4),
            min_size: size(4, 3),
            sample: r#"{"kind":"rich_text","blocks":[{"kind":"code","text":"12:04:01 GET /api/health 200 3ms\n12:04:09 GET /api/items 200 18ms\n12:04:15 worker: job 4411 done"}]}"#,
            fields: &[CONTAINER, LINES],
            fetch: logs,
            validate: no_widget_options,
        },
        Widget {
            id: "history",
            name: "History chart",
            description: "One host metric over the last hour, 6 or 24 hours, from Hobnail's history.",
            recommended: false,
            default_size: size(6, 3),
            min_size: size(4, 2),
            sample: r#"{"kind":"tiles","tiles":[{"label":"CPU, last 6 h","value":"18%","glyph":"cpu","delta":"peak 64%","series":[12,15,22,18,31,24,19,64,21,18,16,20,22,19]}]}"#,
            fields: &[METRIC, RANGE],
            fetch: history,
            validate: no_widget_options,
        },
    ],
};

fn text(options: &Values, field: &Field) -> Option<String> {
    options
        .get(field.id)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(field.default)
        .map(str::to_owned)
}

fn number(options: &Values, field: &Field) -> Option<u8> {
    options
        .get(field.id)
        .and_then(Value::as_u64)
        .or_else(|| field.default.and_then(|d| d.parse().ok()))
        .map(|n| {
            n.clamp(
                u64::from(field.min.unwrap_or(0)),
                u64::from(field.max.unwrap_or(255)),
            ) as u8
        })
}

fn view(view: &str, options: HobnailOptions) -> Job<'static, StructuredBody> {
    let view = view.to_owned();
    Box::pin(async move { Ok(StructuredBody::Hobnail { view, options }) })
}

fn host<'a>(_: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    view("host", HobnailOptions::default())
}

fn containers<'a>(_: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    view(
        "containers",
        HobnailOptions {
            sort: text(options, &SORT),
            count: number(options, &COUNT),
            ..Default::default()
        },
    )
}

fn stacks<'a>(_: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    view("stacks", HobnailOptions::default())
}

fn events<'a>(_: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
    view("events", HobnailOptions::default())
}

fn logs<'a>(_: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    view(
        "logs",
        HobnailOptions {
            container: text(options, &CONTAINER),
            count: number(options, &LINES),
            ..Default::default()
        },
    )
}

fn history<'a>(_: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    view(
        "history",
        HobnailOptions {
            metric: text(options, &METRIC),
            range: text(options, &RANGE),
            ..Default::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn names_the_view_with_validated_defaults() {
        super::super::batch_test::exercise(
            &HOBNAIL,
            &[],
            Values::new(),
            &[],
            |_, _| "",
            async |conn| {
                let get = async |id: &str, options: Value| {
                    let widget = HOBNAIL.widgets.iter().find(|w| w.id == id).unwrap();
                    let options = options.as_object().unwrap().clone();
                    serde_json::to_value(super::super::fetch(conn, widget, &options).await.unwrap())
                        .unwrap()
                };
                assert_eq!(
                    get("host", json!({})).await,
                    json!({"kind":"hobnail","view":"host","options":{}})
                );
                assert_eq!(
                    get("containers", json!({"sort":"memory"})).await,
                    json!({"kind":"hobnail","view":"containers","options":{"sort":"memory","count":6}})
                );
                assert_eq!(
                    get("logs", json!({"container":" dozzle ","lines":99})).await,
                    json!({"kind":"hobnail","view":"logs","options":{"container":"dozzle","count":30}})
                );
                assert_eq!(
                    get("history", json!({})).await,
                    json!({"kind":"hobnail","view":"history","options":{"metric":"cpu","range":"6h"}})
                );
            },
        )
        .await;
    }
}
