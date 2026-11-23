//! Push-based JSON scanner for streamed bodies ([`Conn::scan`]).
//!
//! It checks the document's structure and reports each value with the keys
//! of its enclosing objects, without building the document. State is fixed
//! size: a depth stack, the keys of the first [`KEY_DEPTH`] levels and one
//! scalar, each truncated to [`CAPTURE`] bytes. A visitor keeps whatever
//! bounded state it needs (a counter, a few numbers).

pub(super) const MAX_DEPTH: usize = 32;
pub(super) const KEY_DEPTH: usize = 4;
pub(super) const CAPTURE: usize = 64;

/// A value reported to [`Visit::value`].
pub(super) enum Scalar<'a> {
    /// The start of an object or array; its members follow.
    Container {
        object: bool,
    },
    /// A string, truncated to [`CAPTURE`] bytes when `complete` is false.
    Str {
        text: &'a str,
        complete: bool,
    },
    /// The literal text of a number (longer numbers are not complete).
    Number {
        text: &'a str,
        complete: bool,
    },
    Bool,
    Null,
}

/// Where a value sits: its depth and the member keys that lead to it.
pub(super) struct Path<'a> {
    depth: usize,
    objects: &'a [bool; MAX_DEPTH],
    keys: &'a [Capture; KEY_DEPTH],
}

impl Path<'_> {
    /// 0 for the root value, 1 for its members, and so on.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// The member key at `level` (1-based) when that level is an object and
    /// the key was captured whole.
    pub fn key(&self, level: usize) -> Option<&str> {
        if level == 0 || level > self.depth || level > KEY_DEPTH || !self.objects[level - 1] {
            return None;
        }
        self.keys[level - 1]
            .text()
            .filter(|_| !self.keys[level - 1].overflow)
    }

    /// Whether the value is a member of the root array.
    pub fn in_root_array(&self) -> bool {
        self.depth == 1 && !self.objects[0]
    }
}

pub(super) trait Visit {
    fn value(&mut self, path: &Path<'_>, value: Scalar<'_>);
}

#[derive(Clone, Copy)]
pub(super) struct Capture {
    bytes: [u8; CAPTURE],
    len: usize,
    overflow: bool,
}

impl Capture {
    const EMPTY: Self = Self {
        bytes: [0; CAPTURE],
        len: 0,
        overflow: false,
    };

    fn push(&mut self, bytes: &[u8]) {
        if self.len + bytes.len() <= CAPTURE {
            self.bytes[self.len..self.len + bytes.len()].copy_from_slice(bytes);
            self.len += bytes.len();
        } else {
            self.overflow = true;
        }
    }

    fn push_char(&mut self, c: char) {
        self.push(c.encode_utf8(&mut [0; 4]).as_bytes());
    }

    fn text(&self) -> Option<&str> {
        std::str::from_utf8(&self.bytes[..self.len]).ok()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// Expect a value; `true` right after '[' where ']' may close it.
    Value(bool),
    /// Expect a key; `true` right after '{' where '}' may close it.
    Key(bool),
    Colon,
    After,
    String {
        key: bool,
        escape: Escape,
    },
    Number,
    Literal {
        word: &'static [u8],
        at: usize,
    },
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Escape {
    None,
    Backslash,
    Unicode { digits: u8, code: u32 },
}

pub(super) struct Scanner<V> {
    pub visitor: V,
    state: State,
    depth: usize,
    objects: [bool; MAX_DEPTH],
    keys: [Capture; KEY_DEPTH],
    scalar: Capture,
}

#[derive(Debug)]
pub(super) struct Invalid;

impl<V: Visit> Scanner<V> {
    pub fn new(visitor: V) -> Self {
        Self {
            visitor,
            state: State::Value(false),
            depth: 0,
            objects: [false; MAX_DEPTH],
            keys: [Capture::EMPTY; KEY_DEPTH],
            scalar: Capture::EMPTY,
        }
    }

    fn report(&mut self, value: Scalar<'_>) {
        let path = Path {
            depth: self.depth,
            objects: &self.objects,
            keys: &self.keys,
        };
        self.visitor.value(&path, value);
    }

    fn report_scalar(&mut self, number: bool) {
        let capture = self.scalar;
        let text = capture.text().unwrap_or_else(|| {
            // A multi-byte character cut at the capture limit.
            let valid = std::str::from_utf8(&capture.bytes[..capture.len])
                .err()
                .map_or(0, |e| e.valid_up_to());
            std::str::from_utf8(&capture.bytes[..valid]).unwrap_or_default()
        });
        let complete = !capture.overflow && capture.text().is_some();
        self.report(if number {
            Scalar::Number { text, complete }
        } else {
            Scalar::Str { text, complete }
        });
    }

    fn ended(&mut self) {
        self.state = if self.depth == 0 {
            State::Done
        } else {
            State::After
        };
    }

    fn open(&mut self, object: bool) -> Result<(), Invalid> {
        self.report(Scalar::Container { object });
        if self.depth == MAX_DEPTH {
            return Err(Invalid);
        }
        self.objects[self.depth] = object;
        self.depth += 1;
        self.state = if object {
            State::Key(true)
        } else {
            State::Value(true)
        };
        Ok(())
    }

    fn close(&mut self, object: bool) -> Result<(), Invalid> {
        if self.depth == 0 || self.objects[self.depth - 1] != object {
            return Err(Invalid);
        }
        self.depth -= 1;
        self.ended();
        Ok(())
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<(), Invalid> {
        for &byte in chunk {
            self.byte(byte)?;
        }
        Ok(())
    }

    /// Whether the input so far is exactly one complete JSON value.
    pub fn finish(&mut self) -> Result<(), Invalid> {
        if self.state == State::Number {
            self.report_scalar(true);
            self.ended();
        }
        if self.state == State::Done {
            Ok(())
        } else {
            Err(Invalid)
        }
    }

    fn byte(&mut self, byte: u8) -> Result<(), Invalid> {
        let space = matches!(byte, b' ' | b'\t' | b'\n' | b'\r');
        match self.state {
            State::String { key, escape } => return self.string(key, escape, byte),
            State::Literal { word, at } => {
                if word.get(at) != Some(&byte) {
                    return Err(Invalid);
                }
                if at + 1 < word.len() {
                    self.state = State::Literal { word, at: at + 1 };
                    return Ok(());
                }
                self.report(if word == b"null" {
                    Scalar::Null
                } else {
                    Scalar::Bool
                });
                self.ended();
                return Ok(());
            }
            State::Number => {
                if matches!(byte, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
                    self.scalar.push(&[byte]);
                    return Ok(());
                }
                self.report_scalar(true);
                self.ended();
            }
            _ => {}
        }
        if space {
            return Ok(());
        }
        match (self.state, byte) {
            (State::Value(_), b'{') => self.open(true),
            (State::Value(_), b'[') => self.open(false),
            (State::Value(true), b']') => self.close(false),
            (State::Value(_), b'"') => {
                self.scalar = Capture::EMPTY;
                self.state = State::String {
                    key: false,
                    escape: Escape::None,
                };
                Ok(())
            }
            (State::Value(_), b'-' | b'0'..=b'9') => {
                self.scalar = Capture::EMPTY;
                self.scalar.push(&[byte]);
                self.state = State::Number;
                Ok(())
            }
            (State::Value(_), b't' | b'f' | b'n') => {
                let word: &'static [u8] = match byte {
                    b't' => b"true",
                    b'f' => b"false",
                    _ => b"null",
                };
                self.state = State::Literal { word, at: 1 };
                Ok(())
            }
            (State::Key(_), b'"') => {
                if let Some(key) = self.keys.get_mut(self.depth - 1) {
                    *key = Capture::EMPTY;
                }
                self.state = State::String {
                    key: true,
                    escape: Escape::None,
                };
                Ok(())
            }
            (State::Key(true), b'}') => self.close(true),
            (State::Colon, b':') => {
                self.state = State::Value(false);
                Ok(())
            }
            (State::After, b',') if self.depth > 0 => {
                self.state = if self.objects[self.depth - 1] {
                    State::Key(false)
                } else {
                    State::Value(false)
                };
                Ok(())
            }
            (State::After, b'}') => self.close(true),
            (State::After, b']') => self.close(false),
            _ => Err(Invalid),
        }
    }

    fn capture(&mut self, key: bool) -> Option<&mut Capture> {
        if key {
            self.keys.get_mut(self.depth - 1)
        } else {
            Some(&mut self.scalar)
        }
    }

    fn string(&mut self, key: bool, escape: Escape, byte: u8) -> Result<(), Invalid> {
        let next = match escape {
            Escape::None => match byte {
                b'"' => {
                    if key {
                        self.state = State::Colon;
                    } else {
                        self.report_scalar(false);
                        self.ended();
                    }
                    return Ok(());
                }
                b'\\' => Escape::Backslash,
                0..=0x1f => return Err(Invalid),
                _ => {
                    if let Some(capture) = self.capture(key) {
                        capture.push(&[byte]);
                    }
                    Escape::None
                }
            },
            Escape::Backslash => {
                let decoded = match byte {
                    b'"' => '"',
                    b'\\' => '\\',
                    b'/' => '/',
                    b'b' => '\u{8}',
                    b'f' => '\u{c}',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    b'u' => {
                        self.state = State::String {
                            key,
                            escape: Escape::Unicode { digits: 0, code: 0 },
                        };
                        return Ok(());
                    }
                    _ => return Err(Invalid),
                };
                if let Some(capture) = self.capture(key) {
                    capture.push_char(decoded);
                }
                Escape::None
            }
            Escape::Unicode { digits, code } => {
                let digit = char::from(byte).to_digit(16).ok_or(Invalid)?;
                let code = code * 16 + digit;
                if digits < 3 {
                    Escape::Unicode {
                        digits: digits + 1,
                        code,
                    }
                } else {
                    // Surrogate pairs are not joined; they read as U+FFFD.
                    let decoded = char::from_u32(code).unwrap_or('\u{fffd}');
                    if let Some(capture) = self.capture(key) {
                        capture.push_char(decoded);
                    }
                    Escape::None
                }
            }
        };
        self.state = State::String { key, escape: next };
        Ok(())
    }
}

/// Counts the members of a root array.
#[derive(Default)]
pub(super) struct ArrayLength {
    pub count: u64,
    pub not_array: bool,
}

impl Visit for ArrayLength {
    fn value(&mut self, path: &Path<'_>, value: Scalar<'_>) {
        if path.depth() == 0 && !matches!(value, Scalar::Container { object: false }) {
            self.not_array = true;
        } else if path.in_root_array() {
            self.count += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Record(Vec<String>);

    impl Visit for Record {
        fn value(&mut self, path: &Path<'_>, value: Scalar<'_>) {
            let keys: Vec<_> = (1..=path.depth())
                .map(|level| path.key(level).unwrap_or("[]"))
                .collect();
            let value = match value {
                Scalar::Container { .. } => "{}".into(),
                Scalar::Str { text, complete } => {
                    format!("{text:?}{}", if complete { "" } else { "…" })
                }
                Scalar::Number { text, .. } => text.into(),
                Scalar::Bool => "bool".into(),
                Scalar::Null => "null".into(),
            };
            self.0.push(format!("{}={value}", keys.join(".")));
        }
    }

    fn scan(input: &str, chunk: usize) -> Result<Vec<String>, Invalid> {
        let mut scanner = Scanner::new(Record::default());
        for part in input.as_bytes().chunks(chunk) {
            scanner.feed(part)?;
        }
        scanner.finish()?;
        Ok(scanner.visitor.0)
    }

    #[test]
    fn reports_values_with_their_keys_across_any_chunking() {
        let input = r#" {"a":[1,-2.5e3,{"b":"x\"\u00e9y"}],"c":true,"d":null,"e":{}, "f":[]} "#;
        let expected = [
            "={}",
            "a={}",
            "a.[]=1",
            "a.[]=-2.5e3",
            "a.[]={}",
            r#"a.[].b="x\"éy""#,
            "c=bool",
            "d=null",
            "e={}",
            "f={}",
        ];
        for chunk in 1..input.len() {
            assert_eq!(scan(input, chunk).unwrap(), expected);
        }
        assert_eq!(scan("7", 1).unwrap(), ["=7"]);
        let long = format!(
            r#"{{"{}":"{}"}}"#,
            "k".repeat(CAPTURE + 1),
            "é".repeat(CAPTURE)
        );
        let values = scan(&long, 7).unwrap();
        assert!(values[1].starts_with("[]=\"éé") && values[1].ends_with('…'));
    }

    #[test]
    fn rejects_malformed_documents() {
        for input in [
            "",
            "{",
            "[1,]",
            "[1 2]",
            "{\"a\"}",
            "{\"a\":1,}",
            "{1:2}",
            "[}",
            "tru",
            "nul ",
            "\"a",
            "\"\\x\"",
            "\"\u{1}\"",
            "1 2",
            "[]]",
            "{\"a\":1}}",
            "\"\\u12g4\"",
        ] {
            assert!(scan(input, 3).is_err(), "{input:?}");
        }
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert!(scan(&deep, 64).is_err());
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(scan(&ok, 64).is_ok());
    }

    #[test]
    fn counts_root_array_members_in_constant_space() {
        let mut scanner = Scanner::new(ArrayLength::default());
        scanner.feed(b"[").unwrap();
        let item = br#"{"hash":"x","nested":[1,2,{"a":[]}],"name":"\u00e9 \"q\""},"#;
        for _ in 0..100_000 {
            scanner.feed(item).unwrap();
        }
        scanner.feed(b"{}]").unwrap();
        scanner.finish().unwrap();
        assert_eq!(scanner.visitor.count, 100_001);
        assert!(!scanner.visitor.not_array);
        // About 7 MB of input passed through a scanner of fixed size.
        assert!(std::mem::size_of::<Scanner<ArrayLength>>() < 512);
    }
}
