use super::*;
use widgets::{RichBlock, RichKind, RichSpan, StructuredBody};

const MAX_BYTES: usize = 8192;
const MAX_BLOCKS: usize = 64;
const MAX_SPANS: usize = 160;

const NOTE: Field = Field {
    required: true,
    max_length: Some(MAX_BYTES),
    ..Field::new(
        "note",
        "Markdown",
        Kind::Multiline,
        "Headings, emphasis, lists, code and links. Maximum 8 KiB.",
    )
};

pub(super) const MARKDOWN_NOTE: Provider = Provider {
    id: "markdown_note",
    name: "Markdown note",
    category: "Utilities",
    icon: "markdown",
    color: "#64748b",
    description: "Keep formatted notes and links on your dashboard.",
    docs: "https://github.com/Rami-0/hobnail/blob/main/docs/features/integrations.md#markdown-note",
    public_internet: false,
    contacts: "None. Notes are parsed locally and links are never fetched.",
    fields: &[ENABLED],
    test: local_test,
    widgets: &[Widget {
        id: "note",
        name: "Note",
        description: "Display a bounded Markdown note. HTML is rendered as literal text.",
        recommended: true,
        default_size: size(4, 3),
        min_size: size(2, 2),
        sample: r#"{"kind":"rich_text","blocks":[{"kind":"heading","level":2,"spans":[{"text":"On-call","kind":"text","link":null}]},{"kind":"paragraph","spans":[{"text":"Backups run nightly at 02:00.","kind":"text","link":null}]}]}"#,
        fields: &[NOTE],
        validate,
        fetch,
    }],
};

fn validate(options: &Values) -> Result<(), &'static str> {
    blocks(options).map(|_| ())
}

fn fetch<'a>(_: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        Ok(StructuredBody::RichText {
            blocks: blocks(options).map_err(|_| Class::Protocol)?,
        })
    })
}

fn blocks(options: &Values) -> Result<Vec<RichBlock>, &'static str> {
    let note = options
        .get(NOTE.id)
        .and_then(Value::as_str)
        .ok_or(NOTE.id)?;
    parse(note).ok_or(NOTE.id)
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let (mark, text) = line.split_once(' ')?;
    let level = match mark {
        "#" => 1,
        "##" => 2,
        "###" => 3,
        _ => return None,
    };
    Some((level, text))
}

fn list_item(line: &str) -> Option<(Option<u32>, &str)> {
    let (mark, text) = line.split_once(' ')?;
    if matches!(mark, "-" | "+" | "*") {
        return Some((None, text));
    }
    let number = mark.strip_suffix('.')?;
    if number.len() > 6 || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((Some(number.parse().ok()?), text))
}

fn fence(line: &str) -> bool {
    line.strip_prefix("```").is_some_and(|tail| {
        tail.trim()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_+-".contains(&b))
    })
}

fn parse(note: &str) -> Option<Vec<RichBlock>> {
    if note.len() > MAX_BYTES || note.trim().is_empty() {
        return None;
    }
    let note = note.replace("\r\n", "\n");
    if note
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return None;
    }
    let mut remaining = MAX_SPANS;
    let mut blocks = Vec::new();
    let mut lines = note.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        if blocks.len() == MAX_BLOCKS {
            return None;
        }
        let block = if fence(line) {
            let lines = lines
                .by_ref()
                .take_while(|line| line.trim() != "```")
                .collect::<Vec<_>>();
            RichBlock::Code {
                text: lines.join("\n"),
            }
        } else if let Some((level, text)) = heading(line) {
            let mut spans = Vec::new();
            inline(text, &mut spans, &mut remaining)?;
            RichBlock::Heading { level, spans }
        } else if let Some((start, text)) = list_item(line) {
            let mut items = Vec::new();
            let mut text = text;
            loop {
                let mut spans = Vec::new();
                // Empty list entries also consume the unit budget.
                inline(text, &mut spans, &mut remaining)?;
                if spans.is_empty() {
                    remaining = remaining.checked_sub(1)?;
                }
                items.push(spans);
                let Some((number, next)) = lines.peek().and_then(|line| list_item(line)) else {
                    break;
                };
                if start.is_some() != number.is_some() {
                    break;
                }
                lines.next();
                text = next;
            }
            RichBlock::List { start, items }
        } else {
            let mut spans = Vec::new();
            inline(line, &mut spans, &mut remaining)?;
            while let Some(next) = lines.peek().filter(|line| {
                !line.trim().is_empty()
                    && heading(line).is_none()
                    && list_item(line).is_none()
                    && !fence(line)
            }) {
                push(&mut spans, &mut remaining, RichKind::Break, "", None)?;
                inline(next, &mut spans, &mut remaining)?;
                lines.next();
            }
            RichBlock::Paragraph { spans }
        };
        blocks.push(block);
    }
    // Cache accounting uses capacities. Keep small final arrays compact.
    blocks.shrink_to_fit();
    Some(blocks)
}

fn push(
    spans: &mut Vec<RichSpan>,
    remaining: &mut usize,
    kind: RichKind,
    text: &str,
    link: Option<&str>,
) -> Option<()> {
    *remaining = remaining.checked_sub(1)?;
    spans.push(RichSpan {
        kind,
        text: text.into(),
        link: link.map(str::to_owned),
    });
    Some(())
}

fn allowed_link(link: &str) -> bool {
    if navigation_link(link) {
        return true;
    }
    let Some(address) = link.strip_prefix("mailto:") else {
        return false;
    };
    !address.is_empty()
        && !address
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\\'))
        && !address
            .as_bytes()
            .windows(3)
            .any(|part| part.eq_ignore_ascii_case(b"%0a") || part.eq_ignore_ascii_case(b"%0d"))
}

fn inline(mut line: &str, output: &mut Vec<RichSpan>, remaining: &mut usize) -> Option<()> {
    let mut cursor = 0;
    while cursor < line.len() {
        let tail = &line[cursor..];
        let lookahead = &tail[..tail.floor_char_boundary(tail.len().min(1280))];
        // Angle-delimited text is literal even if it contains Markdown markers.
        if lookahead.starts_with('<') {
            if let Some(end) = lookahead.find('>') {
                cursor += end + 1;
                continue;
            }
        }
        let mut parsed = None;
        if let Some(link) = lookahead.strip_prefix('[') {
            if let Some((label, rest)) = link.split_once("](") {
                if let Some((url, _)) = rest.split_once(')') {
                    let length = label.len() + url.len() + 4;
                    if !label.is_empty()
                        && label.len() <= 256
                        && !label.contains('[')
                        && url.len() <= 960
                        && allowed_link(url)
                    {
                        parsed = Some((RichKind::Link, label, Some(url), length));
                    } else if !label.contains('[') {
                        cursor += length;
                        continue;
                    }
                }
            }
        } else {
            for (marker, kind) in [
                ("`", RichKind::Code),
                ("**", RichKind::Bold),
                ("__", RichKind::Bold),
                ("*", RichKind::Italic),
                ("_", RichKind::Italic),
            ] {
                if let Some(rest) = lookahead.strip_prefix(marker) {
                    if let Some((text, _)) =
                        rest.split_once(marker).filter(|(text, _)| !text.is_empty())
                    {
                        parsed = Some((kind, text, None, text.len() + 2 * marker.len()));
                    }
                    break;
                }
            }
        }
        if let Some((kind, text, link, length)) = parsed {
            if cursor > 0 {
                push(output, remaining, RichKind::Text, &line[..cursor], None)?;
            }
            push(output, remaining, kind, text, link)?;
            line = &line[cursor + length..];
            cursor = 0;
        } else {
            cursor += tail.chars().next()?.len_utf8();
        }
    }
    if !line.is_empty() {
        push(output, remaining, RichKind::Text, line, None)?;
    }
    output.shrink_to_fit();
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_structured_blocks_for_the_documented_subset() {
        let note = "# Title\n## Level two\n### Level three\n\nA **bold** and *italic* `command`.\nNext line\n\n- a\n- b\n5. c\n6. d\n\n```sh\necho **literal**\n```\n[Docs](https://example.test/doc)";
        let body = serde_json::to_value(parse(note).unwrap()).unwrap();
        assert_eq!(body[2]["level"], 3);
        assert_eq!(body[3]["spans"][1]["kind"], "bold");
        assert_eq!(body[3]["spans"][3]["kind"], "italic");
        assert_eq!(body[3]["spans"][5]["kind"], "code");
        assert_eq!(body[3]["spans"][7]["kind"], "break");
        assert_eq!(body[4]["items"].as_array().unwrap().len(), 2);
        assert_eq!(body[5]["start"], 5);
        assert_eq!(body[6]["text"], "echo **literal**");
        assert_eq!(body[7]["spans"][0]["link"], "https://example.test/doc");
    }

    #[test]
    fn keeps_hostile_markup_literal_and_rejects_oversized_notes() {
        let bad = "[j](javascript:alert(1)) [j](java\tscript:evil) [x](data:text/html,evil) [x](https://example.test/\"onmouseover=\"x) <img src=x onerror=evil()> <script>evil()</script>";
        let parsed = parse(bad).unwrap();
        assert!(
            matches!(&parsed[0], RichBlock::Paragraph { spans } if spans.len() == 1 && spans[0].text == bad && spans[0].link.is_none())
        );
        assert!(allowed_link("mailto:team@example.test"));
        assert!(!allowed_link(
            "mailto:team@example.test?subject=x%0aBcc:attacker"
        ));
        assert!(parse(&"x".repeat(MAX_BYTES + 1)).is_none());
        assert!(parse(&"# x\n".repeat(MAX_BLOCKS + 1)).is_none());
        assert!(parse(&"*x* ".repeat(MAX_SPANS + 1)).is_none());
    }
}
