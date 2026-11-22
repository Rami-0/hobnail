//! Bounded UTF-8 feed scanner. No declarations, external resources, or
//! recursively expanded entities. Names intentionally use the ASCII subset.
const ATTR_LIMIT: usize = 1024;
const ATTR_COUNT: usize = 24;

pub(super) struct Tag<'a> {
    pub name: &'a str,
    pub attrs: Vec<(&'a str, String)>,
    pub empty: bool,
}

impl Tag<'_> {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }
}

pub(super) enum Event<'a> {
    Open(Tag<'a>),
    Close(&'a str),
    Text(&'a str, bool),
    Skip,
}

pub(super) struct Scanner<'a>(pub &'a str);

fn space(c: char) -> bool {
    matches!(c, ' ' | '\n' | '\r' | '\t')
}

fn name(input: &str) -> Option<(&str, &str)> {
    let end = input
        .find(|c: char| !c.is_ascii_alphanumeric() && !":-._".contains(c))
        .unwrap_or(input.len());
    let value = &input[..end];
    let valid = |part: &str| part.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
    if end > 128 || !value.split(':').all(valid) || value.matches(':').count() > 1 {
        return None;
    }
    Some((value, &input[end..]))
}

impl<'a> Scanner<'a> {
    pub fn next(&mut self) -> Option<Event<'a>> {
        let mut rest = self.0;
        if let Some((prefix, end)) = [("<!--", "-->"), ("<?", "?>"), ("<![CDATA[", "]]>")]
            .into_iter()
            .find(|(prefix, _)| rest.starts_with(prefix))
        {
            let (value, after) = rest[prefix.len()..].split_once(end)?;
            self.0 = after;
            return Some(if prefix == "<![CDATA[" {
                Event::Text(value, false)
            } else {
                Event::Skip
            });
        }
        if let Some(close) = rest.strip_prefix("</") {
            let (name, after) = name(close)?;
            self.0 = after.trim_start_matches(space).strip_prefix('>')?;
            return Some(Event::Close(name));
        }
        if let Some(open) = rest.strip_prefix('<') {
            let (tag_name, after) = name(open)?; // Any other <!… declaration fails here.
            rest = after;
            let mut attrs: Vec<(&str, String)> = Vec::new();
            loop {
                let before = rest.len();
                rest = rest.trim_start_matches(space);
                let empty = rest.starts_with("/>");
                if empty || rest.starts_with('>') {
                    self.0 = &rest[if empty { 2 } else { 1 }..];
                    return Some(Event::Open(Tag {
                        name: tag_name,
                        attrs,
                        empty,
                    }));
                }
                if attrs.len() == ATTR_COUNT || rest.len() == before {
                    return None;
                }
                let (key, after) = name(rest)?;
                if attrs.iter().any(|(previous, _)| *previous == key) {
                    return None;
                }
                rest = after
                    .trim_start_matches(space)
                    .strip_prefix('=')?
                    .trim_start_matches(space);
                let quote = rest.chars().next().filter(|q| matches!(q, '\'' | '"'))?;
                let (value, after) = rest[1..].split_once(quote)?;
                if value.len() > ATTR_LIMIT || value.contains('<') {
                    return None;
                }
                let mut decoded = String::new();
                append(value, true, &mut decoded, ATTR_LIMIT)?;
                attrs.push((key, decoded));
                rest = after;
            }
        }
        let end = rest.find('<').unwrap_or(rest.len());
        self.0 = &rest[end..];
        Some(Event::Text(&rest[..end], true))
    }
}

fn character(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

pub(super) fn append(
    mut text: &str,
    entities: bool,
    output: &mut String,
    max: usize,
) -> Option<()> {
    let mut push = |c: char| {
        if !character(c) {
            return None;
        }
        // Leave enough room for the largest character so trimming never skips
        // a multi-byte character and then admits later ASCII.
        if output.len() < max.saturating_sub(3) {
            output.push(c);
        }
        Some(())
    };
    if entities {
        while let Some((plain, rest)) = text.split_once('&') {
            for c in plain.chars() {
                push(c)?;
            }
            let (entity, _) =
                rest[..rest.floor_char_boundary(rest.len().min(16))].split_once(';')?;
            let c = match entity {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let (number, radix) = entity.strip_prefix("#x").map_or_else(
                        || entity.strip_prefix('#').map(|digits| (digits, 10)),
                        |digits| Some((digits, 16)),
                    )?;
                    if !number.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return None;
                    }
                    char::from_u32(u32::from_str_radix(number, radix).ok()?)?
                }
            };
            push(c)?;
            text = &rest[entity.len() + 1..];
        }
    }
    for c in text.chars() {
        push(c)?;
    }
    Some(())
}
