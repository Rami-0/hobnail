use super::*;
use serde_json::json;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// The example from the README / final report: three real services on this
/// host and one command widget.
pub const EXAMPLE: &str = r#"
[[widgets]]
id = "beszel"
type = "custom-api"
title = "Beszel"
link = "http://100.64.0.10:8090"
url = "http://100.64.0.10:8090/api/health"
template = "{{message}}"
status = { field = "code", ok = [200] }
[widgets.fields]
message = "message"
code = "code"

[[widgets]]
id = "mailpit"
type = "custom-api"
title = "Mailpit"
link = "http://100.64.0.10:8025"
url = "http://100.64.0.10:8025/api/v1/info"
refresh = "60s"
[widgets.fields]
unread = "Unread"
messages = "Messages"
size = "DatabaseSize"
uptime = "RuntimeStats.Uptime"
version = "Version"
[[widgets.rows]]
label = "Unread"
value = "{{unread | number}} / {{messages | number}}"
[[widgets.rows]]
label = "Database"
value = "{{size | bytes}}"
[[widgets.rows]]
label = "Up"
value = "{{uptime | duration}} · {{version}}"

[[widgets]]
id = "odoo"
type = "custom-api"
title = "Odoo"
link = "http://100.64.0.10:8069"
url = "http://100.64.0.10:8069/web/health"
template = "{{status | upper}}"
status = { field = "status", ok = ["pass"] }
[widgets.fields]
status = "status"

[[widgets]]
id = "procs"
type = "command"
title = "Host processes"
enabled = true
refresh = "15s"
timeout = "2s"
# Shell builtins only: the image ships busybox sh and nothing else.
command = ["sh", "-c", 'set -- /host/proc/[0-9]*; printf "{\"procs\": %d}" "$#"']
template = "{{procs | number}}"
[widgets.fields]
procs = "procs"
"#;

fn widgets(toml: &str) -> Vec<WidgetConfig> {
    crate::config::parse(toml, "test.toml")
        .unwrap_or_else(|e| panic!("{e}"))
        .widgets
}

fn widget(toml: &str) -> WidgetConfig {
    widgets(toml).remove(0)
}

fn api(port: u16, path: &str, extra: &str) -> WidgetConfig {
    widget(&format!(
        "[[widgets]]\nid = \"t\"\ntype = \"custom-api\"\nurl = \"http://127.0.0.1:{port}{path}\"\ntimeout = \"2s\"\n{extra}\n"
    ))
}

fn cmd(script: &str, extra: &str) -> WidgetConfig {
    let mut w = widget(&format!(
        "[[widgets]]\nid = \"c\"\ntype = \"command\"\nenabled = true\ncommand = [\"sh\"]\ntimeout = \"2s\"\n{extra}\n"
    ));
    w.command = vec!["sh".into(), "-c".into(), script.into()];
    w
}

fn header<'a>(req: &'a str, name: &str) -> Option<&'a str> {
    req.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// A tiny HTTP/1.1 server on 127.0.0.1 with canned routes.
async fn server() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // Headers, then the body by content-length.
                loop {
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_string();
                        let len: usize = header(&head, "content-length")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        while buf.len() < end + 4 + len {
                            let n = s.read(&mut chunk).await.unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            buf.extend_from_slice(&chunk[..n]);
                        }
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&buf).to_string();
                let first = req.lines().next().unwrap_or("").to_string();
                let mut parts = first.split(' ');
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let (code, ctype, body) = match path.as_str() {
                    "/ok" => (
                        200,
                        "application/json",
                        json!({
                            "status": "pass", "n": 1234567, "size": 1610612736u64,
                            "items": [1, 2, 3], "nested": {"x.y": 5}
                        })
                        .to_string(),
                    ),
                    "/500" => (500, "text/plain", "oops".into()),
                    "/401" => (401, "text/plain", "no".into()),
                    "/html" => (200, "text/html", "<html>not json</html>".into()),
                    "/text" => (200, "text/plain", "hello <world>\n".into()),
                    "/hang" => {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        return;
                    }
                    "/echo" => {
                        let body = req.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                        (
                            200,
                            "application/json",
                            json!({
                                "method": method,
                                "auth": header(&req, "authorization"),
                                "auth_len": header(&req, "authorization").map(str::len),
                                "token": header(&req, "x-token"),
                                "token_len": header(&req, "x-token").map(str::len),
                                "ctype": header(&req, "content-type"),
                                "body": body,
                            })
                            .to_string(),
                        )
                    }
                    // A destination that reflects the request (TRACE, a
                    // debug page): as text, and as JSON with the header as
                    // a key, a value and inside a longer string.
                    "/reflect" => (200, "message/http", req.clone()),
                    "/reflect.json" => {
                        let auth = header(&req, "authorization").unwrap_or("").to_string();
                        let tok = header(&req, "x-token").unwrap_or("").to_string();
                        (
                            200,
                            "application/json",
                            json!({
                                "seen": format!("you sent {auth} and {tok}"),
                                "list": [auth.clone()],
                                (tok.clone()): "as a key",
                                "token_only": auth.split_once(' ').map(|x| x.1).unwrap_or(""),
                            })
                            .to_string(),
                        )
                    }
                    _ => (404, "text/plain", "not found".into()),
                };
                let resp = format!(
                    "HTTP/1.1 {code} X\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
                let _ = s.shutdown().await;
            });
        }
    });
    port
}

async fn go(w: &WidgetConfig) -> WidgetOutput {
    run(w, &HttpPolicy::default()).await
}

// ---------------------------------------------------------------- custom-api

#[tokio::test]
async fn custom_api_ok() {
    let port = server().await;
    let w = api(
        port,
        "/ok",
        r#"
template = "{{status | upper}}"
status = { field = "status", ok = ["pass"] }
[fields]
status = "status"
n = "n"
size = "size"
count = "items.#"
last = "items[-1]"
dotted = 'nested."x.y"'
missing = "nope.nope"
[[rows]]
label = "N"
value = "{{n | number}}"
[[rows]]
label = "Size"
value = "{{size | bytes}} <{{missing}}>"
"#
        .replace("[fields]", "[widgets.fields]")
        .replace("[[rows]]", "[[widgets.rows]]")
        .as_str(),
    );
    let o = go(&w).await;
    assert_eq!(o.error, None);
    assert_eq!(o.values["status"], json!("pass"));
    assert_eq!(o.values["count"], json!(3));
    assert_eq!(o.values["last"], json!(3));
    assert_eq!(o.values["dotted"], json!(5));
    assert_eq!(o.values["missing"], Value::Null);
    assert_eq!(o.values["_status"], json!(200));
    assert_eq!(o.values["_ok"], json!(true));
    assert!(o.values["_ms"].is_u64());
    assert_eq!(o.display.headline.as_deref(), Some("PASS"));
    assert_eq!(
        o.display.rows,
        [
            ("N".into(), "1,234,567".into()),
            ("Size".into(), "1.5 GiB <—>".into())
        ]
    );
    assert!(o.fetched_at > 0);
}

#[tokio::test]
async fn custom_api_default_rows_and_raw() {
    let port = server().await;
    // Default: one row per field, sorted by name.
    let w = api(
        port,
        "/ok",
        "[widgets.fields]\nzeta = \"n\"\nalpha = \"status\"\n",
    );
    let body = go(&w).await.display;
    assert_eq!(
        body.rows,
        [
            ("alpha".into(), "pass".into()),
            ("zeta".into(), "1234567".into())
        ]
    );
    let body = go(&api(port, "/ok", "")).await.display;
    let raw: Value = serde_json::from_str(body.text.as_deref().unwrap()).unwrap();
    assert_eq!(raw["status"], "pass");
}

#[tokio::test]
async fn custom_api_status_not_ok() {
    let port = server().await;
    let w = api(
        port,
        "/ok",
        "status = { field = \"status\", ok = [\"fine\"] }\n[widgets.fields]\nstatus = \"status\"\n",
    );
    assert_eq!(go(&w).await.values["_ok"], json!(false));
    // Truthy rule and the built-in _status.
    let w = api(
        port,
        "/ok",
        "status = { field = \"_status\", ok = [200] }\n",
    );
    assert_eq!(go(&w).await.values["_ok"], json!(true));
    let w = api(
        port,
        "/ok",
        "status = { field = \"n\" }\n[widgets.fields]\nn = \"n\"\n",
    );
    assert_eq!(go(&w).await.values["_ok"], json!(true));
}

#[tokio::test]
async fn custom_api_http_errors() {
    let port = server().await;
    let w = api(port, "/500", "status = { field = \"_status\", ok = [200] }");
    let o = go(&w).await;
    assert_eq!(o.error.as_deref(), Some("HTTP 500 Internal Server Error"));
    assert_eq!(o.values["_status"], json!(500));
    assert_eq!(o.values["_ok"], json!(false));
    assert!(o.display.text.is_none() && o.display.headline.is_none() && o.display.rows.is_empty());
    let o = go(&api(port, "/401", "")).await;
    assert_eq!(o.error.as_deref(), Some("HTTP 401 Unauthorized"));
}

#[tokio::test]
async fn custom_api_bad_json() {
    let port = server().await;
    let o = go(&api(port, "/html", "")).await;
    let e = o.error.unwrap();
    assert!(e.starts_with("response is not JSON"), "{e}");
    assert_eq!(o.values["_status"], json!(200));
}

#[tokio::test]
async fn custom_api_text_output() {
    let port = server().await;
    let o = go(&api(port, "/text", "output = \"text\"")).await;
    assert_eq!(o.values["_text"], json!("hello <world>"));
    assert_eq!(o.display.text.as_deref(), Some("hello <world>"));
    let o = go(&api(
        port,
        "/text",
        "output = \"text\"\ntemplate = \"{{_text | upper}}\"",
    ))
    .await;
    assert_eq!(o.display.headline.as_deref(), Some("HELLO <WORLD>"));
}

#[tokio::test]
async fn custom_api_timeout() {
    let port = server().await;
    let mut w = api(port, "/hang", "");
    w.timeout = crate::util::Dur(Duration::from_millis(300));
    let t0 = Instant::now();
    let o = go(&w).await;
    assert!(t0.elapsed() < Duration::from_secs(2));
    assert_eq!(o.error.as_deref(), Some("timed out after 300ms"));
}

#[tokio::test]
async fn custom_api_connection_refused() {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let o = go(&api(port, "/ok", "")).await;
    let e = o.error.unwrap();
    assert!(e.starts_with(&format!("connect 127.0.0.1:{port}")), "{e}");
    assert!(!o.values.contains_key("_status"));
}

#[tokio::test]
async fn custom_api_allow_hosts() {
    let port = server().await;
    let policy = HttpPolicy {
        allow_hosts: vec!["example.com".into()],
    };
    let o = run(&api(port, "/ok", ""), &policy).await;
    assert!(o.error.unwrap().contains("allow_hosts"));
}

#[tokio::test]
async fn custom_api_secret_headers_and_body() {
    let port = server().await;
    let var = format!("RH_TEST_SECRET_{}", std::process::id());
    std::env::set_var(&var, "Bearer s3cret");
    let file = std::env::temp_dir().join(format!("rh-secret-{}", std::process::id()));
    std::fs::write(&file, "tok-123\nsecond line ignored\n").unwrap();
    let w = api(
        port,
        "/echo",
        &format!(
            "method = \"post\"\nbody = '{{\"q\":1}}'\nheaders_env = {{ Authorization = \"{var}\" }}\nheaders_file = {{ X-Token = \"{}\" }}\n[widgets.fields]\nauth = \"auth\"\nauth_len = \"auth_len\"\ntoken = \"token\"\ntoken_len = \"token_len\"\nmethod = \"method\"\nctype = \"ctype\"\nbody = \"body\"\n",
            file.display()
        ),
    );
    let o = go(&w).await;
    let _ = std::fs::remove_file(&file);
    assert_eq!(o.error, None);
    // Sent (the lengths say so), and never shown back.
    assert_eq!(o.values["auth_len"], json!("Bearer s3cret".len()));
    assert_eq!(o.values["token_len"], json!("tok-123".len()));
    assert_eq!(o.values["auth"], json!("<redacted>"));
    assert_eq!(o.values["token"], json!("<redacted>"));
    assert_eq!(o.values["method"], json!("POST"));
    assert_eq!(o.values["ctype"], json!("application/json"));
    assert_eq!(o.values["body"], json!("{\"q\":1}"));

    // Missing env var / file: named, never a value.
    let mut w2 = w.clone();
    w2.headers_env = [(
        "Authorization".to_string(),
        "RH_TEST_UNSET_VAR_X".to_string(),
    )]
    .into();
    let e = go(&w2).await.error.unwrap();
    assert_eq!(
        e,
        "header Authorization: environment variable RH_TEST_UNSET_VAR_X is not set"
    );
    let mut w3 = w.clone();
    w3.headers_file = [("X-Token".to_string(), "/nonexistent/secret".to_string())].into();
    let e = go(&w3).await.error.unwrap();
    assert!(
        e.starts_with("header X-Token: cannot read /nonexistent/secret"),
        "{e}"
    );
    std::env::set_var(&var, "bad\nvalue");
    let e = go(&w).await.error.unwrap();
    assert!(!e.contains("bad"), "{e}");
    std::env::remove_var(&var);
}

#[tokio::test]
async fn custom_api_secret_headers_never_come_back() {
    let port = server().await;
    let var = format!("RH_TEST_REFLECT_{}", std::process::id());
    std::env::set_var(&var, "Bearer r3fl3ct-me");
    let file = std::env::temp_dir().join(format!("rh-reflect-{}", std::process::id()));
    std::fs::write(&file, "tok-\"quoted\"-9\n").unwrap();
    let secrets = format!(
        "headers_env = {{ Authorization = \"{var}\" }}\nheaders_file = {{ X-Token = \"{}\" }}\n",
        file.display()
    );
    let leaks = |o: &WidgetOutput| {
        let all = format!("{:?} {:?} {:?}", o.values, o.display, o.error);
        ["r3fl3ct-me", "tok-", "quoted"]
            .iter()
            .filter(|s| all.contains(*s))
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    };
    // Text output: `_text` and the rendered tile.
    let w = api(port, "/reflect", &format!("output = \"text\"\n{secrets}"));
    let o = go(&w).await;
    assert_eq!(o.error, None);
    assert!(leaks(&o).is_empty(), "{o:?}");
    assert!(o.values["_text"]
        .as_str()
        .unwrap()
        .contains("authorization: <redacted>"));
    // JSON: extracted values, keys, the raw JSON tile, a template.
    let w = api(
        port,
        "/reflect.json",
        &format!("{secrets}template = \"{{{{seen}}}} {{{{only}}}}\"\n[widgets.fields]\nseen = \"seen\"\nlist = \"list\"\nonly = \"token_only\"\n"),
    );
    let o = go(&w).await;
    assert_eq!(o.error, None);
    assert!(leaks(&o).is_empty(), "{o:?}");
    assert_eq!(
        o.values["seen"],
        json!("you sent <redacted> and <redacted>")
    );
    assert_eq!(o.values["only"], json!("<redacted>"));
    let w = api(port, "/reflect.json", &secrets);
    let o = go(&w).await;
    assert!(leaks(&o).is_empty(), "raw JSON tile: {o:?}");
    // Not JSON: the error names no value either.
    let w = api(port, "/reflect", &secrets);
    let o = go(&w).await;
    assert!(o.error.is_some() && leaks(&o).is_empty(), "{o:?}");
    let _ = std::fs::remove_file(&file);
    std::env::remove_var(&var);
}

#[test]
fn scrub_replaces_each_form_of_a_secret() {
    let s = Scrub::new(&["Bearer abc123".into(), "".into(), "k\"q".into()]);
    assert_eq!(
        s.text("a Bearer abc123 b abc123 c"),
        "a <redacted> b <redacted> c"
    );
    assert_eq!(s.text(r#"{"h":"k\"q"}"#), r#"{"h":"<redacted>"}"#);
    assert_eq!(s.text("nothing here"), "nothing here");
    let mut v = json!({"k\"q": ["Bearer abc123", 1]});
    s.json(&mut v);
    assert_eq!(v, json!({"<redacted>": ["<redacted>", 1]}));
}

// ------------------------------------------------------------------ command

#[tokio::test]
async fn command_json() {
    let w = cmd(
        r#"echo '{"a": 1, "b": {"c": "x"}}'"#,
        "template = \"{{a}}-{{c}}\"\n[widgets.fields]\na = \"a\"\nc = \"b.c\"\n",
    );
    let o = go(&w).await;
    assert_eq!(o.error, None, "{o:?}");
    assert_eq!(o.values["a"], json!(1));
    assert_eq!(o.values["_status"], json!(0));
    assert_eq!(o.display.headline.as_deref(), Some("1-x"));
}

#[tokio::test]
async fn command_disabled_by_default() {
    let mut w = cmd("echo '{}'", "");
    w.enabled = None;
    let o = go(&w).await;
    assert_eq!(o.error.as_deref(), Some(DISABLED));
    assert!(o.values.is_empty());
    w.enabled = Some(false);
    assert_eq!(go(&w).await.error.as_deref(), Some(DISABLED));
}

#[tokio::test]
async fn command_timeout_kills() {
    let mut w = cmd("sleep 5; echo '{}'", "");
    w.timeout = crate::util::Dur(Duration::from_millis(300));
    let t0 = Instant::now();
    let o = go(&w).await;
    assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
    assert_eq!(o.error.as_deref(), Some("timed out after 300ms"));
}

#[tokio::test]
async fn command_nonzero_exit() {
    let o = go(&cmd(
        "echo partial; echo 'boom:\n  bad thing' >&2; exit 3",
        "",
    ))
    .await;
    assert_eq!(o.error.as_deref(), Some("exit 3: boom: bad thing"));
    assert_eq!(o.values["_status"], json!(3));
    let o = go(&cmd("kill -9 $$", "")).await;
    assert_eq!(o.error.as_deref(), Some("killed by signal 9"));
}

#[tokio::test]
async fn command_text_output_and_env() {
    let o = go(&cmd("echo hello; echo world", "output = \"text\"")).await;
    assert_eq!(o.values["_text"], json!("hello\nworld"));
    assert_eq!(o.display.text.as_deref(), Some("hello\nworld"));

    let o = go(&cmd(
        r#"printf '{"home":"%s","foo":"%s","path":"%s"}' "$HOME" "$FOO" "$PATH""#,
        "env = { FOO = \"bar\" }\n[widgets.fields]\nhome = \"home\"\nfoo = \"foo\"\npath = \"path\"\n",
    ))
    .await;
    assert_eq!(o.error, None);
    assert_eq!(o.values["home"], json!(""), "environment is cleared");
    assert_eq!(o.values["foo"], json!("bar"));
    assert_ne!(o.values["path"], json!(""));
}

#[tokio::test]
async fn command_errors() {
    let o = go(&cmd("echo not json", "")).await;
    assert!(o.error.unwrap().starts_with("output is not JSON"));
    let o = go(&cmd("echo why >&2", "")).await;
    assert_eq!(o.error.as_deref(), Some("no output: why"));
    let o = go(&cmd("yes | head -c 300000", "output = \"text\"")).await;
    assert_eq!(o.error.as_deref(), Some("output larger than 256 KiB"));
    let mut w = cmd("", "");
    w.command = vec!["/nonexistent/prog".into()];
    let e = go(&w).await.error.unwrap();
    assert!(e.starts_with("cannot start /nonexistent/prog"), "{e}");
}

// --------------------------------------------------------------- validation

fn invalid(toml: &str) -> String {
    match crate::config::parse(toml, "c.toml") {
        Ok(_) => panic!("should not parse:\n{toml}"),
        Err(e) => e,
    }
}

#[test]
fn validation_errors() {
    let base = "[[widgets]]\nid = \"w1\"\ntype = \"custom-api\"\nurl = \"http://h/\"\n";
    let cases: &[(&str, &str)] = &[
        (
            "[[widgets]]\nid = \"a\"\ntype = \"custom-api\"\nurl = \"http://h/\"\n[[widgets]]\nid = \"a\"\ntype = \"custom-api\"\nurl = \"http://h/\"\n",
            "duplicate widget id \"a\"",
        ),
        ("[[widgets]]\nid = \"a b\"\ntype = \"command\"\n", "id must be"),
        ("[[widgets]]\nid = \"a\"\ntype = \"rss\"\n", "unknown type \"rss\""),
        ("[[widgets]]\nid = \"a\"\ntype = \"custom-api\"\n", "custom-api needs url"),
        ("[[widgets]]\nid = \"a\"\ntype = \"custom-api\"\nurl = \"ftp://x\"\n", "url must be"),
        ("[[widgets]]\nid = \"a\"\ntype = \"command\"\n", "needs command"),
        ("[[widgets]]\nid = \"a\"\ntype = \"command\"\ncommand = [\"\"]\n", "needs command"),
        (
            "[[widgets]]\nid = \"a\"\ntype = \"command\"\ncommand = [\"x\"]\nurl = \"http://h/\"\n",
            "only for type = \"custom-api\"",
        ),
        (&format!("{base}refresh = \"500ms\"\n"), "refresh must be at least 1s"),
        (&format!("{base}timeout = \"10ms\"\n"), "timeout must be"),
        (&format!("{base}link = \"javascript:alert(1)\"\n"), "link must start"),
        (&format!("{base}method = \"G E T\"\n"), "invalid method"),
        (&format!("{base}headers = {{ \"bad name\" = \"x\" }}\n"), "invalid header name"),
        (&format!("{base}template = \"{{{{nope}}}}\"\n"), "template: unknown field \"nope\""),
        (
            &format!("{base}template = \"{{{{a | shout}}}}\"\n[widgets.fields]\na = \"a\"\n"),
            "unknown filter \"shout\"",
        ),
        (&format!("{base}[widgets.fields]\na = \"x..y\"\n"), "field a: path \"x..y\""),
        (&format!("{base}[widgets.fields]\n_a = \"x\"\n"), "field name \"_a\""),
        (
            &format!("{base}[[widgets.rows]]\nlabel = \"L\"\nvalue = \"{{{{zz}}}}\"\n"),
            "row \"L\": unknown field \"zz\"",
        ),
        (&format!("{base}status = {{ field = \"s\" }}\n"), "status: unknown field \"s\""),
        (
            &format!("{base}output = \"text\"\n[widgets.fields]\na = \"a\"\n"),
            "fields need output = \"json\"",
        ),
        (&format!("{base}colour = \"red\"\n"), "colour"),
        (&format!("{base}output = \"xml\"\n"), "xml"),
        (&format!("{base}[[widgets.rows]]\nlabel = \"L\"\nvalu = \"x\"\n"), "valu"),
    ];
    for (toml, want) in cases {
        let e = invalid(toml);
        assert!(
            e.contains(want),
            "expected {want:?} in error: {e}\n--- config:\n{toml}"
        );
    }
    // Errors from semantic checks name the widget.
    let e = invalid(&format!("{base}template = \"{{{{nope}}}}\"\n"));
    assert!(e.contains("widget \"w1\": template"), "{e}");
}

#[test]
fn example_config_parses() {
    let ws = widgets(EXAMPLE);
    let ids: Vec<&str> = ws.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["beszel", "mailpit", "odoo", "procs"]);
    assert!(ws[3].is_enabled());
    assert_eq!(ws[3].command[0], "sh");
    assert_eq!(ws[1].fields["uptime"], "RuntimeStats.Uptime");
    assert_eq!(ws[1].rows.len(), 3);
}

#[tokio::test]
async fn example_command_runs() {
    // /host/proc does not exist in the test container; the widget still
    // produces valid JSON (0 processes) rather than an error.
    let mut w = widgets(EXAMPLE).remove(3);
    w.command[2] = w.command[2].replace("/host/proc", "/proc");
    let o = go(&w).await;
    assert_eq!(o.error, None, "{o:?}");
    assert!(o.values["procs"].as_u64().unwrap() > 0);
}

#[test]
fn widget_links_reject_network_path_references() {
    assert!(safe_link("/local"));
    assert!(!safe_link("//evil.example"));
    assert!(!safe_link(r"/\evil.example"));
}

#[test]
fn refresh_errors_keep_only_the_previous_success_until_recovery() {
    let failed = |fetched_at| WidgetOutput {
        fetched_at,
        error: Some("timed out".into()),
        ..Default::default()
    };
    let success = |fetched_at| WidgetOutput {
        fetched_at,
        display: WidgetBody {
            headline: Some(fetched_at.to_string()),
            ..Default::default()
        },
        values: BTreeMap::from([("number".into(), fetched_at.into())]),
        ..Default::default()
    };
    let mut current = failed(1);
    current.record(failed(2));
    assert!(current.stale_since.is_none());
    assert!(current.display.headline.is_none());
    current.record(success(3));
    current.record(failed(4));
    current.record(failed(5));
    assert_eq!(current.fetched_at, 5);
    assert_eq!(current.stale_since, Some(3));
    assert_eq!(current.display.headline.as_deref(), Some("3"));
    assert_eq!(current.values["number"], 3);
    assert_eq!(current.error.as_deref(), Some("timed out"));
    current.record(success(6));
    assert_eq!(current.display.headline.as_deref(), Some("6"));
    assert!(current.stale_since.is_none());
    assert!(current.error.is_none());
}

#[test]
fn widget_refresh_uses_elapsed_time_and_orders_overlapping_attempts() {
    let mut cache = WidgetCache::default();
    let tick = Instant::now();
    let refresh = Duration::from_secs(30);
    let success = cache.begin("widget");
    let failure = cache.begin("widget");
    assert!(cache.record(
        "widget",
        success,
        tick,
        WidgetOutput {
            fetched_at: 1000,
            display: WidgetBody {
                headline: Some("last good".into()),
                ..Default::default()
            },
            ..Default::default()
        }
    ));
    let completed = tick + Duration::from_secs(1);
    assert!(cache.record(
        "widget",
        failure,
        completed,
        WidgetOutput {
            fetched_at: 900,
            error: Some("timed out".into()),
            ..Default::default()
        }
    ));
    assert!(!cache.record(
        "widget",
        success,
        completed,
        WidgetOutput {
            fetched_at: 1001,
            ..Default::default()
        }
    ));
    let out = cache.get("widget").unwrap();
    assert_eq!(out.fetched_at, 900);
    assert_eq!(out.stale_since, Some(1000));
    assert_eq!(out.error.as_deref(), Some("timed out"));
    assert_eq!(out.display.headline.as_deref(), Some("last good"));
    assert_eq!(cache.deadline("widget", refresh), Some(completed + refresh));
}
