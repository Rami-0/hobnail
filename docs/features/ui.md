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
- **Container detail** tabs: Overview, Logs, Metrics (network, block I/O, PIDs,
  throttling, OOM), Inspect (env values, commands and health-check output only
  when signed in), Processes (signed in), Events.
- **Keyboard:** `g` then `o c h l a s` jumps to a page, `j/k` moves in lists,
  `l` opens a container's logs, `?` shows all shortcuts.
- **Themes:** Dark, Light, Ember, High contrast (◐ button or Settings →
  Appearance), all on the brand palette (`docs/BRANDING.md`). Auto follows
  the OS between Dark and Light. Ember is a deeper, redder dark with the
  lighter brand orange as its accent. Every theme is checked for WCAG
  contrast by a test, including a 3:1 control border (`--ctl-border`) on
  every surface.
- **Install:** "Add to Home Screen" from the browser menu; the tab title shows
  the number of problems.
- **Live:** one SSE stream per visible page; numbers update in place. Updates
  never move what you are reading: each one is scroll-anchored by hand (Safari
  on iOS has no CSS scroll anchoring), and updates that arrive while you touch
  or scroll wait until 1.5 s after you stop (`scripts/phone-scroll-test.mjs`). The stream
  closes when the tab is hidden for 10 s (no work for nobody) and reopens on
  return. A new build reloads open pages.
- **Charts:** inline SVG drawn on the server from the history database; hover
  for values; gaps stay gaps.
- **Theming:** every color is a CSS variable (the token list is in
  DESIGN.md §11.2); `data/custom.css` loads last (editable in Settings →
  Appearance). Moving onto the brand tokens renamed or re-pointed a few
  tokens; for an existing `custom.css`:
  - `--ctl`, `--border-2`, `--accent-bg`, `--glow`, `--pop` and `--fs-label`
    still **read** correctly (they alias `--ctl-border`, `--ctl-border`,
    `--accent`, `transparent`, `--shadow-2` and `--fs-micro`), but **setting**
    one of them no longer recolours anything: set the new name instead.
  - `--border` used to be every hairline. Panel outlines, table rows and
    dividers now use `--line`; `--border` is the stronger outline of dialogs,
    menus, the palette and the primary panel. A `custom.css` that set
    `--border` to recolour panels should set `--line` as well.
  - `--fs-micro` is now 11 px (uppercase labels; same as the old
    `--fs-label`); the old 12 px size is `--fs-xs`.
  - Settings › Appearance warns when the saved `custom.css` sets one of the
    retired names (or `--border` without `--line`) and names the token to set
    instead.
- **Components:** buttons, form fields, dialog, toast, menu, tabs, empty
  states and skeletons are one shared layer (`static/components.css` inlined
  in every page, `static/ui.js` as the first part of `/static/app.js`,
  `window.HB`). DESIGN.md §11.4 to §11.9. Until each form moves onto the
  dialog, the field styles (inputs, checkboxes, switches) apply only inside a
  dialog, and the Settings pages keep their own fields and toasts.

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
