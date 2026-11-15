# The interface

Server-rendered pages (maud), hand-written CSS and a little vanilla JS. No
framework, no build step, no web fonts, no CDN. Dark first, light follows the
OS, the ◐ button overrides it per browser.

- **Sidebar:** pages, then every compose stack with running/total counts and its
  containers (status dots; pulsing while starting or restarting; stopped ones
  dimmed). Filter box. Collapses to an icon rail (remembered); a drawer on
  phones.
- **Top bar:** breadcrumb, search, CPU / MEM / DISK chips with sparklines,
  theme, account (Sign in / Sign out).
- **Command palette:** `Ctrl/⌘ K` or `/`. Containers, pages, bookmarks,
  settings sections, actions ("Logs: X" via Dozzle, toggle theme). Enter on no
  match searches the web (`page.search_url`).
- **Pages:** Overview (health strip, host vitals with 1 h charts, needs
  attention, widgets, top consumers, recent events, bookmarks), Containers
  (dense table: filter, sort, group by stack or flat, remembered; phones get
  stacked rows), container detail (charts, timeline, ports and links, labels,
  matching alert rules), History, Alerts, Settings, Login.
- **Live:** one SSE stream per visible page; numbers update in place. Updates
  never move what you are reading: each one is scroll-anchored by hand (Safari
  on iOS has no CSS scroll anchoring), and updates that arrive while you touch
  or scroll wait until 1.5 s after you stop (`scripts/phone-scroll-test.mjs`). The stream
  closes when the tab is hidden for 10 s (no work for nobody) and reopens on
  return. A new build reloads open pages.
- **Charts:** inline SVG drawn on the server from the history database; hover
  for values; gaps stay gaps.
- **Theming:** every color is a CSS variable; `data/custom.css` loads last
  (editable in Settings → Appearance).

`page.columns` and per-group `columns` from v1 no longer have an effect (there is
no card grid); `layout = "list"` makes the flat table the default view.

## v3 design system (Wave 1)

Strict type scale with tabular numbers and calm zeros; primary / secondary /
ghost panels; **24 h health strips** per container and stack; charts with a
shared crosshair, drag-to-zoom (History), event markers, a "yesterday" line and
legend toggles; memory treemap; ranked "Needs attention" and "since you last
looked" on Overview. Themes: Dark, Light, Ember, High contrast (contrast-tested).
Keyboard: `Ctrl/⌘ K` or `/` palette, `g` then `o c h l a s` for pages, `j/k` rows,
`?` cheat sheet, `l` logs of the current container. The tab title and favicon
show the number of problems. Installable as an app (manifest, icons, offline
page; no data caching); a bottom bar on phones. Decisions: DESIGN.md §10.
