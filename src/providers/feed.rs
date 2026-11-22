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
    published: String,
    updated: String,
    summary: String,
}

impl Entry {
    fn body(self, summaries: bool) -> BodyItem {
        let compact = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        let title = compact(&self.title);
        let published = compact(&self.published);
        let date = if published.is_empty() {
            compact(&self.updated)
        } else {
            published
        };
        let summary = if summaries {
            compact(&self.summary)
        } else {
            String::new()
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
            link: (link.len() <= LINK && navigation_link(link)).then(|| link.into()),
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
            Self::Summary => (&mut item.summary, 240),
        }
    }
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

fn parse(body: &[u8], summaries: bool) -> Option<Vec<BodyItem>> {
    if body.len() > MAX_RESPONSE {
        return None;
    }
    let input = std::str::from_utf8(body).ok()?;
    let mut scanner = Scanner(input.strip_prefix('\u{feff}').unwrap_or(input));
    let mut stack: Vec<Tag<'_>> = Vec::new();
    let mut items = Vec::new();
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
                    xml::append(text, entities, out, max)?;
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
                            items.push(item.body(summaries));
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
                        items.push(item.body(summaries));
                    }
                }
            }
        }
    }
    (root.is_some() && stack.is_empty()).then_some(items)
}

async fn feed(conn: &Conn<'_>, summaries: bool) -> Result<Vec<BodyItem>, Fault> {
    let response = success(conn.request(Step::get("")).await?)?;
    parse(&response.body, summaries).ok_or_else(|| Class::Protocol.into())
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
                    Some("Sun, 22 Nov 2026 12:00:00 GMT · One <b>story</b>")
                );
            },
        )
        .await;
    }

    #[test]
    fn atom_resolves_prefixes_and_keeps_only_alternate_http_links() {
        let feed = br#"<a:feed xmlns:a="http://www.w3.org/2005/Atom"><a:entry xmlns:b="http://www.w3.org/2005/Atom"><b:title>Atom</b:title><a:link href="javascript:alert(1)"/><b:link rel="self" href="https://example.test/feed"/><b:link href="https://example.test/story"/><a:updated>2026-11-22T00:00:00Z</a:updated></a:entry></a:feed>"#;
        let entry = parse(feed, true).unwrap().pop().unwrap();
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
            format!("<rss version='2.0' attr='{}'/>", "a".repeat(1025)),
            format!(
                "<rss version='2.0'>{}{}</rss>",
                "<x>".repeat(24),
                "</x>".repeat(24)
            ),
        ];
        for xml in hostile {
            assert!(parse(xml.as_bytes(), true).is_none());
        }
        for offset in 0..RSS.len() {
            assert!(parse(&RSS.as_bytes()[..offset], true).is_none());
        }
        let many = format!(
            "<rss version='2.0'><channel>{}</channel></rss>",
            "<item><title>hello</title></item>".repeat(60)
        );
        assert_eq!(parse(many.as_bytes(), false).unwrap().len(), MAX_ITEMS);
    }
}
