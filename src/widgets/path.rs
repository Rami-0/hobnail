//! Path expressions into a JSON document.
//!
//! ```text
//! ""  or  "$"          the whole document
//! a.b.c                object keys
//! items[0]  items[-1]  array index (negative counts from the end)
//! items.0              same as items[0] when `items` is an array
//! items.#              length of an array / object / string
//! a."x.y"  a["x.y"]    keys with odd characters (`\"` and `\\` escape)
//! ```
//! A path that does not match evaluates to `null`; it is never an error.

use serde_json::Value;
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Key(String),
    Index(i64),
    Len,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Path(pub Vec<Seg>);

impl Path {
    pub fn parse(s: &str) -> Result<Path, String> {
        let s = s.trim();
        let rest = match s.strip_prefix('$') {
            Some(r) if r.is_empty() || r.starts_with('.') || r.starts_with('[') => {
                r.strip_prefix('.').unwrap_or(r)
            }
            _ => s,
        };
        let chars: Vec<char> = rest.chars().collect();
        let mut segs = Vec::new();
        let mut i = 0;
        // True when a segment must follow (start, or right after a '.').
        let mut need = !chars.is_empty();
        while i < chars.len() {
            let c = chars[i];
            if c == '[' {
                i += 1;
                if chars.get(i) == Some(&'"') {
                    let (key, n) = quoted(&chars[i..])?;
                    i += n;
                    segs.push(Seg::Key(key));
                } else {
                    let start = i;
                    while i < chars.len() && chars[i] != ']' {
                        i += 1;
                    }
                    let inner: String = chars[start..i].iter().collect();
                    let inner = inner.trim();
                    if inner == "#" {
                        segs.push(Seg::Len);
                    } else {
                        let n = inner
                            .parse::<i64>()
                            .map_err(|_| format!("bad index [{inner}] (use [\"key\"] for keys)"))?;
                        segs.push(Seg::Index(n));
                    }
                }
                if chars.get(i) != Some(&']') {
                    return Err("missing ]".into());
                }
                i += 1;
                need = false;
            } else if c == '.' {
                if need {
                    return Err("empty path segment".into());
                }
                i += 1;
                need = true;
                if i == chars.len() {
                    return Err("path ends with '.'".into());
                }
            } else {
                if !need {
                    return Err(format!("expected '.' or '[' before {c:?}"));
                }
                if c == '"' {
                    let (key, n) = quoted(&chars[i..])?;
                    i += n;
                    segs.push(Seg::Key(key));
                } else {
                    let start = i;
                    while i < chars.len() && chars[i] != '.' && chars[i] != '[' {
                        if chars[i] == '"' || chars[i] == ']' {
                            return Err(format!("unexpected {:?} in key", chars[i]));
                        }
                        i += 1;
                    }
                    let key: String = chars[start..i].iter().collect();
                    segs.push(if key == "#" { Seg::Len } else { Seg::Key(key) });
                }
                need = false;
            }
        }
        Ok(Path(segs))
    }

    /// Evaluate against `root`. Missing → `Value::Null`.
    pub fn eval(&self, root: &Value) -> Value {
        let mut cur: Cow<Value> = Cow::Borrowed(root);
        for seg in &self.0 {
            let next = match (seg, cur.as_ref()) {
                (Seg::Key(k), Value::Object(m)) => m.get(k).cloned(),
                (Seg::Key(k), Value::Array(a)) => k.parse::<i64>().ok().and_then(|n| index(a, n)),
                (Seg::Index(n), Value::Array(a)) => index(a, *n),
                (Seg::Len, Value::Array(a)) => Some(Value::from(a.len())),
                (Seg::Len, Value::Object(m)) => Some(Value::from(m.len())),
                (Seg::Len, Value::String(s)) => Some(Value::from(s.chars().count())),
                _ => None,
            };
            match next {
                Some(v) => cur = Cow::Owned(v),
                None => return Value::Null,
            }
        }
        cur.into_owned()
    }
}

fn index(a: &[Value], n: i64) -> Option<Value> {
    let len = a.len() as i64;
    let i = if n < 0 { len + n } else { n };
    if (0..len).contains(&i) {
        a.get(i as usize).cloned()
    } else {
        None
    }
}

/// Parse a `"..."` string at the start of `c`; returns (key, chars consumed).
fn quoted(c: &[char]) -> Result<(String, usize), String> {
    let mut out = String::new();
    let mut i = 1;
    while i < c.len() {
        match c[i] {
            '"' => return Ok((out, i + 1)),
            '\\' if i + 1 < c.len() => {
                out.push(c[i + 1]);
                i += 2;
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    Err("unterminated quoted key".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn get(doc: &Value, p: &str) -> Value {
        Path::parse(p).unwrap().eval(doc)
    }

    #[test]
    fn evaluates() {
        let doc = json!({
            "a": {"b": {"c": 3}},
            "items": [{"name": "x"}, {"name": "y"}, {"name": "z"}],
            "x.y": 1,
            "odd": {"we\"ird": 2},
            "s": "héllo",
            "RuntimeStats": {"Uptime": 12345}
        });
        assert_eq!(get(&doc, "a.b.c"), json!(3));
        assert_eq!(get(&doc, "$.a.b.c"), json!(3));
        assert_eq!(get(&doc, "items[0].name"), json!("x"));
        assert_eq!(get(&doc, "items[-1].name"), json!("z"));
        assert_eq!(get(&doc, "items.1.name"), json!("y"));
        assert_eq!(get(&doc, "items.#"), json!(3));
        assert_eq!(get(&doc, "items[#]"), json!(3));
        assert_eq!(get(&doc, "a.#"), json!(1));
        assert_eq!(get(&doc, "s.#"), json!(5));
        assert_eq!(get(&doc, r#""x.y""#), json!(1));
        assert_eq!(get(&doc, r#"["x.y"]"#), json!(1));
        assert_eq!(get(&doc, r#"odd."we\"ird""#), json!(2));
        assert_eq!(get(&doc, "RuntimeStats.Uptime"), json!(12345));
        assert_eq!(get(&doc, ""), doc);
        assert_eq!(get(&doc, "$"), doc);
        // Missing → null, never an error.
        assert_eq!(get(&doc, "a.nope.c"), Value::Null);
        assert_eq!(get(&doc, "items[9]"), Value::Null);
        assert_eq!(get(&doc, "items[-9]"), Value::Null);
        assert_eq!(get(&doc, "a.b.c.d"), Value::Null);
        assert_eq!(get(&doc, "items.#.x"), Value::Null);
        assert_eq!(get(&json!([1, 2]), "[1]"), json!(2));
        assert_eq!(get(&json!({"$x": 1}), "$x"), json!(1));
    }

    #[test]
    fn rejects_bad_paths() {
        for p in [
            "a..b", "a.", ".a", "a[", "a[x]", "a[0", r#"a."b"#, "a]b", r#"a"b""#,
        ] {
            assert!(Path::parse(p).is_err(), "{p} should not parse");
        }
    }
}
