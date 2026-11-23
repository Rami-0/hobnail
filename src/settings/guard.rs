//! Guards applied to **every** config write from the app: form operations,
//! the raw editor and revision restore all end in [`check`].
//!
//! # The `command` widget rule
//!
//! A `command` widget runs a program inside this container, which has the
//! Docker socket: from a browser that would be remote code execution as root
//! on the host. So the app may never *create*, *change* or *enable* one.
//!
//! The check compares the old and new file canonically (parsed TOML values,
//! so formatting, comments and key order do not matter):
//!
//! - A widget is **command-capable** when its `type` is anything other than
//!   `"custom-api"` (so `"command"`, a typo or a future type all count, fail
//!   closed) or when its table has a `command` or `env` key at all.
//! - Every command-capable widget in the new file must be **identical**
//!   (every key and value) to a widget in the old file. Adding one, changing
//!   any key (argv, env, timeout, title, `enabled`, …), renaming its id, or
//!   turning a `custom-api` widget into a `command` one is refused.
//! - **Removing** a command widget is allowed: it only takes capability away,
//!   and the owner may want to clean up from the browser. Turning a
//!   `command` widget into a plain `custom-api` one (no `command`/`env` left)
//!   is the same as removing it and adding a harmless widget, so it is
//!   allowed too.
//! - If the old file does not parse, it counts as having no widgets at all
//!   (most restrictive): fix it on the host or remove the command widgets.
//!
//! # Secrets: use, never read
//!
//! `*_file` settings make the app read a file and send its first line to a
//! host. A path *added from the app* must be inside `{data_dir}/secrets/`
//! (allowlist, also checked after resolving symlinks): a denylist missed the
//! host mounts (`/host/proc/self/environ` would leak the whole environment).
//! Paths already in the file (set on the host) are left alone.
//!
//! Secret references are also **bound to their destination** (see
//! [`check_secret_bindings`]): the app cannot point an existing secret at a
//! new host, which would let a stolen session read it.

use std::collections::BTreeMap;
use toml::Value as T;

/// Why a write was refused (HTTP 403).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied(pub String);

pub const COMMAND_RULE: &str =
    "command widgets can only be added, changed or enabled in the config file on the host";

fn parse_table(text: &str) -> Option<toml::Table> {
    toml_edit::de::from_str::<toml::Table>(text).ok()
}

fn widgets(t: &toml::Table) -> Vec<&toml::Table> {
    match t.get("widgets") {
        Some(T::Array(a)) => a.iter().filter_map(T::as_table).collect(),
        Some(T::Table(w)) => vec![w],
        _ => Vec::new(),
    }
}

/// True for a widget table that could run a program (see module docs).
pub fn is_command_capable(w: &toml::Table) -> bool {
    w.get("type").and_then(T::as_str) != Some("custom-api")
        || w.contains_key("command")
        || w.contains_key("env")
}

fn widget_id(w: &toml::Table) -> String {
    w.get("id")
        .and_then(T::as_str)
        .unwrap_or("(no id)")
        .to_string()
}

/// The command-widget rule. `old` is the file as it is on disk now.
pub fn check_command_widgets(old: &str, new: &str) -> Result<(), Denied> {
    let new_t =
        parse_table(new).ok_or_else(|| Denied("the new config is not valid TOML".into()))?;
    let old_t = parse_table(old).unwrap_or_default();
    let old_w = widgets(&old_t);
    for w in widgets(&new_t) {
        if !is_command_capable(w) {
            continue;
        }
        if old_w.contains(&w) {
            continue;
        }
        let id = widget_id(w);
        let before = old_w
            .iter()
            .find(|o| o.get("id") == w.get("id") && w.get("id").is_some());
        let what = match before {
            None => "adds",
            Some(o) if !is_command_capable(o) => "turns into a command widget",
            Some(o) => {
                let mut o2 = (*o).clone();
                o2.insert("enabled".into(), T::Boolean(true));
                if w.get("enabled") == Some(&T::Boolean(true)) && o2 == *w {
                    "enables"
                } else {
                    "changes"
                }
            }
        };
        return Err(Denied(format!(
            "refused: this {what} the command widget {id:?}; {COMMAND_RULE}"
        )));
    }
    Ok(())
}

/// Every `*_file` path in widgets and channels.
fn secret_files(t: &toml::Table) -> Vec<String> {
    let mut out = Vec::new();
    for w in widgets(t) {
        if let Some(T::Table(h)) = w.get("headers_file") {
            out.extend(h.values().filter_map(T::as_str).map(str::to_string));
        }
    }
    if let Some(T::Table(chans)) = t.get("channels") {
        for c in chans.values().filter_map(T::as_table) {
            for k in ["url_file", "webhook_url_file", "token_file"] {
                if let Some(p) = c.get(k).and_then(T::as_str) {
                    out.push(p.to_string());
                }
            }
        }
    }
    out
}

/// Format check for a `*_file` path typed in a form (absolute, plain). The
/// location policy is applied to the whole file by [`check`].
pub fn check_secret_path(p: &str) -> Result<(), String> {
    if !p.starts_with('/') {
        return Err(format!("secret file {p:?} must be an absolute path"));
    }
    if p.chars().any(|c| c.is_control()) || p.split('/').any(|s| s == ".." || s == ".") {
        return Err(format!("secret file {p:?} is not a plain path"));
    }
    Ok(())
}

/// Location policy for a `*_file` path **added from the app**: an
/// allowlist, not a denylist. It must lie inside `secrets_dir`
/// (`{data_dir}/secrets/`), lexically and, if the file exists, after
/// resolving symlinks. Anything else (`/host/proc/self/environ`,
/// `/proc/…`, the database, …) can only be set in the file on the host.
pub fn check_secret_location(p: &str, secrets_dir: &std::path::Path) -> Result<(), String> {
    check_secret_path(p)?;
    let dir = secrets_dir
        .to_string_lossy()
        .trim_end_matches('/')
        .to_string();
    let outside = || {
        format!(
            "secret file {p:?}: from the app, secret files must be in {dir}/ \
             (other paths can only be set in the config file on the host)"
        )
    };
    if !p.starts_with(&format!("{dir}/")) {
        return Err(outside());
    }
    if let Ok(real) = std::fs::canonicalize(p) {
        let real_dir = std::fs::canonicalize(secrets_dir).map_err(|_| outside())?;
        if !real.starts_with(&real_dir) {
            return Err(outside());
        }
    }
    Ok(())
}

/// New `*_file` paths must pass [`check_secret_location`]. Paths that were
/// already in the file (set on the host) are left alone.
pub fn check_secret_files(
    old: &str,
    new: &str,
    secrets_dir: &std::path::Path,
) -> Result<(), Denied> {
    let Some(new_t) = parse_table(new) else {
        return Err(Denied("the new config is not valid TOML".into()));
    };
    let old_files = parse_table(old)
        .map(|t| secret_files(&t))
        .unwrap_or_default();
    for p in secret_files(&new_t) {
        if !old_files.contains(&p) {
            check_secret_location(&p, secrets_dir).map_err(Denied)?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------ secret bindings
//
// The app may *use* a secret but never *read* it. A secret is only safe while
// it goes where the owner pointed it. So where the destination is chosen
// separately from the secret, the pair is bound: the app cannot attach a
// secret reference to a destination, nor move a secret-bearing entry to a new
// destination, unless that exact (destination, references) pair is already in
// the file. Otherwise a stolen session could add `headers_env = {X =
// "RH_TELEGRAM_TOKEN"}` to a widget pointing at its own server.
//
// Not bound (safe): Slack `webhook_url_*` and webhook `url_*` (the secret IS
// the destination), Telegram with the default `api_base` (fixed host).

fn opt<'a>(t: &'a toml::Table, k: &str) -> Option<&'a T> {
    t.get(k)
}

fn nonempty_table(t: &toml::Table, k: &str) -> bool {
    matches!(t.get(k), Some(T::Table(h)) if !h.is_empty())
}

/// (destination, secret references) of a widget with header secrets.
fn widget_binding(w: &toml::Table) -> Option<Vec<Option<&T>>> {
    if !(nonempty_table(w, "headers_env") || nonempty_table(w, "headers_file")) {
        return None;
    }
    Some(vec![
        opt(w, "url"),
        opt(w, "headers_env"),
        opt(w, "headers_file"),
    ])
}

/// (destination, secret references) of a channel whose destination is chosen
/// separately from its secrets.
fn channel_binding(c: &toml::Table) -> Option<Vec<Option<&T>>> {
    match c.get("type").and_then(T::as_str) {
        Some("telegram") if c.contains_key("api_base") => Some(vec![
            opt(c, "api_base"),
            opt(c, "token_env"),
            opt(c, "token_file"),
            opt(c, "chat_id_env"),
        ]),
        Some("webhook")
            if nonempty_table(c, "headers_env") || nonempty_table(c, "headers_file") =>
        {
            Some(vec![
                opt(c, "url"),
                opt(c, "url_env"),
                opt(c, "url_file"),
                opt(c, "headers_env"),
                opt(c, "headers_file"),
            ])
        }
        _ => None,
    }
}

fn channels(t: &toml::Table) -> Vec<&toml::Table> {
    match t.get("channels") {
        Some(T::Table(c)) => c.values().filter_map(T::as_table).collect(),
        _ => Vec::new(),
    }
}

pub const SECRET_BINDING_RULE: &str = "an entry that sends a secret can only get a new \
     destination, or new secret references, in the config file on the host";

/// The secret-binding rule (see the section comment above).
pub fn check_secret_bindings(old: &str, new: &str) -> Result<(), Denied> {
    let new_t =
        parse_table(new).ok_or_else(|| Denied("the new config is not valid TOML".into()))?;
    let old_t = parse_table(old).unwrap_or_default();
    let old_w: Vec<_> = widgets(&old_t)
        .into_iter()
        .filter_map(widget_binding)
        .collect();
    for w in widgets(&new_t) {
        if let Some(b) = widget_binding(w) {
            if !old_w.contains(&b) {
                return Err(Denied(format!(
                    "refused: widget {:?} would send header secrets to a new destination; {SECRET_BINDING_RULE}",
                    widget_id(w)
                )));
            }
        }
    }
    let old_c: Vec<_> = channels(&old_t)
        .into_iter()
        .filter_map(channel_binding)
        .collect();
    if let Some(T::Table(chans)) = new_t.get("channels") {
        for (name, c) in chans.iter().filter_map(|(n, c)| Some((n, c.as_table()?))) {
            if let Some(b) = channel_binding(c) {
                if !old_c.contains(&b) {
                    return Err(Denied(format!(
                        "refused: channel {name:?} would send its secrets to a new destination \
                         (api_base / url with secret headers); {SECRET_BINDING_RULE}"
                    )));
                }
            }
        }
    }
    Ok(())
}

/// All guards for one write. `secrets_dir` is `{data_dir}/secrets`.
pub fn check(old: &str, new: &str, secrets_dir: &std::path::Path) -> Result<(), Denied> {
    check_command_widgets(old, new)?;
    check_secret_files(old, new, secrets_dir)?;
    check_secret_bindings(old, new)
}

// ------------------------------------------------------------ env var names

/// `^[A-Z_][A-Z0-9_]*$`, at most 128 characters.
pub fn valid_env_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase() || c == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && s.len() <= 128
}

// ------------------------------------------------------------ secret values

/// What a secret-looking string is.
fn secret_kind(line: &str) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let lower = line.to_ascii_lowercase();
    for (needle, kind) in [
        ("hooks.slack.com/services/", "a Slack webhook URL"),
        ("discord.com/api/webhooks/", "a Discord webhook URL"),
        ("discordapp.com/api/webhooks/", "a Discord webhook URL"),
    ] {
        let mut from = 0;
        while let Some(i) = lower[from..].find(needle) {
            let start = from + i;
            let end = line[start..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                .map(|e| start + e)
                .unwrap_or(line.len());
            out.push((kind, line[start..end].to_string()));
            from = end.max(start + needle.len());
        }
    }
    // Telegram bot token: \d+:[A-Za-z0-9_-]{30,} with a boundary before.
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            let ds = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i < b.len() && b[i] == b':' {
                let ts = i + 1;
                let mut j = ts;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'-')
                {
                    j += 1;
                }
                if j - ts >= 30 {
                    out.push(("a Telegram bot token", line[ds..j].to_string()));
                    i = j;
                    continue;
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Warnings for secret-looking values that `new` adds (not ones already in
/// `old`). The warning names the line, never the value.
pub fn secret_warnings(old: &str, new: &str) -> Vec<String> {
    let mut seen: BTreeMap<String, ()> = BTreeMap::new();
    for line in old.lines() {
        for (_, s) in secret_kind(line) {
            seen.insert(s, ());
        }
    }
    let mut out = Vec::new();
    // Comment lines count too: they are stored in the file and its revisions.
    for (n, line) in new.lines().enumerate() {
        for (kind, s) in secret_kind(line) {
            if !seen.contains_key(&s) {
                out.push(format!(
                    "line {}: this looks like {kind}, which is a secret. Secrets should not be \
                     stored in the config file (or its revisions): put it in an environment \
                     variable and reference the variable's name (e.g. webhook_url_env, token_env).",
                    n + 1
                ));
            }
        }
    }
    out
}
