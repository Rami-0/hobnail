use super::*;
use crate::util::unix_now;
use repositories::REPOSITORIES;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use widgets::StructuredBody;

mod releases;

const URL: &str = "https://api.github.com";
const URLS: &[&str] = &[URL, "https://api.github.com/"];
const TOKEN: &str = "token";

pub const GITHUB: Provider = Provider {
    id: "github",
    name: "GitHub",
    category: "Development",
    icon: "github",
    description: "Show repository activity and watch releases for apps you run.",
    docs: "https://docs.github.com/en/rest",
    public_internet: true,
    contacts: "GitHub’s public REST API at https://api.github.com.",
    fields: &[
        Field {
            label: "API URL",
            default: Some(URL),
            choices: URLS,
            help: "Use https://api.github.com.",
            ..SERVICE_URL
        },
        Field {
            max_length: Some(4096),
            ..Field::new(
                TOKEN,
                "Fine-grained access token",
                Kind::Secret,
                "Optional for public repositories, which share an anonymous rate limit.",
            )
        },
        ENABLED,
    ],
    test,
    widgets: &[
        Widget {
            id: "repositories",
            name: "Repository activity",
            description: "Stars, issue and PR counts, and the latest repository workflow run.",
            recommended: true,
            fields: &[
                REPOSITORIES,
                Field {
                    default: Some("1800"),
                    ..PUBLIC_REFRESH
                },
            ],
            fetch: overview,
            validate: repositories::validate_options,
        },
        releases::WATCHER,
    ],
};

// Keep a short history after expiry so repeated secondary limits back off.
const MAX_COOLDOWNS: usize = 256;
const MAX_WAIT: u64 = 3600;
type Identity = Option<[u8; 32]>;

struct Cooldown {
    until: Instant,
    secondary_wait: u64,
}

#[derive(Default)]
struct Cooldowns {
    entries: Vec<(Identity, Cooldown)>,
    overflow_until: Option<Instant>,
}

#[derive(Default)]
struct RateLimits {
    cooldowns: Mutex<Cooldowns>,
}

static LIMITS: std::sync::LazyLock<RateLimits> = std::sync::LazyLock::new(RateLimits::default);

fn header<'a>(response: &'a client::Response, name: &str) -> Option<&'a str> {
    response.headers.get(name)?.to_str().ok()
}

fn numeric_header(response: &client::Response, name: &str) -> Option<u64> {
    let mut values = response.headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some()
        || value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

impl RateLimits {
    fn blocked(&self, identity: Identity, now: Instant) -> bool {
        let mut cooldowns = self.cooldowns.lock().unwrap();
        cooldowns
            .entries
            .retain(|(_, entry)| now < entry.until + Duration::from_secs(MAX_WAIT));
        if let Some((_, entry)) = cooldowns.entries.iter().find(|(key, _)| *key == identity) {
            return now < entry.until;
        }
        // Never evict an active cooldown to make room for another identity.
        cooldowns.overflow_until.is_some_and(|until| now < until)
            || (cooldowns.entries.len() == MAX_COOLDOWNS
                && cooldowns.entries.iter().all(|(_, entry)| now < entry.until))
    }

    fn update(&self, identity: Identity, now: Instant, delay: Option<u64>) {
        let mut cooldowns = self.cooldowns.lock().unwrap();
        let entries = &mut cooldowns.entries;
        let existing = entries.iter().position(|(key, _)| *key == identity);
        let previous = existing.map(|index| &entries[index].1);
        let secondary_wait = delay.is_none().then(|| {
            previous
                .map_or(60, |entry| entry.secondary_wait.saturating_mul(2).max(60))
                .min(MAX_WAIT)
        });
        let wait = delay.or(secondary_wait).unwrap().clamp(1, MAX_WAIT);
        let until = now + Duration::from_secs(wait);
        let entry = Cooldown {
            until: previous.map_or(until, |entry| until.max(entry.until)),
            secondary_wait: secondary_wait.unwrap_or(0),
        };
        if let Some(index) = existing {
            entries[index].1 = entry;
        } else if entries.len() < MAX_COOLDOWNS {
            entries.push((identity, entry));
        } else if let Some((index, _)) = entries
            .iter()
            .enumerate()
            .filter(|(_, (_, entry))| entry.until <= now)
            .min_by_key(|(_, (_, entry))| entry.until)
        {
            entries[index] = (identity, entry);
        } else {
            // Concurrent responses may fill the map after a request starts.
            // Keep their deadline for unknown identities rather than losing it.
            cooldowns.overflow_until = cooldowns.overflow_until.max(Some(until));
        }
    }

    async fn get(
        &self,
        conn: &Conn<'_>,
        path: &str,
        query: &[(&str, Input<'_>)],
    ) -> Result<client::Response, Fault> {
        let identity = conn.credential_revision_hash(TOKEN);
        let bearer = [Auth::Bearer(Input::Credential(TOKEN))];
        let auth = if identity.is_some() {
            bearer.as_slice()
        } else {
            &[]
        };
        let started = Instant::now();
        if self.blocked(identity, started) {
            return Err(Class::RateLimited.into());
        }
        let response = conn
            .request(Step {
                auth,
                query,
                accept_json: true,
                response_headers: &[
                    "link",
                    "retry-after",
                    "x-ratelimit-reset",
                    "x-ratelimit-remaining",
                ],
                ..Step::get(path)
            })
            .await?;
        let now = Instant::now();
        let retry = numeric_header(&response, "retry-after");
        let remaining_zero = numeric_header(&response, "x-ratelimit-remaining") == Some(0);
        let rate_limited = response.status == 429
            || (response.status == 403
                && (retry.is_some()
                    || remaining_zero
                    || response.text().to_ascii_lowercase().contains("rate limit")));
        let reset = if remaining_zero {
            numeric_header(&response, "x-ratelimit-reset")
                .and_then(|reset| reset.checked_sub(unix_now()))
                .filter(|seconds| *seconds > 0)
        } else {
            None
        };
        if rate_limited || remaining_zero {
            self.update(identity, now, retry.into_iter().chain(reset).max());
        }
        if rate_limited {
            return Err(Class::RateLimited.into());
        }
        Ok(response)
    }
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct Rate {
    rate: RateBucket,
}

#[derive(Deserialize)]
struct RateBucket {
    limit: u64,
}

#[derive(Deserialize)]
struct Repo {
    stargazers_count: u64,
    open_issues_count: u64,
}

#[derive(Deserialize)]
struct Pull {
    number: u64,
}

#[derive(Deserialize)]
struct Runs {
    workflow_runs: Vec<Run>,
}

#[derive(Deserialize)]
struct Run {
    status: String,
    conclusion: Option<String>,
}

async fn verify(conn: &Conn<'_>, limits: &RateLimits) -> Result<(), Fault> {
    let expected = if conn.has_credential(TOKEN) {
        let user: User = parse_json(limits.get(conn, "user", &[]).await?)?;
        !user.login.is_empty()
    } else {
        let rate: Rate = parse_json(limits.get(conn, "rate_limit", &[]).await?)?;
        rate.rate.limit > 0
    };
    if !expected {
        return Err(Class::Protocol.into());
    }
    Ok(())
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        if !conn.base_url_is_one_of(URLS) {
            return Err(Class::Blocked.into());
        }
        verify(conn, &LIMITS).await
    })
}

async fn optional(
    conn: &Conn<'_>,
    limits: &RateLimits,
    path: &str,
    query: &[(&str, Input<'_>)],
) -> Result<Option<client::Response>, Fault> {
    let response = within_response_limit(limits.get(conn, path, query).await)?;
    Ok(response.filter(|response| !matches!(response.status, 403 | 404 | 410)))
}

/// With one result per page, the last page number is the count. Never follow links.
fn last_page(link: &str) -> Option<u64> {
    link.split(',').find_map(|part| {
        let (url, rel) = part.trim().split_once('>')?;
        if rel.trim() != r#"; rel="last""# {
            return None;
        }
        let (_, query) = url.strip_prefix('<')?.split_once('?')?;
        query
            .split('&')
            .find_map(|param| param.strip_prefix("page="))?
            .parse()
            .ok()
    })
}

async fn pulls(conn: &Conn<'_>, limits: &RateLimits, repo: &str) -> Result<Option<u64>, Fault> {
    let Some(response) = optional(
        conn,
        limits,
        &format!("{repo}/pulls"),
        &[
            ("state", Input::Text("open")),
            ("per_page", Input::Text("1")),
        ],
    )
    .await?
    else {
        return Ok(None);
    };
    let link = header(&response, "link");
    let page = link.map(last_page);
    let items: Vec<Pull> = parse_json(response)?;
    if items.len() > 1 || items.iter().any(|pull| pull.number == 0) {
        return Err(Class::Protocol.into());
    }
    Ok(match page {
        Some(last) => last.filter(|count| *count >= items.len() as u64),
        None => Some(items.len() as u64),
    })
}

async fn workflow(conn: &Conn<'_>, limits: &RateLimits, repo: &str) -> Result<String, Fault> {
    let Some(response) = optional(
        conn,
        limits,
        &format!("{repo}/actions/runs"),
        &[("per_page", Input::Text("1"))],
    )
    .await?
    else {
        return Ok("Workflow unavailable".into());
    };
    let runs: Runs = parse_json(response)?;
    Ok(match runs.workflow_runs.into_iter().next() {
        Some(run) => {
            let value = run
                .conclusion
                .filter(|v| !v.is_empty())
                .unwrap_or(run.status);
            format!("Workflow {}", display_text(conn, value))
        }
        None => "No workflow runs".into(),
    })
}

async fn activity(
    conn: &Conn<'_>,
    limits: &RateLimits,
    options: &Values,
) -> Result<StructuredBody, Fault> {
    let names = repositories::names(options).map_err(|_| Class::Protocol)?;
    let mut rows = Vec::with_capacity(names.len());
    for name in names {
        let path = format!("repos/{name}");
        let value = if let Some(response) = optional(conn, limits, &path, &[]).await? {
            let repo: Repo = parse_json(response)?;
            let counts = match pulls(conn, limits, &path).await? {
                Some(prs) if prs <= repo.open_issues_count => {
                    format!("{} issues · {prs} PRs", repo.open_issues_count - prs)
                }
                Some(prs) => format!("Issue count unavailable · {prs} PRs"),
                None => format!("{} issues and PRs", repo.open_issues_count),
            };
            let workflow = workflow(conn, limits, &path).await?;
            format!("{} stars · {counts} · {workflow}", repo.stargazers_count)
        } else {
            "Repository unavailable".into()
        };
        rows.push((name.into(), value));
    }
    Ok(StructuredBody::StatGrid { rows })
}

fn overview<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        if !conn.base_url_is_one_of(URLS) {
            return Err(Class::Blocked.into());
        }
        activity(conn, &LIMITS, options).await
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{assert_header, assert_test_fault, exercise, Exchange};
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn counts_open_prs_and_preserves_workflow_and_stars_when_pulls_are_oversized() {
        const REPO: &str = "GET /prefix/repos/o/r HTTP/1.1";
        const PULLS: &str = "GET /prefix/repos/o/r/pulls?state=open&per_page=1 HTTP/1.1";
        const ACTIONS: &str = "GET /prefix/repos/o/r/actions/runs?per_page=1 HTTP/1.1";
        const USER: &str = "GET /prefix/user HTTP/1.1";
        const REPO_JSON: &str = r#"{"stargazers_count":17,"open_issues_count":9}"#;
        const RUN: &str = r#"{"workflow_runs":[{"status":"completed","conclusion":"success"}]}"#;
        let oversized = "x".repeat(MAX_RESPONSE + 1);
        let exchanges = [
            (USER, 200, r#"{"login":"user"}"#),
            (REPO, 200, REPO_JSON),
            (PULLS, 200, r#"[{"number":10}]"#),
            (ACTIONS, 200, RUN),
            (REPO, 200, REPO_JSON),
            (PULLS, 200, &oversized),
            (ACTIONS, 200, RUN),
            (USER, 401, ""),
            (USER, 200, "{}"),
            (REPO, 429, "{}"),
        ]
        .map(|(request, status, body)| Exchange {
            request,
            status,
            body,
        });
        let limits = RateLimits::default();
        exercise(&GITHUB, &[(TOKEN, "fake-gh-token")], Values::new(), &exchanges,
            |index, wire| {
                assert_header(wire, "authorization: Bearer fake-gh-token", "fake-gh-token");
                match index {
                    2 => "Link: <https://api.github.com/repos/o/r/pulls?state=open&per_page=1&page=2>; rel=\"next\", <https://api.github.com/repos/o/r/pulls?state=open&per_page=1&page=3>; rel=\"last\"\r\n",
                    9 => "Retry-After: 900\r\n",
                    _ => "",
                }
            },
            async |conn| {
                conn.run(verify(conn, &limits)).await.unwrap();
                let options = Values::from_iter([("repositories".into(), json!(["o/r"]))]);
                let fetch = async || conn.run(activity(conn, &limits, &options)).await;
                assert_eq!(serde_json::to_value(fetch().await.unwrap()).unwrap()["rows"], json!([
                    ["o/r","17 stars · 6 issues · 3 PRs · Workflow success"]
                ]));
                assert_eq!(serde_json::to_value(fetch().await.unwrap()).unwrap()["rows"], json!([
                    ["o/r","17 stars · 9 issues and PRs · Workflow success"]
                ]));
                assert!(matches!(verify(conn, &limits).await.unwrap_err().class, Class::Auth));
                assert!(matches!(verify(conn, &limits).await.unwrap_err().class, Class::Protocol));
                for _ in 0..2 {
                    assert!(matches!(fetch().await.unwrap_err().class, Class::RateLimited));
                }
                assert!(limits.blocked(conn.credential_revision_hash(TOKEN), Instant::now() + Duration::from_secs(890)));
                assert_test_fault(conn, &GITHUB, Class::Blocked).await;
            }
        ).await;
    }

    #[tokio::test]
    async fn anonymous_test_is_credential_free_and_honors_primary_reset() {
        let limits = RateLimits::default();
        let reset_headers = format!(
            "X-RateLimit-Remaining: 0\r\nX-RateLimit-Reset: {}\r\n",
            unix_now() + 120,
        );
        let path = "GET /prefix/rate_limit HTTP/1.1";
        exercise(
            &GITHUB,
            &[],
            Values::new(),
            &[
                Exchange {
                    request: path,
                    status: 200,
                    body: r#"{"rate":{"limit":60}}"#,
                },
                Exchange {
                    request: path,
                    status: 403,
                    body: "{}",
                },
            ],
            |index, wire| {
                assert!(!wire.contains("authorization:"));
                if index == 1 {
                    &reset_headers
                } else {
                    ""
                }
            },
            async |conn| {
                verify(conn, &limits).await.unwrap();
                for _ in 0..2 {
                    assert!(matches!(
                        verify(conn, &limits).await.unwrap_err().class,
                        Class::RateLimited
                    ));
                }
                let now = Instant::now();
                assert!(limits.blocked(None, now + Duration::from_secs(110)));
                assert!(!limits.blocked(None, now + Duration::from_secs(125)));
            },
        )
        .await;
    }

    #[tokio::test]
    async fn cooldown_covers_both_widgets_but_isolates_credentials() {
        let limits = RateLimits::default();
        let activity_options = Values::from_iter([("repositories".into(), json!(["o/r"]))]);
        let release_options = Values::from_iter([("tracked".into(), json!(["o/r | 1"]))]);
        exercise(&GITHUB, &[(TOKEN, "fixture-token-a")], Values::new(), &[
            Exchange {
                request: "GET /prefix/repos/o/r HTTP/1.1",
                status: 403,
                body: r#"{"message":"You have exceeded a secondary rate limit.","documentation_url":"https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api"}"#,
            },
        ], |_, _| "X-RateLimit-Remaining: 57\r\nX-RateLimit-Reset: 9999999999\r\nRetry-After: +900\r\n", async |conn| {
            assert!(matches!(activity(conn, &limits, &activity_options).await.unwrap_err().class, Class::RateLimited));
            assert!(matches!(releases::latest(conn, &limits, &release_options).await.unwrap_err().class, Class::RateLimited));
            let identity = conn.credential_revision_hash(TOKEN);
            let later = Instant::now() + Duration::from_secs(65);
            assert!(!limits.blocked(identity, later));
            limits.update(identity, later, None);
            assert!(limits.blocked(identity, later + Duration::from_secs(119)));
            assert!(!limits.blocked(identity, later + Duration::from_secs(121)));
        }).await;
        exercise(
            &GITHUB,
            &[(TOKEN, "fixture-token-b")],
            Values::new(),
            &[
                Exchange {
                    request: "GET /prefix/repos/o/r/releases/latest HTTP/1.1",
                    status: 200,
                    body: r#"{"tag_name":"v2"}"#,
                },
                Exchange {
                    request: "GET /prefix/repos/o/r/releases/latest HTTP/1.1",
                    status: 429,
                    body: r#"{"message":"API rate limit exceeded"}"#,
                },
            ],
            |index, _| {
                if index == 1 {
                    "Retry-After: 86400\r\n"
                } else {
                    ""
                }
            },
            async |conn| {
                releases::latest(conn, &limits, &release_options)
                    .await
                    .unwrap();
                assert!(matches!(
                    releases::latest(conn, &limits, &release_options)
                        .await
                        .unwrap_err()
                        .class,
                    Class::RateLimited
                ));
                let now = Instant::now();
                assert!(limits.blocked(
                    conn.credential_revision_hash(TOKEN),
                    now + Duration::from_secs(3590)
                ));
                assert!(!limits.blocked(
                    conn.credential_revision_hash(TOKEN),
                    now + Duration::from_secs(3601)
                ));
                assert!(!limits.blocked(None, now));
            },
        )
        .await;
    }

    #[test]
    fn applies_public_refresh_minimum_and_only_accepts_the_public_api_url() {
        assert!(super::super::get("github").unwrap().public_internet);
        assert!(GITHUB.fields[0].accepts(&json!(URL)));
        assert!(!GITHUB.fields[0].accepts(&json!("https://example.test")));
        let mut options = Values::from_iter([
            ("repositories".into(), json!(["o/r"])),
            ("refresh".into(), json!(599)),
        ]);
        assert_eq!(
            validate(GITHUB.widgets[0].fields, &mut options),
            Err("refresh".into())
        );
        assert_eq!(refresh(&options, true).as_secs(), 600);
    }
}
