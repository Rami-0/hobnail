//! Read the RFC 5280 fields displayed by the TLS probe. Rustls separately
//! checks certificate trust and the signature proving possession of the key.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::{SubjectAltName, TlsLeaf, TlsVerification};
use crate::providers::{Class, Fault};

const MAX_LEAF: usize = 32 * 1024;
const MAX_NAMES: usize = 64;

pub(super) fn read(
    bytes: &[u8],
    fingerprint: [u8; 32],
    verification: TlsVerification,
) -> Result<TlsLeaf, Fault> {
    if bytes.len() > MAX_LEAF {
        return Err(Class::TooLarge.into());
    }
    parse(bytes, fingerprint, verification).ok_or_else(|| Class::Protocol.into())
}

fn parse(bytes: &[u8], fingerprint: [u8; 32], verification: TlsVerification) -> Option<TlsLeaf> {
    let mut certificate = Der(bytes).read(0x30)?;
    let mut tbs = certificate.read(0x30)?;
    if tbs.starts_with(0xa0) {
        tbs.read(0xa0)?;
    }
    tbs.read(0x02)?; // Serial.
    tbs.read(0x30)?; // Signature algorithm.
    let issuer = name(tbs.read(0x30)?)?;
    let mut validity = tbs.read(0x30)?;
    let not_before = time(&mut validity)?;
    let not_after = time(&mut validity)?;
    let subject = name(tbs.read(0x30)?)?;
    tbs.read(0x30)?; // Subject public key.
    for tag in [0x81, 0x82] {
        if tbs.starts_with(tag) {
            tbs.read(tag)?; // Optional issuer and subject unique IDs.
        }
    }
    let sans = if tbs.0.is_empty() {
        Vec::new()
    } else {
        subject_alt_names(tbs.read(0xa3)?.read(0x30)?)?
    };
    Some(TlsLeaf {
        subject,
        issuer,
        not_before,
        not_after,
        sans,
        fingerprint,
        verification,
    })
}

struct Der<'a>(&'a [u8]);

impl<'a> Der<'a> {
    fn starts_with(&self, tag: u8) -> bool {
        self.0.first() == Some(&tag)
    }

    fn read(&mut self, tag: u8) -> Option<Self> {
        let (actual, value) = self.next()?;
        (actual == tag).then_some(value)
    }

    fn next(&mut self) -> Option<(u8, Self)> {
        let (&tag, rest) = self.0.split_first()?;
        let (&len, mut rest) = rest.split_first()?;
        let len = if len < 128 {
            usize::from(len)
        } else {
            let n = usize::from(len & 0x7f);
            if !(1..=2).contains(&n) {
                return None;
            }
            let (len, tail) = rest.split_at_checked(n)?;
            rest = tail;
            len.iter()
                .fold(0usize, |value, &byte| (value << 8) | usize::from(byte))
        };
        let (value, remaining) = rest.split_at_checked(len)?;
        self.0 = remaining;
        Some((tag, Self(value)))
    }

    fn text(&self) -> Option<String> {
        Some(std::str::from_utf8(self.0).ok()?.to_owned())
    }
}

fn time(value: &mut Der<'_>) -> Option<i64> {
    let (tag, bytes) = value.next()?;
    let raw = std::str::from_utf8(bytes.0).ok()?.strip_suffix('Z')?;
    if !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (year, rest) = match (tag, raw.len()) {
        (0x17, 12) => {
            let (year, rest) = raw.split_at(2);
            let year = year.parse::<u16>().ok()?;
            (year + if year < 50 { 2000 } else { 1900 }, rest)
        }
        (0x18, 14) => (raw[..4].parse().ok()?, &raw[4..]),
        _ => return None,
    };
    let (month, day) = (
        rest[..2].parse::<u16>().ok()?,
        rest[2..4].parse::<u16>().ok()?,
    );
    let days = match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !(1..=days).contains(&day) {
        return None;
    }
    let iso = format!(
        "{year:04}-{}-{}T{}:{}:{}Z",
        &rest[..2],
        &rest[2..4],
        &rest[4..6],
        &rest[6..8],
        &rest[8..10]
    );
    crate::logs::frame::parse_rfc3339_parts(&iso).map(|(seconds, _)| seconds)
}

fn name(mut sequence: Der<'_>) -> Option<String> {
    use std::fmt::Write as _;

    let mut sets = Vec::new();
    let mut count = 0;
    while !sequence.0.is_empty() {
        let mut set = sequence.read(0x31)?;
        let mut entries = Vec::new();
        while !set.0.is_empty() {
            count += 1;
            if count > MAX_NAMES {
                return None;
            }
            let mut entry = set.read(0x30)?;
            let oid = entry.read(0x06)?.0;
            let label = match oid {
                [0x55, 4, 3] => "CN".into(),
                [0x55, 4, 6] => "C".into(),
                [0x55, 4, 7] => "L".into(),
                [0x55, 4, 8] => "ST".into(),
                [0x55, 4, 10] => "O".into(),
                [0x55, 4, 11] => "OU".into(),
                _ => oid_text(oid)?,
            };
            let (tag, value) = entry.next()?;
            let mut display = format!("{label}=");
            let text = match tag {
                0x0c | 0x13 | 0x16 => value.text(),
                0x1e if value.0.len() % 2 == 0 => String::from_utf16(
                    &value
                        .0
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .copied()
                        .map(u16::from_be_bytes)
                        .collect::<Vec<_>>(),
                )
                .ok(),
                _ => None,
            };
            if let Some(text) = text {
                for ch in text.chars() {
                    match ch {
                        ',' | '+' | '=' | '\\' => {
                            let _ = write!(display, "\\{ch}");
                        }
                        ch if ch.is_control() => display.extend(ch.escape_default()),
                        ch => display.push(ch),
                    }
                }
            } else {
                display.push('#');
                for byte in value.0 {
                    let _ = write!(display, "{byte:02x}");
                }
            }
            entries.push(display);
        }
        sets.push(entries.join("+"));
    }
    Some(sets.join(", "))
}

fn oid_text(bytes: &[u8]) -> Option<String> {
    use std::fmt::Write as _;

    let mut out = String::new();
    let mut component = 0u64;
    for &byte in bytes {
        component = component
            .checked_mul(128)?
            .checked_add(u64::from(byte & 0x7f))?;
        if byte < 128 {
            if out.is_empty() {
                let first = (component / 40).min(2);
                let _ = write!(out, "{first}.{}", component - first * 40);
            } else {
                let _ = write!(out, ".{component}");
            }
            component = 0;
        }
    }
    (bytes.last()? & 0x80 == 0).then_some(out)
}

fn subject_alt_names(mut extensions: Der<'_>) -> Option<Vec<SubjectAltName>> {
    let mut result = Vec::new();
    while !extensions.0.is_empty() {
        let mut extension = extensions.read(0x30)?;
        if extension.read(0x06)?.0 != [0x55, 0x1d, 0x11] {
            continue;
        }
        if extension.starts_with(0x01) {
            extension.read(0x01)?;
        }
        let mut names = extension.read(0x04)?.read(0x30)?;
        while !names.0.is_empty() {
            if result.len() == MAX_NAMES {
                return None;
            }
            let (tag, mut value) = names.next()?;
            result.push(match tag {
                0x81 => SubjectAltName::Email(value.text()?),
                0x82 => SubjectAltName::Dns(value.text()?),
                0x86 => SubjectAltName::Uri(value.text()?),
                0x87 => SubjectAltName::Ip(match value.0.len() {
                    4 => IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(value.0).ok()?)),
                    16 => IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(value.0).ok()?)),
                    _ => return None,
                }),
                0xa4 => SubjectAltName::DirectoryName(name(value.read(0x30)?)?),
                0x88 => SubjectAltName::RegisteredId(oid_text(value.0)?),
                _ => SubjectAltName::Other(tag),
            });
        }
    }
    Some(result)
}
