//! The inline SVG icon sprite: ~40 hand-drawn line icons on a 24 px grid,
//! stroke `currentColor`. Use with `ui::ic("name")`, which emits
//! `<svg class="i"><use href="#i-name"/></svg>`.

use maud::{html, Markup, PreEscaped};

const ICONS: &[(&str, &str)] = &[
    (
        "contrast",
        r#"<circle cx="12" cy="12" r="9"/><path d="M12 3a9 9 0 0 1 0 18z" fill="currentColor"/>"#,
    ),
    (
        "flame",
        r#"<path d="M12 21a6 6 0 0 0 6-6c0-4-3-6-4-10-2 2-3 4-3 6-1-1-2-2-2-4-2 2-3 5-3 8a6 6 0 0 0 6 6z"/>"#,
    ),
    (
        "more",
        r#"<circle cx="5" cy="12" r="1.2"/><circle cx="12" cy="12" r="1.2"/><circle cx="19" cy="12" r="1.2"/>"#,
    ),
    (
        "keyboard",
        r#"<rect x="2.5" y="6" width="19" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/>"#,
    ),
    (
        "grid",
        r#"<rect x="3.5" y="3.5" width="7" height="7" rx="1.5"/><rect x="13.5" y="3.5" width="7" height="7" rx="1.5"/><rect x="3.5" y="13.5" width="7" height="7" rx="1.5"/><rect x="13.5" y="13.5" width="7" height="7" rx="1.5"/>"#,
    ),
    (
        "box",
        r#"<path d="m12 3 8 4.5v9L12 21l-8-4.5v-9z"/><path d="m4 7.5 8 4.5 8-4.5M12 12v9"/>"#,
    ),
    (
        "layers",
        r#"<path d="m12 3 9 5-9 5-9-5z"/><path d="m3 12.5 9 5 9-5"/><path d="m3 16.5 9 5 9-5"/>"#,
    ),
    (
        "history",
        r#"<path d="M3 3v18h18"/><path d="m7 15 4-5 3 3 5-7"/>"#,
    ),
    (
        "clock",
        r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/>"#,
    ),
    (
        "bell",
        r#"<path d="M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15z"/><path d="M10 21h4"/>"#,
    ),
    (
        "sliders",
        r#"<path d="M4 7h10M18 7h2M4 17h2M10 17h10"/><circle cx="16" cy="7" r="2"/><circle cx="8" cy="17" r="2"/>"#,
    ),
    (
        "search",
        r#"<circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/>"#,
    ),
    (
        "sun",
        r#"<circle cx="12" cy="12" r="4"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6 7 7M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4"/>"#,
    ),
    (
        "moon",
        r#"<path d="M20 14.5A8.5 8.5 0 1 1 9.5 4a7 7 0 0 0 10.5 10.5z"/>"#,
    ),
    (
        "monitor",
        r#"<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>"#,
    ),
    ("menu", r#"<path d="M4 6.5h16M4 12h16M4 17.5h16"/>"#),
    (
        "user",
        r#"<circle cx="12" cy="8.5" r="3.5"/><path d="M5 20c.7-3.7 3.5-5.5 7-5.5s6.3 1.8 7 5.5"/>"#,
    ),
    (
        "log-in",
        r#"<path d="M14 4h5a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1h-5"/><path d="m10 8 4 4-4 4M14 12H4"/>"#,
    ),
    (
        "log-out",
        r#"<path d="M10 4H5a1 1 0 0 0-1 1v14a1 1 0 0 0 1 1h5"/><path d="m16 8 4 4-4 4M20 12H10"/>"#,
    ),
    ("chev-r", r#"<path d="m9 6 6 6-6 6"/>"#),
    ("chev-d", r#"<path d="m6 9 6 6 6-6"/>"#),
    ("x", r#"<path d="M6 6l12 12M18 6 6 18"/>"#),
    (
        "external",
        r#"<path d="M14 4h6v6M20 4l-9 9"/><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/>"#,
    ),
    (
        "logs",
        r#"<path d="M14 3H7a1 1 0 0 0-1 1v16a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1V7z"/><path d="M14 3v4h4M9 12h6M9 16h6"/>"#,
    ),
    (
        "info",
        r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>"#,
    ),
    (
        "lock",
        r#"<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>"#,
    ),
    ("check", r#"<path d="m5 12.5 4.5 4.5L19 7.5"/>"#),
    (
        "alert",
        r#"<path d="M12 4 2.5 20h19z"/><path d="M12 10v4.5M12 17.5h.01"/>"#,
    ),
    (
        "refresh",
        r#"<path d="M20 11a8 8 0 0 0-14-4.5L4 9"/><path d="M4 4v5h5M4 13a8 8 0 0 0 14 4.5L20 15M20 20v-5h-5"/>"#,
    ),
    ("filter", r#"<path d="M4 5h16l-6 8v5l-4 2v-7z"/>"#),
    (
        "cpu",
        r#"<rect x="6" y="6" width="12" height="12" rx="2"/><rect x="9.5" y="9.5" width="5" height="5"/><path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3"/>"#,
    ),
    (
        "memory",
        r#"<rect x="3" y="7" width="18" height="9" rx="1.5"/><path d="M7 11v1.5M11 11v1.5M15 11v1.5M7 16v2.5M11 16v2.5M15 16v2.5M19 16v2.5"/>"#,
    ),
    (
        "disk",
        r#"<path d="M4 14h16v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1z"/><path d="M4 14 6.5 5.5A1 1 0 0 1 7.5 5h9a1 1 0 0 1 1 .5L20 14"/><path d="M8 17h.01M12 17h4"/>"#,
    ),
    ("activity", r#"<path d="M3 12h4l3-8 4 16 3-8h4"/>"#),
    (
        "thermo",
        r#"<path d="M14 14.5V5a2 2 0 0 0-4 0v9.5a4 4 0 1 0 4 0z"/>"#,
    ),
    (
        "panel",
        r#"<rect x="3.5" y="4" width="17" height="16" rx="2"/><path d="M9.5 4v16"/>"#,
    ),
    ("arrow-up", r#"<path d="M12 19V5M6 11l6-6 6 6"/>"#),
    ("arrow-down", r#"<path d="M12 5v14M6 13l6 6 6-6"/>"#),
    ("arrow-right", r#"<path d="M5 12h14M13 6l6 6-6 6"/>"#),
    ("plus", r#"<path d="M12 5v14M5 12h14"/>"#),
    (
        "link",
        r#"<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>"#,
    ),
    ("bookmark", r#"<path d="M6 3h12v18l-6-4-6 4z"/>"#),
    (
        "globe",
        r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9S9.5 5.7 12 3z"/>"#,
    ),
    (
        "server",
        r#"<rect x="3" y="4" width="18" height="7" rx="1.5"/><rect x="3" y="13" width="18" height="7" rx="1.5"/><path d="M7 7.5h.01M7 16.5h.01"/>"#,
    ),
    (
        "terminal",
        r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>"#,
    ),
    (
        "edit",
        r#"<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="m13.5 6.5 4 4"/>"#,
    ),
    (
        "copy",
        r#"<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/>"#,
    ),
    ("zap", r#"<path d="M13 3 5 13.5h6L10 21l8-10.5h-6z"/>"#),
];

/// The hidden sprite sheet, once per page.
pub fn sheet() -> Markup {
    html! {
        svg xmlns="http://www.w3.org/2000/svg" width="0" height="0" style="position:absolute" aria-hidden="true" focusable="false" {
            defs {
                @for (name, body) in ICONS {
                    symbol id={"i-" (name)} viewBox="0 0 24 24" fill="none" stroke="currentColor"
                        stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" {
                        (PreEscaped(*body))
                    }
                }
            }
        }
    }
}

/// `<svg class="i"><use href="#i-name"/></svg>`
pub fn ic(name: &str) -> Markup {
    html! { svg.i aria-hidden="true" { use href={"#i-" (name)} {} } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprite_is_well_formed_and_small() {
        let s = sheet().into_string();
        assert_eq!(s.matches("<symbol ").count(), ICONS.len());
        assert_eq!(
            s.matches("<symbol ").count(),
            s.matches("</symbol>").count()
        );
        assert!(ICONS.len() >= 30);
        assert!(s.len() < 12_000, "sprite is {} bytes", s.len());
        // No duplicate ids.
        let mut names: Vec<&str> = ICONS.iter().map(|i| i.0).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), ICONS.len());
        assert_eq!(
            ic("box").into_string(),
            r##"<svg class="i" aria-hidden="true"><use href="#i-box"></use></svg>"##
        );
    }
}
