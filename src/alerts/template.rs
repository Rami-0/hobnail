//! `{{name}}` templates for alert messages and webhook bodies.

use std::collections::BTreeMap;

pub type Vars = BTreeMap<String, String>;

/// Every variable a message, recovery or body template may use.
pub const VARS: &[&str] = &[
    "rule", "subject", "state", "value", "time", "host", "title", "message", "kind", "duration",
];

/// Replace `{{name}}` (spaces inside the braces allowed) with `vars[name]`.
/// Unknown names are left as they are. With `json`, inserted values are
/// JSON-string-escaped (without the surrounding quotes), so a template like
/// `{"text":"{{message}}"}` stays valid JSON whatever the message contains.
pub fn render(tpl: &str, vars: &Vars, json: bool) -> String {
    let mut out = String::with_capacity(tpl.len() + 64);
    let mut rest = tpl;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let name = after[..end].trim();
                match vars.get(name) {
                    Some(v) if json => out.push_str(&json_escape(v)),
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[start..start + 2 + end + 2]),
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Names used in a template, in order (duplicates kept).
pub fn placeholders(tpl: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = tpl;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        names.push(after[..end].trim());
        rest = &after[end + 2..];
    }
    names
}

/// Check that a template only uses known variables. `allowed` defaults to [`VARS`].
pub fn check(tpl: &str, allowed: &[&str]) -> Result<(), String> {
    for n in placeholders(tpl) {
        if !allowed.contains(&n) {
            return Err(format!(
                "unknown template variable {{{{{n}}}}} (known: {})",
                allowed.join(", ")
            ));
        }
    }
    Ok(())
}

fn json_escape(s: &str) -> String {
    serde_json::to_string(s)
        .map(|q| q[1..q.len() - 1].to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        [
            ("subject", "odoo"),
            ("state", "stopped (exit 137)"),
            ("q", "a\"b\\c\nd"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn renders_and_keeps_unknown() {
        assert_eq!(
            render("🔴 {{subject}}: {{ state }} {{nope}} {{", &vars(), false),
            "🔴 odoo: stopped (exit 137) {{nope}} {{"
        );
        assert_eq!(render("no vars", &vars(), false), "no vars");
        assert_eq!(render("{{subject}}{{subject}}", &vars(), false), "odooodoo");
    }

    #[test]
    fn json_escapes_values() {
        let out = render(r#"{"t":"{{q}}"}"#, &vars(), true);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["t"], "a\"b\\c\nd");
        assert_eq!(render("{{q}}", &Vars::new(), true), "{{q}}");
    }

    #[test]
    fn checks_names() {
        assert!(check("{{subject}} {{time}}", VARS).is_ok());
        let e = check("{{subjet}}", VARS).unwrap_err();
        assert!(e.contains("subjet"), "{e}");
        assert_eq!(placeholders("{{a}} x {{ b }}"), vec!["a", "b"]);
    }
}
