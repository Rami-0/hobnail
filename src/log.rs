//! Tiny structured logger: one JSON object per line on stderr.
//!
//! Deliberately not `tracing`: we need a handful of log lines, not spans, and
//! every crate costs binary size and RAM. Never pass secrets as fields.

use std::fmt::Display;
use std::io::Write;

pub fn emit(level: &str, msg: &str, fields: &[(&str, &dyn Display)]) {
    let mut line = String::with_capacity(128);
    line.push_str("{\"ts\":\"");
    line.push_str(&crate::util::fmt_utc(crate::util::unix_now()));
    line.push_str("\",\"level\":\"");
    line.push_str(level);
    line.push_str("\",\"msg\":");
    line.push_str(&json_str(msg));
    for (k, v) in fields {
        line.push(',');
        line.push_str(&json_str(k));
        line.push(':');
        line.push_str(&json_str(&v.to_string()));
    }
    line.push_str("}\n");
    let _ = std::io::stderr().lock().write_all(line.as_bytes());
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

#[macro_export]
macro_rules! info {
    ($msg:expr $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::log::emit("info", &$msg, &[$((stringify!($k), &$v as &dyn std::fmt::Display)),*])
    };
}

#[macro_export]
macro_rules! warn {
    ($msg:expr $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::log::emit("warn", &$msg, &[$((stringify!($k), &$v as &dyn std::fmt::Display)),*])
    };
}

#[macro_export]
macro_rules! error {
    ($msg:expr $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::log::emit("error", &$msg, &[$((stringify!($k), &$v as &dyn std::fmt::Display)),*])
    };
}
