//! Log level detection on one line of plain text (ANSI already stripped).
//! Cheap on purpose: the background counter runs it on every line of every
//! counted container, so there is no JSON parse, only a few byte scans.
//!
//! Sources, in order: a JSON object's `level` / `lvl` / `severity` /
//! `levelname` / `log.level` / `@l` (strings or pino/bunyan numbers),
//! logfmt `level=` / `lvl=` / `severity=`, then the first few words
//! (`ERROR`, `[warn]`, `error:`, `<E>`, glog `E1114 …`, postgres
//! `LOG:` …). Lowercase words only count when bracketed or followed by a
//! colon, so "connection error" in prose is not an error line.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    /// No level found.
    Other = 4,
}

impl Level {
    /// One-letter code sent to the browser.
    pub fn code(self) -> &'static str {
        match self {
            Level::Error => "e",
            Level::Warn => "w",
            Level::Info => "i",
            Level::Debug => "d",
            Level::Other => "",
        }
    }
}

/// A level word ("error", "WARNING", "crit", "E") to a level.
fn word(w: &str) -> Option<Level> {
    let l = w.to_ascii_lowercase();
    Some(match l.as_str() {
        "error" | "err" | "eror" | "fatal" | "crit" | "critical" | "panic" | "alert" | "emerg"
        | "emergency" | "severe" | "e" | "f" | "c" => Level::Error,
        "warn" | "warning" | "wrn" | "w" => Level::Warn,
        "info" | "inf" | "notice" | "information" | "informational" | "log" | "i" | "n" => {
            Level::Info
        }
        "debug" | "dbg" | "trace" | "trc" | "verbose" | "d" | "t" | "v" => Level::Debug,
        _ => return None,
    })
}

/// pino / bunyan numeric levels, and syslog severities 0..=7.
fn number(n: i64) -> Option<Level> {
    Some(match n {
        0..=3 => Level::Error, // syslog emerg..err
        4 => Level::Warn,
        5 | 6 => Level::Info,
        7 => Level::Debug,
        10 | 20 => Level::Debug,
        30 => Level::Info,
        40 => Level::Warn,
        50 | 60 => Level::Error,
        _ => return None,
    })
}

pub fn detect(line: &str) -> Level {
    let t = line.trim_start();
    if t.starts_with('{') {
        if let Some(l) = json_level(t) {
            return l;
        }
    }
    if let Some(l) = logfmt_level(t) {
        return l;
    }
    prefix_level(t).unwrap_or(Level::Other)
}

const JSON_KEYS: &[&str] = &[
    "\"level\"",
    "\"lvl\"",
    "\"severity\"",
    "\"levelname\"",
    "\"log.level\"",
    "\"@l\"",
    "\"loglevel\"",
    "\"Level\"",
    "\"Severity\"",
];

fn json_level(t: &str) -> Option<Level> {
    for k in JSON_KEYS {
        let mut from = 0;
        while let Some(i) = t[from..].find(k) {
            let at = from + i + k.len();
            from = at;
            let rest = t[at..].trim_start();
            let Some(rest) = rest.strip_prefix(':') else {
                continue;
            };
            let rest = rest.trim_start();
            if let Some(s) = rest.strip_prefix('"') {
                let end = s.find('"')?;
                return word(&s[..end]);
            }
            let end = rest
                .find(|c: char| !c.is_ascii_digit() && c != '-')
                .unwrap_or(rest.len());
            return rest[..end].parse().ok().and_then(number);
        }
    }
    None
}

fn logfmt_level(t: &str) -> Option<Level> {
    for k in ["level=", "lvl=", "severity=", "loglevel="] {
        let mut from = 0;
        while let Some(i) = t[from..].find(k) {
            let at = from + i;
            from = at + k.len();
            // Must start a key: line start or after a space.
            if at > 0 && !t.as_bytes()[at - 1].is_ascii_whitespace() {
                continue;
            }
            let v = t[from..].trim_start_matches('"');
            let end = v
                .find(|c: char| c.is_whitespace() || c == '"')
                .unwrap_or(v.len());
            if let Some(l) = word(&v[..end]) {
                return Some(l);
            }
        }
    }
    None
}

/// Looks at the first few words. Timestamps, dates, pids and other tokens
/// that start with a digit are skipped.
fn prefix_level(t: &str) -> Option<Level> {
    let head = match t.char_indices().nth(120) {
        Some((i, _)) => &t[..i],
        None => t,
    };
    let mut seen = 0;
    for raw in head.split(|c: char| c.is_whitespace() || c == '|') {
        if raw.is_empty() {
            continue;
        }
        // glog: E1114 12:00:00.000 ...
        let b = raw.as_bytes();
        if b.len() == 5 && b[1..].iter().all(u8::is_ascii_digit) {
            if let Some(l) = match b[0] {
                b'E' | b'F' => Some(Level::Error),
                b'W' => Some(Level::Warn),
                b'I' => Some(Level::Info),
                b'D' => Some(Level::Debug),
                _ => None,
            } {
                return Some(l);
            }
        }
        if raw.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        if seen >= 5 {
            break;
        }
        seen += 1;
        let bracketed = raw.starts_with(['[', '<', '(']);
        let core = raw.trim_matches(|c: char| matches!(c, '[' | ']' | '<' | '>' | '(' | ')' | ','));
        let colon = core.ends_with(':') || raw.ends_with("]:");
        // `ERROR:root:message` (Python logging) -> first part.
        let core = core.split(':').next().unwrap_or(core);
        let core = core.trim_end_matches(['.', '-']);
        if core.is_empty() {
            continue;
        }
        let Some(l) = word(core) else { continue };
        // Single letters only in brackets (`[E]`, `<W>`).
        if core.len() == 1 && !bracketed {
            continue;
        }
        let upper = core.bytes().all(|c| !c.is_ascii_lowercase());
        // "LOG:" is postgres info; "log" alone is just a word.
        if (upper && (core.len() > 1)) || bracketed || colon {
            return Some(l);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use Level::*;

    #[test]
    fn json_levels() {
        assert_eq!(detect(r#"{"level":"error","msg":"x"}"#), Error);
        assert_eq!(detect(r#"{"msg":"x", "lvl" : "WARN"}"#), Warn);
        assert_eq!(detect(r#"{"severity":"INFO"}"#), Info);
        assert_eq!(detect(r#"{"level":30,"msg":"pino"}"#), Info);
        assert_eq!(detect(r#"{"level":50}"#), Error);
        assert_eq!(detect(r#"{"levelname":"DEBUG"}"#), Debug);
        assert_eq!(detect(r#"{"@l":"Warning"}"#), Warn);
        assert_eq!(detect(r#"{"msg":"no level"}"#), Other);
        // A key that only appears in a value still finds the real key later.
        assert_eq!(
            detect(r#"{"msg":"\"level\" mentioned","level":"debug"}"#),
            Debug
        );
    }

    #[test]
    fn logfmt_levels() {
        assert_eq!(detect("ts=2026 level=warn msg=\"disk slow\""), Warn);
        assert_eq!(detect("time=x lvl=error"), Error);
        assert_eq!(detect(r#"level="info" msg=hi"#), Info);
        assert_eq!(detect("sublevel=error not a key"), Other);
    }

    #[test]
    fn prefixes() {
        assert_eq!(detect("ERROR something broke"), Error);
        assert_eq!(detect("2026-11-14 21:30:00,123 ERROR odoo: boom"), Error);
        assert_eq!(
            detect("2026/11/14 21:30:00 [warn] 12#12: upstream slow"),
            Warn
        );
        assert_eq!(detect("[error] nginx"), Error);
        assert_eq!(detect("error: cannot open file"), Error);
        assert_eq!(detect("WARNING:root:careful"), Warn);
        assert_eq!(detect("E1114 21:30:00.000000 1 main.go:1] fail"), Error);
        assert_eq!(detect("I1114 21:30:00.000000 1 main.go:1] ok"), Info);
        assert_eq!(
            detect("2026-11-14 21:30:00.000 UTC [1] LOG:  checkpoint"),
            Info
        );
        assert_eq!(
            detect("2026-11-14 21:30:00.000 UTC [1] FATAL:  role"),
            Error
        );
        assert_eq!(detect("<W> watch out"), Warn);
        assert_eq!(detect("[DEBUG] details"), Debug);
        assert_eq!(detect("time=\"x\" INFO started"), Info);
        assert_eq!(detect("TRACE deep"), Debug);
    }

    #[test]
    fn prose_is_not_a_level() {
        assert_eq!(detect("connection error while reading"), Other);
        assert_eq!(detect("the info you asked for"), Other);
        assert_eq!(
            detect("a b c d e f ERROR late"),
            Other,
            "only the first words"
        );
        assert_eq!(detect("E is a letter"), Other);
        assert_eq!(detect(""), Other);
        assert_eq!(detect("GET /index.html 200"), Other);
    }
}
