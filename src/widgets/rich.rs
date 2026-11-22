use std::mem::size_of;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(tag = "kind", rename_all = "snake_case"))]
pub enum RichBlock {
    Heading {
        level: u8,
        spans: Vec<RichSpan>,
    },
    Paragraph {
        spans: Vec<RichSpan>,
    },
    List {
        start: Option<u32>,
        items: Vec<Vec<RichSpan>>,
    },
    Code {
        text: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename_all = "snake_case"))]
pub enum RichKind {
    Text,
    Bold,
    Italic,
    Code,
    Link,
    Break,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RichSpan {
    pub kind: RichKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub link: Option<String>,
}

impl RichBlock {
    pub(super) fn retained_bytes(&self) -> usize {
        fn span_bytes(spans: &Vec<RichSpan>) -> usize {
            spans.capacity() * size_of::<RichSpan>()
                + spans
                    .iter()
                    .map(|span| {
                        span.text.capacity() + span.link.as_ref().map_or(0, String::capacity)
                    })
                    .sum::<usize>()
        }
        match self {
            Self::Heading { spans, .. } | Self::Paragraph { spans } => span_bytes(spans),
            Self::List { items, .. } => {
                items.capacity() * size_of::<Vec<RichSpan>>()
                    + items.iter().map(span_bytes).sum::<usize>()
            }
            Self::Code { text } => text.capacity(),
        }
    }
}
