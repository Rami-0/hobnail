use super::*;
use repositories::REPOSITORIES;
use widgets::StructuredBody;

const TOKEN: &str = "token";

pub const GITEA: Provider = Provider {
    id: "gitea",
    name: "Gitea / Forgejo",
    category: "Development",
    icon: "gitea",
    color: "#609926",
    description: "Show accessible repositories, open issues and pull requests, and Actions status.",
    docs: "https://docs.gitea.com/api/",
    public_internet: false,
    contacts: "Your configured Gitea or Forgejo server.",
    fields: &[
        SERVICE_URL,
        Field {
            required: true,
            max_length: Some(4096),
            ..Field::new(TOKEN, "Access token", Kind::Secret, "Use a personal access token with read:user and read:repository scopes and access to the selected repositories.")
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "repositories",
        name: "Repository activity",
        description: "Visible repository count, with issues, pull requests and Actions for selected repositories.",
        recommended: true,
        default_size: size(4, 2),
        min_size: size(3, 2),
        sample: r#"{"kind":"stat_grid","rows":[["Repositories","42"],["Open issues","17"],["Open pull requests","3"]]}"#,
        fields: &[REPOSITORIES, REFRESH],
        fetch: overview,
        validate: repositories::validate_options,
    }],
};

#[derive(Deserialize)]
struct Version {
    version: String,
}

#[derive(Deserialize)]
struct RepoName {
    full_name: String,
}

#[derive(Deserialize)]
struct RepoSearch {
    ok: bool,
    data: Vec<RepoName>,
}

#[derive(Deserialize)]
struct Repo {
    open_issues_count: u64,
    open_pr_counter: u64,
    has_actions: bool,
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

async fn get(
    conn: &Conn<'_>,
    path: &str,
    query: &[(&str, Input<'_>)],
) -> Result<client::Response, Fault> {
    conn.request(Step {
        auth: &[Auth::Authorization("token ", Input::Credential(TOKEN))],
        query,
        response_headers: &["x-total-count"],
        ..Step::get(path)
    })
    .await
}

const LIMIT_ONE: &[(&str, Input<'_>)] = &[("limit", Input::Text("1"))];

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move {
        let version: Version = parse_json(get(conn, "api/v1/version", &[]).await?)?;
        if version.version.is_empty() {
            return Err(Class::Protocol.into());
        }
        let result: Vec<RepoName> = parse_json(get(conn, "api/v1/user/repos", LIMIT_ONE).await?)?;
        if result.iter().any(|repo| repo.full_name.is_empty()) {
            return Err(Class::Protocol.into());
        }
        Ok(())
    })
}

async fn visible_repos(conn: &Conn<'_>) -> Result<String, Fault> {
    let Some(response) = within_response_limit(get(conn, "api/v1/repos/search", LIMIT_ONE).await)?
    else {
        return Ok("List too large".into());
    };
    let count = response
        .headers
        .get("x-total-count")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let search: RepoSearch = parse_json(response)?;
    if !search.ok || search.data.iter().any(|repo| repo.full_name.is_empty()) {
        return Err(Class::Protocol.into());
    }
    Ok(count.map_or_else(|| "Count unavailable".into(), |count| count.to_string()))
}

/// One repository the token cannot see (403), that is missing (404), removed
/// (410) or legally withheld (451) must not fail the other rows. A token
/// rejected outright already fails the repository search first.
fn unavailable(status: u16) -> bool {
    matches!(status, 403 | 404 | 410 | 451)
}

async fn actions(conn: &Conn<'_>, path: &str) -> Result<String, Fault> {
    let response = get(conn, &format!("{path}/actions/runs"), LIMIT_ONE).await;
    let (response, label) = match response {
        Ok(response) if response.status == 404 => (
            get(conn, &format!("{path}/actions/tasks"), LIMIT_ONE).await,
            "Last task",
        ),
        result => (result, "Last run"),
    };
    let Some(response) = within_response_limit(response)? else {
        return Ok("Actions response too large".into());
    };
    if unavailable(response.status) {
        return Ok("Actions unavailable".into());
    }
    let runs: Runs = parse_json(response)?;
    Ok(match runs.workflow_runs.into_iter().next() {
        Some(run) => format!(
            "{label}: {}",
            display_text(
                conn,
                run.conclusion
                    .filter(|v| !v.is_empty())
                    .unwrap_or(run.status)
            )
        ),
        None => "No Actions runs reported".into(),
    })
}

fn overview<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let names = repositories::names(options).map_err(|_| Class::Protocol)?;
        let mut rows = vec![("Visible repositories".into(), visible_repos(conn).await?)];
        for name in names {
            let path = format!("api/v1/repos/{name}");
            let response = within_response_limit(get(conn, &path, &[]).await)?;
            let summary = match response {
                None => "Repository response too large".into(),
                Some(response) if unavailable(response.status) => "Repository unavailable".into(),
                Some(response) => {
                    let repo: Repo = parse_json(response)?;
                    let actions = if repo.has_actions {
                        actions(conn, &path).await?
                    } else {
                        "Actions disabled".into()
                    };
                    format!(
                        "{} issues · {} PRs · {actions}",
                        repo.open_issues_count, repo.open_pr_counter
                    )
                }
            };
            rows.push((name.into(), summary));
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
    async fn reads_bounded_repo_counts_and_actions_with_task_fallback() {
        const VERSION: &str = "GET /prefix/api/v1/version HTTP/1.1";
        const USER_REPOS: &str = "GET /prefix/api/v1/user/repos?limit=1 HTTP/1.1";
        const SEARCH: &str = "GET /prefix/api/v1/repos/search?limit=1 HTTP/1.1";
        let oversized = "x".repeat(MAX_RESPONSE + 1);
        let exchanges = [
            (VERSION, 200, r#"{"version":"1.26.0"}"#),
            (USER_REPOS, 200, "[]"),
            (SEARCH, 200, r#"{"ok":true,"data":[{"full_name":"o/r"}]}"#),
            (
                "GET /prefix/api/v1/repos/o/r HTTP/1.1",
                200,
                r#"{"open_issues_count":2,"open_pr_counter":3,"has_actions":true}"#,
            ),
            (
                "GET /prefix/api/v1/repos/o/r/actions/runs?limit=1 HTTP/1.1",
                404,
                "",
            ),
            (
                "GET /prefix/api/v1/repos/o/r/actions/tasks?limit=1 HTTP/1.1",
                200,
                r#"{"workflow_runs":[{"status":"success"}]}"#,
            ),
            ("GET /prefix/api/v1/repos/o/big HTTP/1.1", 200, &oversized),
            ("GET /prefix/api/v1/repos/o/private HTTP/1.1", 403, r#"{"message":"forbidden"}"#),
            ("GET /prefix/api/v1/repos/o/gone HTTP/1.1", 410, ""),
            ("GET /prefix/api/v1/repos/o/legal HTTP/1.1", 451, ""),
            (VERSION, 200, r#"{"version":"1.26.0"}"#),
            (USER_REPOS, 403, r#"{"message":"token does not have at least one of required scope(s), required=[read:user]"}"#),
            (VERSION, 200, "{}"),
        ]
        .map(|(request, status, body)| Exchange {
            request,
            status,
            body,
        });
        exercise(
            &GITEA,
            &[(TOKEN, "fake-forge-token")],
            Values::new(),
            &exchanges,
            |index, wire| {
                assert_header(
                    wire,
                    "authorization: token fake-forge-token",
                    "fake-forge-token",
                );
                if index == 2 {
                    "X-Total-Count: 12\r\n"
                } else {
                    ""
                }
            },
            async |conn| {
                conn.run(test(conn)).await.unwrap();
                let options = Values::from_iter([(
                    "repositories".into(),
                    json!(["o/r", "o/big", "o/private", "o/gone", "o/legal"]),
                )]);
                let body = super::super::fetch(conn, &GITEA.widgets[0], &options)
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(body).unwrap()["rows"],
                    json!([
                        ["Visible repositories", "12"],
                        ["o/r", "2 issues · 3 PRs · Last task: success"],
                        ["o/big", "Repository response too large"],
                        ["o/private", "Repository unavailable"],
                        ["o/gone", "Repository unavailable"],
                        ["o/legal", "Repository unavailable"]
                    ])
                );
                assert_test_fault(conn, &GITEA, Class::Auth).await;
                assert_test_fault(conn, &GITEA, Class::Protocol).await;
            },
        )
        .await;
    }
}
