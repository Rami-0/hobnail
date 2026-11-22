use super::*;
use crate::http::icons::valid_slug;
use widgets::{BodyItem, StructuredBody};

const LINKS: Field = Field {
    required: true,
    max: Some(16),
    max_length: Some(1152),
    ..Field::new(
        "links",
        "Links",
        Kind::List,
        "One per line: Name | https://example.com | optional icon name. Up to 16 links per group.",
    )
};

pub(super) const BOOKMARKS: Provider = Provider {
    id: "bookmarks",
    name: "Bookmarks",
    category: "Utilities",
    icon: "bookmark",
    description: "Create named groups of links to the sites you use.",
    docs: "https://github.com/Rami-0/hobnail/blob/main/docs/features/integrations.md#bookmarks",
    public_internet: false,
    contacts: "None. Hobnail does not contact the saved URLs.",
    fields: &[ENABLED],
    test: local_test,
    widgets: &[Widget {
        id: "links",
        name: "Bookmark group",
        description: "Shows your links in the order entered.",
        recommended: true,
        fields: &[LINKS],
        validate,
        fetch,
    }],
};

fn links(options: &Values) -> Result<Vec<BodyItem>, &'static str> {
    let key = LINKS.id;
    let value = options
        .get(key)
        .filter(|value| LINKS.accepts(value))
        .ok_or(key)?;
    value
        .as_array()
        .ok_or(key)?
        .iter()
        .map(|line| {
            let mut parts = line.as_str().ok_or(key)?.split('|').map(str::trim);
            let title = parts.next().ok_or(key)?;
            let url = parts.next().ok_or(key)?;
            let icon = parts.next().filter(|icon| !icon.is_empty());
            if parts.next().is_some()
                || title.is_empty()
                || title.len() > 80
                || title.chars().any(char::is_control)
                || url.len() > 960
                || !navigation_link(url)
                || icon.is_some_and(|icon| !valid_slug(icon))
            {
                return Err(key);
            }
            Ok(BodyItem {
                title: title.into(),
                secondary: None,
                link: Some(url.into()),
                icon: icon.map(str::to_owned),
                time: None,
            })
        })
        .collect()
}

fn validate(options: &Values) -> Result<(), &'static str> {
    links(options).map(|_| ())
}

fn fetch<'a>(_: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        Ok(StructuredBody::ItemList {
            items: links(options).map_err(|_| Class::Protocol)?,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_navigation_icon_and_group_size_without_probing() {
        let values = |value| Values::from_iter([("links".into(), value)]);
        let group = links(&values(json!([
            "A | https://example.test/?x=1#two | github",
            "B | http://router.local/"
        ])))
        .unwrap();
        assert_eq!(group[0].icon.as_deref(), Some("github"));
        assert_eq!(
            group[0].link.as_deref(),
            Some("https://example.test/?x=1#two")
        );
        assert!(group[1].secondary.is_none());
        for invalid in [
            json!(["A | javascript:alert(1)"]),
            json!(["A | https://a@example.test"]),
            json!(["A | https://example.test | ../github"]),
            json!(vec!["A | https://example.test"; 17]),
        ] {
            assert!(validate(&values(invalid)).is_err());
        }
    }
}
