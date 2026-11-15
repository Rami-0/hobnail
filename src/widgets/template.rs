//! One-line text templates: `"{{unread}} / {{messages}}"`,
//! `"{{size | bytes}}"`, `"{{load | round:1 | default:\"n/a\"}}"`.
//!
//! Filters (applied left to right):
//! - `bytes`      number → `1.5 GiB`
//! - `duration`   seconds → `3d 4h`, `5h 2m`, `7m`, `12s`
//! - `round:N`    N decimals (default 0)
//! - `percent`    append `%`
//! - `number`     thousands separators: `1234567` → `1,234,567`
//! - `upper`, `lower`
//! - `default:"x"` use `x` when the value is missing (null) or empty
//!
//! A missing value renders as "—". The output is plain text: the caller
//! escapes it when building HTML.

use serde_json::Value;
use std::collections::BTreeMap;

pub const MISSING: &str = "—";
/// Longest text we put on the page for one value.
const MAX_TEXT: usize = 300;

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    Bytes,
    Duration,
    Round(usize),
    Percent,
    Number,
    Upper,
    Lower,
    Default(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Part {
    Lit(String),
    Expr { name: String, filters: Vec<Filter> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Template(Vec<Part>);

impl Template {
    pub fn parse(s: &str) -> Result<Template, String> {
        let mut parts = Vec::new();
        let mut rest = s;
        while let Some(start) = rest.find("{{") {
            if start > 0 {
                parts.push(Part::Lit(rest[..start].to_string()));
            }
            let after = &rest[start + 2..];
            let end = find_close(after).ok_or("unclosed {{ in template")?;
            parts.push(parse_expr(&after[..end])?);
            rest = &after[end + 2..];
        }
        if !rest.is_empty() {
            parts.push(Part::Lit(rest.to_string()));
        }
        Ok(Template(parts))
    }

    /// Names referenced by `{{name}}`.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().filter_map(|p| match p {
            Part::Expr { name, .. } => Some(name.as_str()),
            Part::Lit(_) => None,
        })
    }

    pub fn render(&self, values: &BTreeMap<String, Value>) -> String {
        let mut out = String::new();
        for p in &self.0 {
            match p {
                Part::Lit(s) => out.push_str(s),
                Part::Expr { name, filters } => {
                    let v = values.get(name).unwrap_or(&Value::Null);
                    out.push_str(&apply(v, filters));
                }
            }
        }
        out
    }
}

/// Position of the `}}` that closes an expression, skipping quoted strings.
fn find_close(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut in_q = false;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if in_q => i += 1,
            b'"' => in_q = !in_q,
            b'}' if !in_q && b.get(i + 1) == Some(&b'}') => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Split on `|` outside quotes.
fn split_pipes(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut in_q, mut esc, mut start) = (false, false, 0);
    for (i, c) in s.char_indices() {
        match c {
            _ if esc => esc = false,
            '\\' if in_q => esc = true,
            '"' => in_q = !in_q,
            '|' if !in_q => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn parse_expr(inner: &str) -> Result<Part, String> {
    let mut pieces = split_pipes(inner).into_iter();
    let name = pieces.next().unwrap_or("").trim().to_string();
    if name.is_empty() {
        return Err("empty {{ }} in template".into());
    }
    if !valid_name(&name) {
        return Err(format!("bad name {name:?} in template"));
    }
    let filters = pieces.map(parse_filter).collect::<Result<Vec<_>, _>>()?;
    Ok(Part::Expr { name, filters })
}

/// Names are field names (`[A-Za-z0-9_-]`) or the built-ins `_status` etc.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn parse_filter(s: &str) -> Result<Filter, String> {
    let s = s.trim();
    let (name, arg) = match s.split_once(':') {
        Some((n, a)) => (n.trim(), Some(a.trim())),
        None => (s, None),
    };
    let no_arg = |f: Filter| match arg {
        None => Ok(f),
        Some(_) => Err(format!("filter {name} takes no argument")),
    };
    match name {
        "bytes" => no_arg(Filter::Bytes),
        "duration" => no_arg(Filter::Duration),
        "percent" => no_arg(Filter::Percent),
        "number" => no_arg(Filter::Number),
        "upper" => no_arg(Filter::Upper),
        "lower" => no_arg(Filter::Lower),
        "round" => match arg {
            None => Ok(Filter::Round(0)),
            Some(a) => match a.parse::<usize>() {
                Ok(n) if n <= 6 => Ok(Filter::Round(n)),
                _ => Err(format!("round:{a}: expected 0..6 decimals")),
            },
        },
        "default" => {
            let a = arg.ok_or("default needs a value: default:\"x\"")?;
            Ok(Filter::Default(unquote(a)?))
        }
        "" => Err("empty filter after |".into()),
        other => Err(format!(
            "unknown filter {other:?} (known: bytes, duration, round:N, percent, number, upper, lower, default:\"x\")"
        )),
    }
}

fn unquote(a: &str) -> Result<String, String> {
    let Some(inner) = a.strip_prefix('"') else {
        return Ok(a.to_string());
    };
    let inner = inner
        .strip_suffix('"')
        .ok_or("unterminated string in filter")?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn apply(v: &Value, filters: &[Filter]) -> String {
    let mut cur = v.clone();
    for f in filters {
        cur = apply_one(cur, f);
    }
    if cur.is_null() {
        return MISSING.to_string();
    }
    truncate(text(&cur))
}

fn apply_one(v: Value, f: &Filter) -> Value {
    if let Filter::Default(d) = f {
        return match &v {
            Value::Null => Value::String(d.clone()),
            Value::String(s) if s.is_empty() => Value::String(d.clone()),
            _ => v,
        };
    }
    if v.is_null() {
        return v;
    }
    let num = as_f64(&v);
    let s = match (f, num) {
        (Filter::Bytes, Some(n)) if n >= 0.0 => crate::util::human_bytes(n as u64),
        (Filter::Duration, Some(n)) if n >= 0.0 => crate::util::human_duration(n as u64),
        (Filter::Round(d), Some(n)) => format!("{n:.d$}", d = *d),
        (Filter::Number, Some(_)) => group_thousands(&text(&v)),
        (Filter::Percent, _) => format!("{}%", text(&v)),
        (Filter::Upper, _) => text(&v).to_uppercase(),
        (Filter::Lower, _) => text(&v).to_lowercase(),
        // Numeric filter on a non-number: leave it as it is.
        _ => return v,
    };
    Value::String(s)
}

/// Numbers, and strings that hold a number.
pub fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}

/// Plain-text form of a JSON value (strings without quotes).
pub fn text(v: &Value) -> String {
    match v {
        Value::Null => MISSING.to_string(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

/// Text shown for a value on the page: "—" for null, shortened.
pub fn display(v: &Value) -> String {
    if v.is_null() {
        MISSING.to_string()
    } else {
        truncate(text(v))
    }
}

fn truncate(mut s: String) -> String {
    if s.chars().count() > MAX_TEXT {
        s = s.chars().take(MAX_TEXT).collect::<String>() + "…";
    }
    s
}

/// `1234567.5` → `1,234,567.5` (only the leading run of digits is grouped).
fn group_thousands(s: &str) -> String {
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => ("-", r),
        None => ("", s),
    };
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits <= 3 {
        return s.to_string();
    }
    let (int, tail) = rest.split_at(digits);
    let mut out = String::from(sign);
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (digits - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.push_str(tail);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vals(v: Value) -> BTreeMap<String, Value> {
        match v {
            Value::Object(m) => m.into_iter().collect(),
            _ => BTreeMap::new(),
        }
    }

    fn r(t: &str, v: Value) -> String {
        Template::parse(t).unwrap().render(&vals(v))
    }

    #[test]
    fn renders_and_filters() {
        let v = json!({
            "unread": 3, "messages": 1234567, "size": 1610612736u64, "up": 273600,
            "f": 2.71666, "s": "pass", "empty": "", "n": null, "t": true, "neg": -1234.5,
            "numstr": "2048", "obj": {"a": 1}
        });
        assert_eq!(r("{{unread}} / {{messages}}", v.clone()), "3 / 1234567");
        assert_eq!(r("{{ messages | number }}", v.clone()), "1,234,567");
        assert_eq!(r("{{size|bytes}}", v.clone()), "1.5 GiB");
        assert_eq!(r("{{numstr|bytes}}", v.clone()), "2.0 KiB");
        assert_eq!(r("{{up | duration}}", v.clone()), "3d 4h");
        assert_eq!(r("{{f | round:2}}", v.clone()), "2.72");
        assert_eq!(r("{{f | round}}", v.clone()), "3");
        assert_eq!(r("{{f | round:1 | percent}}", v.clone()), "2.7%");
        assert_eq!(r("{{s | upper}}", v.clone()), "PASS");
        assert_eq!(r("{{s | upper | lower}}", v.clone()), "pass");
        assert_eq!(r("{{t}}", v.clone()), "true");
        assert_eq!(r("{{neg | number}}", v.clone()), "-1,234.5");
        assert_eq!(r("{{obj}}", v.clone()), r#"{"a":1}"#);
        assert_eq!(r("{{n}}", v.clone()), "—");
        assert_eq!(r("{{nothere | bytes}}", v.clone()), "—");
        assert_eq!(r(r#"{{n | default:"n/a"}}"#, v.clone()), "n/a");
        assert_eq!(r(r#"{{empty | default:"a|b}}"}}"#, v.clone()), "a|b}}");
        assert_eq!(r("{{n | default:none | upper}}", v.clone()), "NONE");
        assert_eq!(r("{{s | bytes}}", v.clone()), "pass");
        assert_eq!(r("no placeholders", v.clone()), "no placeholders");
        assert_eq!(r("a } b }} c", v.clone()), "a } b }} c");
        assert_eq!(r("<b>{{s}}</b>", v), "<b>pass</b>"); // escaping is the caller's job
    }

    #[test]
    fn names() {
        let t = Template::parse("{{a}} {{ b | bytes }} {{_status}}").unwrap();
        assert_eq!(t.names().collect::<Vec<_>>(), ["a", "b", "_status"]);
    }

    #[test]
    fn parse_errors() {
        for t in [
            "{{a",
            "{{}}",
            "{{ a | nope }}",
            "{{ a | round:x }}",
            "{{ a | round:9 }}",
            "{{ a | bytes:1 }}",
            "{{ a | default }}",
            "{{ a | }}",
            "{{ a b }}",
            r#"{{ a | default:"x }}"#,
        ] {
            assert!(Template::parse(t).is_err(), "{t} should not parse");
        }
        let e = Template::parse("{{ a | nope }}").unwrap_err();
        assert!(e.contains("nope") && e.contains("bytes"), "{e}");
    }

    #[test]
    fn thousands() {
        assert_eq!(group_thousands("1"), "1");
        assert_eq!(group_thousands("123"), "123");
        assert_eq!(group_thousands("1234"), "1,234");
        assert_eq!(group_thousands("123456"), "123,456");
        assert_eq!(group_thousands("-1234567.891"), "-1,234,567.891");
        assert_eq!(group_thousands("1e21"), "1e21");
    }
}
