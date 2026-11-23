use super::*;
use std::cmp::Ordering as VersionOrder;
use widgets::BodyItem;

const TRACKED: Field = Field {
    required: true,
    max: Some(10),
    max_length: Some(208),
    ..Field::new(
        "tracked",
        "Repositories and current versions",
        Kind::List,
        "Up to ten lines, each with owner/repository | current version, e.g. owner/app | v1.2.3.",
    )
};

pub(super) const WATCHER: Widget = Widget {
    id: "releases",
    name: "GitHub releases watcher",
    description: "See whether GitHub has a newer release for the apps you run.",
    recommended: false,
    fields: &[
        TRACKED,
        Field {
            min: Some(3600),
            default: Some("3600"),
            ..PUBLIC_REFRESH
        },
    ],
    fetch,
    validate,
};

fn tracked(options: &Values) -> Result<Vec<(&str, &str)>, &'static str> {
    let key = TRACKED.id;
    let value = options
        .get(key)
        .filter(|value| TRACKED.accepts(value))
        .ok_or(key)?;
    let mut tracked: Vec<(&str, &str)> = Vec::new();
    for line in value.as_array().ok_or(key)? {
        let (name, version) = line
            .as_str()
            .and_then(|line| line.split_once('|'))
            .ok_or(key)?;
        let (name, version) = (name.trim(), version.trim());
        if name.len() > 140
            || !repositories::valid_name(name)
            || version.is_empty()
            || version.len() > 64
            || version.contains('|')
            || version.chars().any(char::is_control)
            || tracked
                .iter()
                .any(|(previous, _)| previous.eq_ignore_ascii_case(name))
        {
            return Err(key);
        }
        tracked.push((name, version));
    }
    Ok(tracked)
}

fn validate(options: &Values) -> Result<(), &'static str> {
    tracked(options).map(|_| ())
}

fn version(text: &str) -> Option<[u64; 3]> {
    let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
    let core = if let Some((core, build)) = text.split_once('+') {
        if !build.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) {
            return None;
        }
        core
    } else {
        text
    };
    let mut parts = [0; 3];
    for (index, piece) in core.split('.').enumerate() {
        if !piece.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *parts.get_mut(index)? = piece.parse().ok()?;
    }
    Some(parts)
}

fn compare(current: &str, latest: &str) -> &'static str {
    match version(current)
        .zip(version(latest))
        .map(|(current, latest)| current.cmp(&latest))
    {
        Some(VersionOrder::Less) => "New version available",
        Some(VersionOrder::Equal) => "Up to date",
        Some(VersionOrder::Greater) => "Current version is ahead",
        None => "Versions not comparable",
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: Option<String>,
}

async fn release(
    conn: &Conn<'_>,
    limits: &RateLimits,
    repo: &str,
    current: &str,
) -> Result<(String, Option<String>), Fault> {
    let path = format!("repos/{repo}/releases/latest");
    let Some(response) = optional(conn, limits, &path, &[]).await? else {
        return Ok((
            format!("Current {current} · Latest release unavailable"),
            None,
        ));
    };
    let release: Release = parse_json(response)?;
    if release.tag_name.trim().is_empty() {
        return Err(Class::Protocol.into());
    }
    let verdict = compare(current, &release.tag_name);
    let tag = display_text(conn, release.tag_name);
    Ok((
        format!("Current {current} · Latest {tag} · {verdict}"),
        release.html_url.filter(|url| {
            url.len() <= 512 && url.starts_with("https://github.com/") && navigation_link(url)
        }),
    ))
}

pub(super) async fn latest(
    conn: &Conn<'_>,
    limits: &RateLimits,
    options: &Values,
    deadline: tokio::time::Instant,
) -> Result<StructuredBody, Fault> {
    let repos = tracked(options).map_err(|_| Class::Protocol)?;
    let key = Rotation::key(conn, "releases", repos.iter().map(|(repo, _)| repo));
    let start = limits.rotation.start(key, repos.len());
    let mut results = vec![None; repos.len()];
    let mut reached = 0;
    for index in rotated(start, repos.len()) {
        let (repo, current) = repos[index];
        let Ok(result) =
            tokio::time::timeout_at(deadline, release(conn, limits, repo, current)).await
        else {
            break;
        };
        results[index] = Some(result?);
        reached += 1;
    }
    limits.rotation.advance(key, start, reached, repos.len());
    if reached == 0 && !repos.is_empty() {
        return Err(Class::Timeout.into());
    }
    let items = repos
        .into_iter()
        .zip(results)
        .map(|((repo, current), result)| {
            let (secondary, link) =
                result.unwrap_or_else(|| (format!("Current {current} · {NOT_REACHED}"), None));
            BodyItem {
                title: repo.into(),
                secondary: Some(secondary),
                link,
                icon: None,
                time: None,
            }
        })
        .collect();
    Ok(StructuredBody::ItemList { items })
}

fn fetch<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        if !conn.base_url_is_one_of(URLS) {
            return Err(Class::Blocked.into());
        }
        latest(conn, &LIMITS, options, budget()).await
    })
}

#[cfg(test)]
mod tests {
    use super::super::super::batch_test::{exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[test]
    fn compares_numeric_tags_conservatively() {
        assert_eq!(compare("v1.9", "1.10.0"), "New version available");
        assert_eq!(compare("1.10+abc", "V1.10.0"), "Up to date");
        assert_eq!(compare("2", "1.9"), "Current version is ahead");
        for tag in ["nightly", "1.2.3-rc.1", "1..0", "1.2.3.4", "1.2+A..B"] {
            assert_eq!(compare(tag, "1.2.3"), "Versions not comparable");
        }
        let invalid = Values::from_iter([("tracked".into(), json!(["O/r | 1", "o/R | 2"]))]);
        assert!(validate(&invalid).is_err());
        assert_eq!(WATCHER.refresh(&Values::new(), true).as_secs(), 3600);
    }

    #[tokio::test]
    async fn fetches_latest_releases_and_honors_the_shared_cooldown() {
        let limits = RateLimits::default();
        let values = |repos| Values::from_iter([("tracked".into(), repos)]);
        let path = "GET /prefix/repos/o/r/releases/latest HTTP/1.1";
        let oversized = json!({"tag_name":"v1","body":"x".repeat(128 * 1024)}).to_string();
        exercise(&GITHUB, &[], Values::new(), &[
            Exchange { request: "GET /prefix/repos/o/big/releases/latest HTTP/1.1", status: 200, body: &oversized },
            Exchange { request: path, status: 200, body: r#"{"tag_name":"v1.10.0","html_url":"https://github.com/o/r/releases/tag/v1.10.0"}"# },
            Exchange { request: "GET /prefix/repos/o/missing/releases/latest HTTP/1.1", status: 404, body: "{}" },
            Exchange { request: "GET /prefix/repos/o/private/releases/latest HTTP/1.1", status: 403, body: r#"{"message":"Resource not accessible by integration"}"# },
            Exchange { request: "GET /prefix/repos/o/gone/releases/latest HTTP/1.1", status: 410, body: "{}" },
            Exchange { request: "GET /prefix/repos/o/legal/releases/latest HTTP/1.1", status: 451, body: "{}" },
            Exchange { request: "GET /prefix/repos/o/big/releases/latest HTTP/1.1", status: 429, body: "{}" },
        ], |i, wire| {
            assert!(!wire.contains("authorization:"));
            if i == 6 { "Retry-After: 600\r\n" } else { "" }
        }, async |conn| {
            let options = values(json!(["o/big | 1", "o/r | 1.9", "o/missing | custom", "o/private | 1", "o/gone | 1", "o/legal | 1"]));
            let body = serde_json::to_value(latest(conn, &limits, &options, budget()).await.unwrap()).unwrap();
            assert_eq!(body["items"][0]["secondary"], "Current 1 · Latest release unavailable");
            assert_eq!(body["items"][1]["secondary"], "Current 1.9 · Latest v1.10.0 · New version available");
            assert_eq!(body["items"][2]["secondary"], "Current custom · Latest release unavailable");
            for index in 3..6 {
                assert_eq!(body["items"][index]["secondary"], "Current 1 · Latest release unavailable");
            }
            for _ in 0..2 {
                assert!(matches!(latest(conn, &limits, &options, budget()).await.unwrap_err().class, Class::RateLimited));
            }
        }).await;
    }
}
