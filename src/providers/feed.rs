use super::*;
use widgets::{BodyItem, StructuredBody};

mod xml;
use xml::{Event, Scanner, Tag};

const ATOM: &str = "http://www.w3.org/2005/Atom";
const MAX_ITEMS: usize = 20;
const MAX_DEPTH: usize = 24;
const LINK: usize = 768;

pub(super) const FEED: Provider = Provider {
    id: "feed",
    name: "RSS / Atom",
    category: "Utilities",
    icon: "rss",
    description: "Read titles, links, dates and optional summaries from a feed.",
    docs: "https://www.rssboard.org/rss-specification",
    public_internet: false,
    contacts: "The configured feed URL. Public feeds require internet access.",
    fields: &[
        Field {
            label: "Feed URL",
            ..SERVICE_URL
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "items",
        name: "Feed items",
        description: "Up to 20 entries in feed order.",
        recommended: true,
        fields: &[
            Field {
                default: Some("true"),
                ..Field::new("summaries", "Show summaries", Kind::Bool, "")
            },
            Field {
                default: Some("900"),
                ..REFRESH
            },
        ],
        fetch,
        validate: no_widget_options,
    }],
};

#[derive(Default)]
struct Entry {
    title: String,
    link: String,
    link_overflow: bool,
    published: String,
    updated: String,
    summary: String,
    summary_html: bool,
}

/// Raw summary bytes kept per entry. Markup is stripped afterwards, so this
/// leaves room for tags around the visible text.
const SUMMARY_RAW: usize = 2048;
/// Visible summary bytes after markup is stripped.
const SUMMARY: usize = 240;

impl Entry {
    /// Folding, tag stripping and entity decoding can hide a secret from the
    /// final output pass, so each field is checked before and after them.
    fn body(self, summaries: bool, redactor: &OutputRedactor) -> BodyItem {
        let compact = |s: &str| {
            redactor.transformed(&[s], s.split_whitespace().collect::<Vec<_>>().join(" "))
        };
        let title = compact(&self.title);
        let date = if self.published.trim().is_empty() {
            compact(&self.updated)
        } else {
            compact(&self.published)
        };
        let summary = if !summaries {
            String::new()
        } else if self.summary_html {
            let text = html_text(&self.summary);
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            clip(redactor.transformed(&[&self.summary], text), SUMMARY)
        } else {
            clip(compact(&self.summary), SUMMARY)
        };
        let secondary = [date, summary]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let link = self.link.trim();
        BodyItem {
            icon: None,
            title: if title.is_empty() {
                "Untitled entry".into()
            } else {
                title
            },
            link: (!self.link_overflow && link.len() <= LINK && navigation_link(link))
                .then(|| link.into()),
            secondary: (!secondary.is_empty()).then_some(secondary),
            time: None,
        }
    }
}

#[derive(Clone, Copy)]
enum FieldName {
    Title,
    Link,
    Published,
    Updated,
    Summary,
}

impl FieldName {
    fn of(name: &str, atom: bool) -> Option<Self> {
        Some(match (name, atom) {
            ("title", _) => Self::Title,
            ("link", false) => Self::Link,
            ("pubDate", false) | ("published", true) => Self::Published,
            ("updated", true) => Self::Updated,
            ("description", false) | ("summary", true) => Self::Summary,
            _ => return None,
        })
    }

    fn target(self, item: &mut Entry) -> (&mut String, usize) {
        match self {
            Self::Title => (&mut item.title, 160),
            Self::Link => (&mut item.link, LINK + 8),
            Self::Published => (&mut item.published, 80),
            Self::Updated => (&mut item.updated, 80),
            Self::Summary => (&mut item.summary, SUMMARY_RAW),
        }
    }
}

fn clip(mut text: String, max: usize) -> String {
    if text.len() > max {
        text.truncate(text.floor_char_boundary(max));
    }
    text
}

/// Plain text of an HTML fragment (RSS descriptions, Atom `type="html"`).
/// Tags, comments and script or style content are dropped and common
/// entities decoded. The result is only ever displayed as text, so this is
/// for legibility, not sanitizing.
fn html_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if !after.starts_with(|c: char| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?')) {
            out.push('<');
            rest = after;
            continue;
        }
        let end = if let Some(comment) = after.strip_prefix("!--") {
            comment.find("-->").map(|end| 3 + end + 3)
        } else {
            after.find('>').map(|end| end + 1)
        };
        let Some(end) = end else {
            rest = ""; // An unterminated tag hides the rest.
            break;
        };
        let name = after[..end]
            .trim_start_matches('/')
            .split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap_or_default();
        rest = &after[end..];
        let opening = !after.starts_with('/');
        for raw in ["script", "style"] {
            if opening && name.eq_ignore_ascii_case(raw) {
                let close = rest.to_ascii_lowercase().find(&format!("</{raw}"));
                rest = close.map_or("", |close| &rest[close..]);
            }
        }
        // Block-level markup separates words; inline markup does not.
        const INLINE: &[&str] = &[
            "a", "abbr", "b", "code", "em", "i", "mark", "s", "small", "span", "strong", "sub",
            "sup", "u",
        ];
        if !INLINE.iter().any(|tag| name.eq_ignore_ascii_case(tag)) {
            out.push(' ');
        }
    }
    out.push_str(rest);
    decode_html_entities(&out)
}

fn decode_html_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let decoded = after[..after.floor_char_boundary(after.len().min(12))]
            .split_once(';')
            .and_then(|(name, _)| {
                let c = match name {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    "nbsp" => ' ',
                    "hellip" => '…',
                    "mdash" => '—',
                    "ndash" => '–',
                    "lsquo" => '‘',
                    "rsquo" => '’',
                    "ldquo" => '“',
                    "rdquo" => '”',
                    "copy" => '©',
                    _ => {
                        let (digits, radix) = name
                            .strip_prefix("#x")
                            .or_else(|| name.strip_prefix("#X"))
                            .map(|digits| (digits, 16))
                            .or_else(|| name.strip_prefix('#').map(|digits| (digits, 10)))?;
                        char::from_u32(u32::from_str_radix(digits, radix).ok()?)
                            .filter(|c| !c.is_control() || c.is_whitespace())?
                    }
                };
                Some((c, name.len() + 1))
            });
        match decoded {
            Some((c, used)) => {
                out.push(c);
                rest = &after[used..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
fn none() -> OutputRedactor {
    OutputRedactor::new(std::iter::empty::<&str>())
}

fn atom_name<'a>(tag: &Tag<'a>, parents: &[Tag<'_>]) -> Option<&'a str> {
    let (prefix, local) = tag.name.split_once(':').unwrap_or(("", tag.name));
    let mut tags = std::iter::once(tag).chain(parents.iter().rev());
    let matches = if prefix.is_empty() {
        tags.find_map(|t| t.attr("xmlns"))? == ATOM
    } else {
        let key = format!("xmlns:{prefix}");
        tags.find_map(|t| t.attr(&key))? == ATOM
    };
    matches.then_some(local)
}

/// Parse a feed. When `complete` is false the body is only the leading
/// [`MAX_RESPONSE`] bytes of a longer feed: the entries that closed before
/// the cut are returned and the unfinished remainder is ignored.
fn parse(
    body: &[u8],
    summaries: bool,
    complete: bool,
    redactor: &OutputRedactor,
) -> Option<Vec<BodyItem>> {
    if body.len() > MAX_RESPONSE {
        return None;
    }
    let input = match std::str::from_utf8(body) {
        Ok(input) => input,
        // The cut can split a character.
        Err(error) if !complete => std::str::from_utf8(&body[..error.valid_up_to()]).ok()?,
        Err(_) => return None,
    };
    let mut items = Vec::new();
    let whole = entries(input, summaries, redactor, &mut items).is_some();
    (if complete { whole } else { !items.is_empty() }).then_some(items)
}

fn entries(
    input: &str,
    summaries: bool,
    redactor: &OutputRedactor,
    items: &mut Vec<BodyItem>,
) -> Option<()> {
    let mut scanner = Scanner(input.strip_prefix('\u{feff}').unwrap_or(input));
    let mut stack: Vec<Tag<'_>> = Vec::new();
    let mut root = None;
    let mut entry = None;
    let mut field = None;
    while !scanner.0.is_empty() {
        let depth = stack.len();
        match scanner.next()? {
            Event::Skip => {}
            Event::Text(text, entities) => {
                if depth == 0 && (!entities || !text.trim().is_empty()) {
                    return None;
                }
                if let (Some(entry), Some(field)) = (&mut entry, field) {
                    let (out, max) = FieldName::target(field, entry);
                    if xml::append(text, entities, out, max)? && matches!(field, FieldName::Link) {
                        entry.link_overflow = true;
                    }
                } else {
                    xml::append(text, entities, &mut String::new(), 0)?;
                }
            }
            Event::Open(tag) => {
                if depth == MAX_DEPTH {
                    return None;
                }
                if depth == 0 {
                    if root.is_some() {
                        return None;
                    }
                    root = Some(match tag.name {
                        "rss" if tag.attr("version") == Some("2.0") => false,
                        _ if atom_name(&tag, &stack) == Some("feed") => true,
                        _ => return None,
                    });
                }
                let atom = root?;
                let item_depth = if atom { 2 } else { 3 };
                let local = if atom {
                    atom_name(&tag, &stack).unwrap_or("")
                } else {
                    tag.name
                };
                if depth + 1 == item_depth
                    && items.len() < MAX_ITEMS
                    && ((atom && local == "entry")
                        || (!atom && stack[1].name == "channel" && local == "item"))
                {
                    entry = Some(Entry::default());
                }
                if depth == item_depth {
                    if let Some(item) = entry.as_mut() {
                        field = FieldName::of(local, atom);
                        if matches!(field, Some(FieldName::Summary)) {
                            item.summary_html = !atom || tag.attr("type") == Some("html");
                        }
                        if atom
                            && local == "link"
                            && item.link.is_empty()
                            && matches!(tag.attr("rel"), None | Some("alternate"))
                        {
                            if let Some(href) = tag
                                .attr("href")
                                .filter(|href| href.len() <= LINK && navigation_link(href))
                            {
                                item.link = href.into();
                            }
                        }
                    }
                }
                if tag.empty {
                    if depth + 1 == item_depth {
                        if let Some(item) = entry.take() {
                            items.push(item.body(summaries, redactor));
                        }
                    }
                    if depth == item_depth {
                        field = None;
                    }
                } else {
                    stack.push(tag);
                }
            }
            Event::Close(name) => {
                if stack.pop()?.name != name {
                    return None;
                }
                let item_depth = if root? { 2 } else { 3 };
                if depth == item_depth + 1 {
                    field = None;
                }
                if depth == item_depth {
                    if let Some(item) = entry.take() {
                        items.push(item.body(summaries, redactor));
                    }
                }
            }
        }
    }
    (root.is_some() && stack.is_empty()).then_some(())
}

async fn feed(conn: &Conn<'_>, summaries: bool) -> Result<Vec<BodyItem>, Fault> {
    // Keep at most the leading MAX_RESPONSE bytes and stop reading there.
    let mut body = Vec::new();
    let mut complete = true;
    let mut sink = |chunk: &[u8]| {
        let room = MAX_RESPONSE - body.len();
        if chunk.len() > room {
            body.extend_from_slice(&chunk[..room]);
            complete = false;
            return Ok(false);
        }
        body.extend_from_slice(chunk);
        Ok(true)
    };
    success(conn.scan(Step::get(""), &mut sink).await?)?;
    parse(&body, summaries, complete, &conn.output_redactor()).ok_or_else(|| Class::Protocol.into())
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { feed(conn, false).await.map(|_| ()) })
}

fn fetch<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let summaries = options
            .get("summaries")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let items = feed(conn, summaries).await?;
        Ok(if items.is_empty() {
            StructuredBody::Stat {
                value: "No feed entries".into(),
            }
        } else {
            StructuredBody::ItemList { items }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::super::batch_test::{exercise, Exchange};
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><item><title>RSS &amp; &#x2605;</title><link>https://example.test/item?a=1&amp;b=2</link><description><![CDATA[One <b>story</b>]]></description><pubDate>Sun, 22 Nov 2026 12:00:00 GMT</pubDate></item></channel></rss>"#;

    #[tokio::test]
    async fn fetches_a_local_rss_feed_with_safe_links() {
        exercise(
            &FEED,
            &[],
            Values::new(),
            &[Exchange {
                request: "GET /prefix HTTP/1.1",
                status: 200,
                body: RSS,
            }],
            |_, _| "",
            async |conn| {
                let items = feed(conn, true).await.unwrap();
                assert_eq!(items[0].title, "RSS & ★");
                assert_eq!(
                    items[0].link.as_deref(),
                    Some("https://example.test/item?a=1&b=2")
                );
                assert_eq!(
                    items[0].secondary.as_deref(),
                    Some("Sun, 22 Nov 2026 12:00:00 GMT · One story")
                );
            },
        )
        .await;
    }

    #[tokio::test]
    async fn reads_the_entries_that_close_within_the_capped_prefix() {
        let item = |i: usize| {
            format!(
                "<item><title>Story {i}</title><link>https://example.test/{i}</link><description>{}</description></item>",
                "&lt;p&gt;long&lt;/p&gt; ".repeat(500)
            )
        };
        let long = format!(
            "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>Big</title>{}</channel></rss>",
            (0..30).map(item).collect::<String>()
        );
        assert!(long.len() > 2 * MAX_RESPONSE);
        let huge_first = format!(
            "<rss version=\"2.0\"><channel><item><title>Only</title><description>{}</description></item></channel></rss>",
            "x".repeat(MAX_RESPONSE)
        );
        let exchange = |body| Exchange {
            request: "GET /prefix HTTP/1.1",
            status: 200,
            body,
        };
        exercise(
            &FEED,
            &[],
            Values::new(),
            &[exchange(&long), exchange(&huge_first)],
            |_, _| "",
            async |conn| {
                let items = feed(conn, true).await.unwrap();
                // About 12 KB per entry: the entries wholly inside 128 KiB.
                assert_eq!(items.len(), MAX_RESPONSE / item(0).len());
                assert_eq!(items[0].title, "Story 0");
                assert_eq!(items[0].link.as_deref(), Some("https://example.test/0"));
                assert!(items[0]
                    .secondary
                    .as_deref()
                    .unwrap()
                    .starts_with("long long"));
                assert!(items[0].secondary.as_deref().unwrap().len() <= SUMMARY);
                // No entry closes within the prefix.
                assert!(matches!(
                    feed(conn, true).await.unwrap_err().class,
                    Class::Protocol
                ));
            },
        )
        .await;
        // A complete body must still be a whole document.
        assert!(parse(&long.as_bytes()[..MAX_RESPONSE], true, true, &none()).is_none());
        assert!(parse("<rss version='2.0'><chan".as_bytes(), true, false, &none()).is_none());
    }

    #[test]
    fn summaries_show_html_as_text_and_long_attributes_are_skipped() {
        let rss = format!(
            "<rss version='2.0' data-blob='{}'><channel><item><title>T</title><description>&lt;p&gt;Hello &amp;amp;&amp;nbsp;&lt;a href=\"https://example.test\"&gt;world&lt;/a&gt;&amp;#8230;&lt;/p&gt;&lt;!-- c --&gt;&lt;script&gt;alert(1)&lt;/script&gt;&lt;style&gt;p{{}}&lt;/style&gt; 1 &lt; 2 &amp;unknown;</description></item></channel></rss>",
            "a".repeat(4096)
        );
        let entry = parse(rss.as_bytes(), true, true, &none())
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(
            entry.secondary.as_deref(),
            Some("Hello & world… 1 < 2 &unknown;")
        );
        let atom = br#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><title>A</title><summary type="text">a &lt;b&gt; c</summary></entry><entry><title>B</title><summary type="html">&lt;i&gt;x&lt;/i&gt;</summary><link href="https://example.test/b" title=""/></entry></feed>"#;
        let entries = parse(atom, true, true, &none()).unwrap();
        assert_eq!(entries[0].secondary.as_deref(), Some("a <b> c"));
        assert_eq!(entries[1].secondary.as_deref(), Some("x"));
        // A skipped attribute cannot stand in for a link.
        let skipped = format!(
            r#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><title>C</title><link href="https://example.test/{}"/></entry></feed>"#,
            "p".repeat(1100)
        );
        assert!(parse(skipped.as_bytes(), true, true, &none()).unwrap()[0]
            .link
            .is_none());
    }

    #[test]
    fn secrets_reshaped_by_stripping_decoding_or_folding_are_withheld() {
        // Each secret is hidden by a transform: tag stripping joins "pa<i>ss"
        // into "pass", entity decoding turns "a&amp;b" into "a&b", and
        // whitespace folding turns "two  spaces" into "two spaces".
        let redactor = OutputRedactor::new(["pa<i>ss", "a&amp;b", "two  spaces"].into_iter());
        let rss = "<rss version='2.0'><channel>\
            <item><title>One</title><description>x pa&lt;i&gt;ss y</description></item>\
            <item><title>Two</title><description>&lt;p&gt;a&amp;amp;b&lt;/p&gt;</description></item>\
            <item><title>two  spaces</title></item></channel></rss>";
        let items = parse(rss.as_bytes(), true, true, &redactor).unwrap();
        assert_eq!(items[0].secondary.as_deref(), Some("<redacted>"));
        assert_eq!(items[1].secondary.as_deref(), Some("<redacted>"));
        assert_eq!(items[2].title, "<redacted>");
        // Without the secrets the same text is shown.
        let items = parse(rss.as_bytes(), true, true, &none()).unwrap();
        assert_eq!(items[0].secondary.as_deref(), Some("x pass y"));
        assert_eq!(items[1].secondary.as_deref(), Some("a&b"));
        // Redaction runs before the summary is clipped, so a cut cannot
        // leave the start of a secret.
        let secret = "SECRET-0123456789";
        let redactor = OutputRedactor::new([secret].into_iter());
        let atom = format!(
            r#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><title>T</title><summary>{} {secret}</summary></entry></feed>"#,
            "x".repeat(SUMMARY - 6)
        );
        let entry = parse(atom.as_bytes(), true, true, &redactor)
            .unwrap()
            .pop()
            .unwrap();
        assert!(!entry.secondary.unwrap().contains("SECR"));
    }

    #[test]
    fn atom_resolves_prefixes_and_keeps_only_alternate_http_links() {
        let feed = br#"<a:feed xmlns:a="http://www.w3.org/2005/Atom"><a:entry xmlns:b="http://www.w3.org/2005/Atom"><b:title>Atom</b:title><a:link href="javascript:alert(1)"/><b:link rel="self" href="https://example.test/feed"/><b:link href="https://example.test/story"/><a:updated>2026-11-22T00:00:00Z</a:updated></a:entry></a:feed>"#;
        let entry = parse(feed, true, true, &none()).unwrap().pop().unwrap();
        assert_eq!(entry.title, "Atom");
        assert_eq!(entry.link.as_deref(), Some("https://example.test/story"));
        assert_eq!(entry.secondary.as_deref(), Some("2026-11-22T00:00:00Z"));
    }

    #[test]
    fn bounds_items_and_rejects_hostile_or_malformed_xml() {
        let hostile = [
            "<!DOCTYPE rss [<!ENTITY a 'lol'><!ENTITY b '&a;&a;&a;'>]><rss version='2.0'>&b;</rss>"
                .into(),
            "<!DOCTYPE rss SYSTEM 'file:///etc/passwd'><rss version='2.0'/>".into(),
            "<rss version='2.0'><channel attr='unterminated".into(),
            "<rss version='2.0'><channel></rss>".into(),
            "<rss version='2.0'>&unknown;</rss>".into(),
            "<rss version='2.0'>&#0;</rss>".into(),
            format!(
                "<rss version='2.0'>{}{}</rss>",
                "<x>".repeat(24),
                "</x>".repeat(24)
            ),
        ];
        for xml in hostile {
            assert!(parse(xml.as_bytes(), true, true, &none()).is_none());
        }
        for offset in 0..RSS.len() {
            assert!(parse(&RSS.as_bytes()[..offset], true, true, &none()).is_none());
        }
        let many = format!(
            "<rss version='2.0'><channel>{}</channel></rss>",
            "<item><title>hello</title></item>".repeat(60)
        );
        assert_eq!(
            parse(many.as_bytes(), false, true, &none()).unwrap().len(),
            MAX_ITEMS
        );
    }
}

#[cfg(test)]
#[test]
fn omits_rss_link_when_truncated_tail_hides_after_whitespace() {
    let feed = format!(
        "<rss version=\"2.0\"><channel><item><title>Story</title><link>https://example.test/story{}<![CDATA[hidden]]></link></item></channel></rss>",
        " ".repeat(LINK + 16),
    );
    let entries = parse(feed.as_bytes(), false, true, &none()).unwrap();
    assert_eq!(entries[0].title, "Story");
    assert!(entries[0].link.is_none());
}
