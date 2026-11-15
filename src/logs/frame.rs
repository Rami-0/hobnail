//! Docker log stream parsing (pure, unit-tested).
//!
//! Non-TTY containers send a multiplexed stream: each frame is an 8-byte
//! header `[stream, 0, 0, 0, len u32 BE]` followed by `len` payload bytes.
//! TTY containers send the raw bytes. Frames and lines can be split at any
//! byte across reads, and a frame can be huge, so nothing here buffers more
//! than one capped line per stream: the payload is streamed through a line
//! splitter that keeps at most [`MAX_LINE`] bytes and marks the rest as cut.

/// Longest line kept (bytes); the rest of the line is dropped and the line
/// is marked truncated.
pub const MAX_LINE: usize = 16 * 1024;

/// 1 = stdout, 2 = stderr (0 = stdin is shown as stdout).
pub type StreamId = u8;

/// One complete log line as raw bytes (no trailing `\n` / `\r\n`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLine {
    pub stream: StreamId,
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

/// Splits a byte stream into lines, capping each at `MAX_LINE`.
#[derive(Debug, Default)]
struct Splitter {
    buf: Vec<u8>,
    truncated: bool,
}

impl Splitter {
    fn push(&mut self, stream: StreamId, mut data: &[u8], out: &mut Vec<RawLine>) {
        while !data.is_empty() {
            match memchr(b'\n', data) {
                Some(i) => {
                    self.take(&data[..i]);
                    out.push(self.finish(stream));
                    data = &data[i + 1..];
                }
                None => {
                    self.take(data);
                    break;
                }
            }
        }
    }

    fn take(&mut self, part: &[u8]) {
        let room = MAX_LINE.saturating_sub(self.buf.len());
        if part.len() > room {
            self.buf.extend_from_slice(&part[..room]);
            self.truncated = true;
        } else {
            self.buf.extend_from_slice(part);
        }
    }

    fn finish(&mut self, stream: StreamId) -> RawLine {
        let mut bytes = std::mem::take(&mut self.buf);
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        // Keep the allocation small between lines of a chatty container.
        if self.buf.capacity() == 0 && bytes.capacity() > 4096 {
            bytes.shrink_to_fit();
        }
        RawLine {
            stream,
            bytes,
            truncated: std::mem::take(&mut self.truncated),
        }
    }

    fn flush(&mut self, stream: StreamId, out: &mut Vec<RawLine>) {
        if !self.buf.is_empty() || self.truncated {
            out.push(self.finish(stream));
        }
    }
}

fn memchr(b: u8, s: &[u8]) -> Option<usize> {
    s.iter().position(|&c| c == b)
}

/// Incremental parser for one container's log stream.
#[derive(Debug)]
pub struct Parser {
    tty: bool,
    head: [u8; 8],
    head_len: usize,
    /// Payload bytes left in the current frame.
    left: u64,
    stream: StreamId,
    out: Splitter,
    err: Splitter,
}

impl Parser {
    /// `tty`: the container was started with a TTY (raw stream).
    pub fn new(tty: bool) -> Self {
        Self {
            tty,
            head: [0; 8],
            head_len: 0,
            left: 0,
            stream: 1,
            out: Splitter::default(),
            err: Splitter::default(),
        }
    }

    fn splitter(&mut self, s: StreamId) -> &mut Splitter {
        if s == 2 {
            &mut self.err
        } else {
            &mut self.out
        }
    }

    /// Feed bytes from the socket; complete lines are appended to `out`.
    pub fn push(&mut self, mut data: &[u8], out: &mut Vec<RawLine>) {
        if self.tty {
            self.out.push(1, data, out);
            return;
        }
        while !data.is_empty() {
            if self.left == 0 {
                let need = 8 - self.head_len;
                let n = need.min(data.len());
                self.head[self.head_len..self.head_len + n].copy_from_slice(&data[..n]);
                self.head_len += n;
                data = &data[n..];
                if self.head_len < 8 {
                    return;
                }
                self.head_len = 0;
                let h = self.head;
                if h[0] > 2 || h[1] != 0 || h[2] != 0 || h[3] != 0 {
                    // Not a multiplexed header: the container is really a
                    // TTY (or the daemon changed format). Fall back to raw
                    // and keep the bytes we took for a header.
                    self.tty = true;
                    self.out.push(1, &h, out);
                    self.out.push(1, data, out);
                    return;
                }
                self.stream = if h[0] == 2 { 2 } else { 1 };
                self.left = u32::from_be_bytes([h[4], h[5], h[6], h[7]]) as u64;
                continue;
            }
            let n = (self.left.min(data.len() as u64)) as usize;
            let s = self.stream;
            self.splitter(s).push(s, &data[..n], out);
            self.left -= n as u64;
            data = &data[n..];
        }
    }

    /// End of stream: emit unterminated last lines.
    pub fn finish(&mut self, out: &mut Vec<RawLine>) {
        self.out.flush(1, out);
        self.err.flush(2, out);
    }
}

/// Split Docker's `timestamps=1` prefix (`2026-11-14T21:30:00.123456789Z `)
/// off a line. Returns unix nanoseconds and the rest of the line.
pub fn split_timestamp(line: &[u8]) -> (Option<i64>, &[u8]) {
    let sp = line.iter().take(40).position(|&b| b == b' ');
    let (head, rest) = match sp {
        Some(i) => (&line[..i], &line[i + 1..]),
        None => (line, &line[line.len()..]),
    };
    match std::str::from_utf8(head).ok().and_then(parse_rfc3339) {
        Some(ns) => (Some(ns), rest),
        None => (None, line),
    }
}

/// `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)` to unix nanoseconds.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let t = s.get(r)?;
        if !t.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        t.parse().ok()
    };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut i = 19;
    let mut frac: i64 = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let mut digits = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            if digits < 9 {
                frac = frac * 10 + (b[i] - b'0') as i64;
                digits += 1;
            }
            i += 1;
        }
        for _ in digits..9 {
            frac *= 10;
        }
    }
    let off = match b.get(i)? {
        b'Z' | b'z' if i + 1 == b.len() => 0,
        c @ (b'+' | b'-') if i + 6 == b.len() && b[i + 3] == b':' => {
            let oh = num(i + 1..i + 3)?;
            let om = num(i + 4..i + 6)?;
            let o = (oh * 60 + om) * 60;
            if *c == b'+' {
                o
            } else {
                -o
            }
        }
        _ => return None,
    };
    let days = days_from_civil(y, mo, d);
    let secs = days * 86_400 + h * 3600 + mi * 60 + sec - off;
    secs.checked_mul(1_000_000_000)?.checked_add(frac)
}

/// Days since 1970-01-01 (proleptic Gregorian), H. Hinnant's algorithm.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Remove ANSI escape sequences (CSI and OSC) and other C0 controls
/// except tab, for level detection, matching and plain-text downloads.
pub fn strip_ansi(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.bytes().any(|b| b == 0x1b || (b < 0x20 && b != b'\t')) {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            match it.peek() {
                Some('[') => {
                    it.next();
                    // Parameters and intermediates, then one final byte.
                    for c in it.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    it.next();
                    // OSC: until BEL or ESC \.
                    while let Some(c) = it.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' {
                            if it.peek() == Some(&'\\') {
                                it.next();
                            }
                            break;
                        }
                    }
                }
                Some(_) => {
                    it.next();
                }
                None => {}
            }
        } else if (c as u32) < 0x20 && c != '\t' {
            continue;
        } else {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(stream: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![stream, 0, 0, 0];
        v.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        v.extend_from_slice(payload);
        v
    }

    fn lines(p: &mut Parser, chunks: &[&[u8]]) -> Vec<RawLine> {
        let mut out = vec![];
        for c in chunks {
            p.push(c, &mut out);
        }
        p.finish(&mut out);
        out
    }

    fn text(l: &RawLine) -> String {
        String::from_utf8_lossy(&l.bytes).into_owned()
    }

    #[test]
    fn multiplexed_frames_split_at_every_byte() {
        let mut data = frame(1, b"hello\nwor");
        data.extend(frame(2, b"oops\n"));
        data.extend(frame(1, b"ld\r\n"));
        // Feed one byte at a time: headers and lines split everywhere.
        let mut p = Parser::new(false);
        let mut out = vec![];
        for b in &data {
            p.push(std::slice::from_ref(b), &mut out);
        }
        p.finish(&mut out);
        let got: Vec<(u8, String)> = out.iter().map(|l| (l.stream, text(l))).collect();
        assert_eq!(
            got,
            vec![
                (1, "hello".to_string()),
                (2, "oops".to_string()),
                (1, "world".to_string()),
            ]
        );
        // And in every two-piece split.
        for cut in 0..data.len() {
            let mut p = Parser::new(false);
            let out = lines(&mut p, &[&data[..cut], &data[cut..]]);
            assert_eq!(out.len(), 3, "cut at {cut}");
            assert_eq!(text(&out[2]), "world");
        }
    }

    #[test]
    fn huge_frame_is_capped_and_marked() {
        let big = vec![b'x'; MAX_LINE * 3 + 5];
        let mut payload = big.clone();
        payload.extend_from_slice(b"\nnext\n");
        let data = frame(1, &payload);
        let mut p = Parser::new(false);
        let out = lines(&mut p, &data.chunks(1000).collect::<Vec<_>>());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].bytes.len(), MAX_LINE);
        assert!(out[0].truncated);
        assert_eq!(text(&out[1]), "next");
        assert!(!out[1].truncated);
    }

    #[test]
    fn invalid_utf8_is_lossy_never_a_panic() {
        let data = frame(1, b"bad \xff\xfe bytes \xe2\x82\n");
        let mut p = Parser::new(false);
        let out = lines(&mut p, &[&data]);
        assert_eq!(out.len(), 1);
        let s = String::from_utf8_lossy(&out[0].bytes);
        assert!(s.starts_with("bad "));
        assert!(s.contains('\u{fffd}'));
        // A multi-byte char split across frames survives.
        let mut d = frame(1, b"caf\xc3");
        d.extend(frame(1, b"\xa9\n"));
        let out = lines(&mut Parser::new(false), &[&d]);
        assert_eq!(text(&out[0]), "café");
    }

    #[test]
    fn tty_is_raw_with_crlf() {
        let mut p = Parser::new(true);
        let out = lines(&mut p, &[b"one\r\ntw", b"o\r\n", b"tail"]);
        let got: Vec<String> = out.iter().map(text).collect();
        assert_eq!(got, vec!["one", "two", "tail"]);
        assert!(out.iter().all(|l| l.stream == 1));
    }

    #[test]
    fn non_multiplexed_stream_falls_back_to_raw() {
        let mut p = Parser::new(false);
        let out = lines(&mut p, &[b"plain text line\nsecond\n"]);
        let got: Vec<String> = out.iter().map(text).collect();
        assert_eq!(got, vec!["plain text line", "second"]);
    }

    #[test]
    fn empty_frames_and_empty_lines() {
        let mut d = frame(1, b"");
        d.extend(frame(1, b"\n\n"));
        let out = lines(&mut Parser::new(false), &[&d]);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|l| l.bytes.is_empty()));
    }

    #[test]
    fn timestamps() {
        let (ts, rest) = split_timestamp(b"2026-11-14T21:30:00.123456789Z hello world");
        assert_eq!(ts, Some(1_794_691_800_123_456_789));
        assert_eq!(rest, b"hello world");
        let (ts, _) = split_timestamp(b"1970-01-01T00:00:01Z x");
        assert_eq!(ts, Some(1_000_000_000));
        let (ts, _) = split_timestamp(b"1970-01-01T01:00:01+01:00 x");
        assert_eq!(ts, Some(1_000_000_000));
        let (ts, rest) = split_timestamp(b"no timestamp here");
        assert_eq!(ts, None);
        assert_eq!(rest, b"no timestamp here");
        assert_eq!(parse_rfc3339("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_rfc3339("2026-11-14T21:30:00"), None);
    }

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(strip_ansi("\x1b[31;1mred\x1b[0m plain"), "red plain");
        assert_eq!(strip_ansi("\x1b]0;title\x07after"), "after");
        assert_eq!(strip_ansi("tab\tok\x08"), "tab\tok");
        assert!(matches!(strip_ansi("plain"), std::borrow::Cow::Borrowed(_)));
    }
}
