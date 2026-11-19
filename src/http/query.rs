//! Percent encoding and query parsing for HTTP routes.

pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                match h {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode everything except unreserved characters.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Parse `a=1&b=two%20words` into pairs (first occurrence wins in `get`).
pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.trim_start_matches('?')
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (url_decode(k), url_decode(v)),
            None => (url_decode(p), String::new()),
        })
        .collect()
}

pub fn query_get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_roundtrip() {
        let q = parse_query("?q=a%20b+c&state=running&x=%zz&flag&q=second");
        assert_eq!(query_get(&q, "q"), Some("a b c"));
        assert_eq!(query_get(&q, "state"), Some("running"));
        assert_eq!(query_get(&q, "x"), Some("%zz"));
        assert_eq!(query_get(&q, "flag"), Some(""));
        assert_eq!(query_get(&q, "none"), None);
        assert_eq!(url_decode("%e2%82%ac"), "€");
        assert_eq!(url_decode("100%"), "100%");
        assert_eq!(url_decode("%4"), "%4");
        assert_eq!(url_encode("a b&c=é"), "a%20b%26c%3D%C3%A9");
        assert_eq!(url_decode(&url_encode("x y/z?€")), "x y/z?€");
    }
}
