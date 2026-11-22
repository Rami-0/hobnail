use std::collections::HashSet;

use super::{connection, MAX_RESPONSE};

pub(super) const MAX_SECRETS: usize = 32;
const MAX_SECRET_BYTES: usize = 16 * 1024;
const MAX_LAYERS: usize = 3;
pub(super) const MARKER: &str = "<redacted>";

/// The only derived needles are the binary-to-text forms. Decode textual
/// escapes on the output side so mixed encoders share the same matching path.
pub(super) struct OutputRedactor(Option<Vec<Needle>>);

impl OutputRedactor {
    pub(super) fn new(values: impl Iterator<Item = impl AsRef<str>>) -> Self {
        let mut secrets = HashSet::new();
        for value in values {
            let value = value.as_ref();
            if value.is_empty() || secrets.contains(value) {
                continue;
            }
            if value.len() > MAX_SECRET_BYTES || secrets.len() == MAX_SECRETS {
                return Self(None);
            }
            secrets.insert(value.to_owned());
        }
        let mut needles = HashSet::new();
        for secret in secrets {
            let standard = connection::base64(&secret);
            let url = standard.replace('+', "-").replace('/', "_");
            for encoded in [standard, url] {
                needles.insert(encoded.trim_end_matches('=').to_owned());
                needles.insert(encoded);
            }
            needles.insert(secret);
        }
        Self(Some(needles.into_iter().map(Needle::new).collect()))
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.as_ref().is_some_and(Vec::is_empty)
    }

    fn spans(&self, text: &str) -> Vec<bool> {
        let mut matched = vec![false; text.len()];
        let Some(needles) = &self.0 else {
            matched.fill(true);
            return matched;
        };
        let mut mark = |bytes: &[u8], origins: &[(u32, u32)]| {
            let mut base = 0;
            for chunk in bytes.utf8_chunks() {
                let text = chunk.valid();
                for needle in needles
                    .iter()
                    .filter(|needle| needle.value.len() <= text.len())
                {
                    let mut rightmost = 0;
                    needle.for_each_match(text, |index| {
                        let start = base + index;
                        let end = start + needle.value.len();
                        let (left, right) = if origins.is_empty() {
                            (start, end)
                        } else {
                            (origins[start].0 as usize, origins[end - 1].1 as usize)
                        };
                        // Avoid repeatedly filling overlapping spans.
                        matched[left.max(rightmost).min(right)..right].fill(true);
                        rightmost = right;
                    });
                }
                base += text.len() + chunk.invalid().len();
            }
        };
        mark(text.as_bytes(), &[]);
        if !text.contains(['%', '+', '\\', '&']) {
            return matched;
        }
        // '+' is literal in RFC 3986, and space in form encoding. Interpret
        // raw plus in each layer while preserving a percent-decoded plus.
        for form in [false, true] {
            let mut view = View {
                bytes: text.as_bytes().to_vec(),
                origins: (0..text.len() as u32)
                    .map(|start| (start, start + 1))
                    .collect(),
            };
            for _ in 0..MAX_LAYERS {
                let Some(next) = view.decode(form) else {
                    break;
                };
                mark(&next.bytes, &next.origins);
                view = next;
            }
        }
        matched
    }

    pub(super) fn matches(&self, text: &str) -> bool {
        !self.is_empty()
            && (text.len() > MAX_RESPONSE || self.spans(text).into_iter().any(|byte| byte))
    }

    pub(super) fn redact(&self, text: &mut String) -> bool {
        if self.is_empty() {
            return false;
        }
        if text.len() > MAX_RESPONSE || self.0.is_none() {
            text.clear();
            return true;
        }
        let mut matched = self.spans(text);
        if !matched.iter().any(|&byte| byte) {
            return false;
        }
        // Decoded matches can begin or end in a multi-byte escape or character.
        for (index, ch) in text.char_indices() {
            let bytes = &mut matched[index..index + ch.len_utf8()];
            if bytes.iter().any(|&byte| byte) {
                bytes.fill(true);
            }
        }
        let mut result = String::with_capacity(text.len());
        let mut start = 0;
        for (end, _) in text.char_indices().skip(1).chain([(text.len(), '\0')]) {
            if end == text.len() || matched[end] != matched[start] {
                result.push_str(if matched[start] {
                    MARKER
                } else {
                    &text[start..end]
                });
                if result.len() > MAX_RESPONSE {
                    text.clear();
                    return true;
                }
                start = end;
            }
        }
        if self.matches(&result) {
            result.clear();
        }
        *text = result;
        true
    }
}

struct Needle {
    value: String,
    // Only needed when two occurrences of this needle can overlap.
    prefix: Vec<u16>,
}

impl Needle {
    fn new(value: String) -> Self {
        let bytes = value.as_bytes();
        let mut prefix = vec![0; bytes.len()];
        for index in 1..bytes.len() {
            prefix[index] =
                Self::extend(bytes, &prefix, usize::from(prefix[index - 1]), bytes[index]) as u16;
        }
        if prefix.last() == Some(&0) {
            prefix = Vec::new();
        }
        Self { value, prefix }
    }

    fn extend(bytes: &[u8], prefix: &[u16], mut matched: usize, byte: u8) -> usize {
        while matched > 0 && bytes[matched] != byte {
            matched = usize::from(prefix[matched - 1]);
        }
        matched + usize::from(bytes[matched] == byte)
    }

    fn for_each_match(&self, text: &str, mut found: impl FnMut(usize)) {
        if self.prefix.is_empty() {
            for (index, _) in text.match_indices(&self.value) {
                found(index);
            }
            return;
        }
        // Prefix fallback finds every overlapping match in a single pass.
        let bytes = self.value.as_bytes();
        let mut matched = 0;
        for (index, byte) in text.bytes().enumerate() {
            matched = Self::extend(bytes, &self.prefix, matched, byte);
            if matched == bytes.len() {
                found(index + 1 - matched);
                matched = usize::from(self.prefix[matched - 1]);
            }
        }
    }
}

struct View {
    bytes: Vec<u8>,
    // Original span of each decoded byte. Bytes emitted by one escape
    // share its entire span.
    origins: Vec<(u32, u32)>,
}

impl View {
    fn decode(&self, form: bool) -> Option<Self> {
        let mut next = Self {
            bytes: Vec::with_capacity(self.bytes.len()),
            origins: Vec::with_capacity(self.origins.len()),
        };
        let mut changed = false;
        let mut index = 0;
        while index < self.bytes.len() {
            let rest = &self.bytes[index..];
            let mut utf8 = [0; 4];
            let decoded = match rest[0] {
                b'%' => hex(rest.get(1..3)).map(|n| {
                    utf8[0] = n as u8;
                    (3, &utf8[..1])
                }),
                b'+' if form => Some((1, &b" "[..])),
                b'\\' => {
                    json_escape(rest).map(|(len, ch)| (len, ch.encode_utf8(&mut utf8).as_bytes()))
                }
                b'&' => entity(rest).map(|(len, ch)| (len, ch.encode_utf8(&mut utf8).as_bytes())),
                _ => None,
            };
            let (len, bytes) = decoded.unwrap_or((1, &rest[..1]));
            changed |= decoded.is_some();
            let span = (self.origins[index].0, self.origins[index + len - 1].1);
            index += len;
            next.bytes.extend_from_slice(bytes);
            next.origins.extend(std::iter::repeat_n(span, bytes.len()));
        }
        changed.then_some(next)
    }
}

fn hex(bytes: Option<&[u8]>) -> Option<u32> {
    bytes?.iter().try_fold(0u32, |n, &b| {
        n.checked_mul(16)?.checked_add(char::from(b).to_digit(16)?)
    })
}

fn json_escape(rest: &[u8]) -> Option<(usize, char)> {
    let ch = match *rest.get(1)? {
        b'u' => {
            let first = hex(rest.get(2..6))?;
            if (0xd800..=0xdbff).contains(&first) && rest.get(6..8) == Some(b"\\u") {
                let second = hex(rest.get(8..12))?;
                if (0xdc00..=0xdfff).contains(&second) {
                    return Some((
                        12,
                        char::from_u32(0x10000 + ((first - 0xd800) << 10) + second - 0xdc00)?,
                    ));
                }
            }
            return Some((6, char::from_u32(first)?));
        }
        b'"' => '"',
        b'\\' => '\\',
        b'/' => '/',
        b'b' => '\x08',
        b'f' => '\x0c',
        b'n' => '\n',
        b'r' => '\r',
        b't' => '\t',
        _ => return None,
    };
    Some((2, ch))
}

fn entity(rest: &[u8]) -> Option<(usize, char)> {
    let end = rest.iter().take(24).position(|&b| b == b';')?;
    let name = std::str::from_utf8(&rest[1..end]).ok()?;
    let ch = match name {
        "amp" | "AMP" => '&',
        "lt" | "LT" => '<',
        "gt" | "GT" => '>',
        "quot" | "QUOT" => '"',
        "apos" => '\'',
        "nbsp" | "NonBreakingSpace" => '\u{a0}',
        "Tab" => '\t',
        "NewLine" => '\n',
        "excl" => '!',
        "num" => '#',
        "dollar" => '$',
        "percnt" => '%',
        "lpar" => '(',
        "rpar" => ')',
        "ast" | "midast" => '*',
        "plus" => '+',
        "comma" => ',',
        "period" => '.',
        "sol" => '/',
        "colon" => ':',
        "semi" => ';',
        "equals" => '=',
        "quest" => '?',
        "commat" => '@',
        "lsqb" | "lbrack" => '[',
        "bsol" => '\\',
        "rsqb" | "rbrack" => ']',
        "Hat" => '^',
        "lowbar" | "UnderBar" => '_',
        "grave" | "DiacriticalGrave" => '`',
        "lcub" | "lbrace" => '{',
        "verbar" | "vert" | "VerticalLine" => '|',
        "rcub" | "rbrace" => '}',
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(digits) => u32::from_str_radix(digits, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some((end + 1, ch))
}

#[cfg(test)]
pub(in crate::providers) mod tests {
    use super::*;

    /// Ordinary encodings of the shared ASCII fixture, including the distinct
    /// form and RFC 3986 escapes for tilde and asterisk.
    pub(in crate::providers) fn encoded_echoes(secret: &str) -> Vec<String> {
        assert_eq!(secret, "xy~*>&/!");
        let base64 = connection::base64(secret);
        let base64url = base64.replace('+', "-").replace('/', "_");
        vec![
            secret.into(),
            "xy%7e*%3e%26%2f%21".into(),
            "xy~%2A%3E%26%2F%21".into(),
            r"xy~*>&/!".into(),
            "xy&#126;&#x2A;&gt;&amp;&#47;!".into(),
            base64.trim_end_matches('=').into(),
            base64url.trim_end_matches('=').into(),
            base64,
            base64url,
        ]
    }

    #[test]
    fn maps_mixed_escapes_to_original_spans() {
        let redactor = OutputRedactor::new(["x~* +&😀", "admin:x~* +&😀"].into_iter());
        for secret in ["x~* +&😀", "admin:x~* +&😀"] {
            let basic = connection::base64(secret);
            let url = basic.replace('+', "-").replace('/', "_");
            for encoded in [
                &basic,
                basic.trim_end_matches('='),
                &url,
                url.trim_end_matches('='),
            ] {
                let mut value = format!("before:{encoded}:after");
                assert!(redactor.redact(&mut value));
                assert_eq!(value, "before:<redacted>:after");
            }
        }
        for encoded in [
            "x%7e*+%2B%26%F0%9F%98%80",
            "x~%2A%20%2b%26%f0%9F%98%80",
            r"x\u007e\u002a\u0020\u002b\u0026\ud83d\ude00",
            "x&#126;&#x2a; +&amp;&#128512;",
            r"x%257E*&#32;%2B&amp;\ud83d\ude00",
        ] {
            let mut value = format!("before:{encoded}:after");
            assert!(redactor.redact(&mut value));
            assert_eq!(value, "before:<redacted>:after");
        }
    }

    #[test]
    fn merges_overlapping_secret_echoes() {
        let redactor = OutputRedactor::new(["abab"].into_iter());
        let mut text = "pre:ababab:post".to_owned();
        assert!(redactor.redact(&mut text));
        assert_eq!(text, "pre:<redacted>:post");
    }

    #[test]
    fn withholds_text_over_the_bounds() {
        let mut overlong = "a".repeat(MAX_RESPONSE + 1);
        assert!(OutputRedactor::new(["test"].into_iter()).redact(&mut overlong));
        assert!(overlong.is_empty());
        let redactor = OutputRedactor::new((0..=MAX_SECRETS).map(|n| n.to_string()));
        assert!(redactor.matches("https://example.test/"));
        assert!(redactor.redact(&mut "display".to_owned()));
    }
}
