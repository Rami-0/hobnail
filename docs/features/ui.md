# The interface

Server-rendered pages (maud), hand-written CSS and a little vanilla JS. No
framework, no build step, no web fonts, no CDN. Dark first, light follows the
OS, the ◐ button overrides it per browser.

- **Sidebar:** pages, then every compose stack with running/total counts and its
  containers (status dots; pulsing while starting or restarting; stopped ones
  dimmed), each with its logs icon. A stack's name opens it in the table; its
  chevron folds it. Filter box: the text stays while you move between pages
  (until cleared), Enter opens the first match, Down moves into the list. The
  open container's item is scrolled into view. Over 1100 px it collapses to
  an icon rail (remembered); from 700 to 1100 px it is a rail that the toggle
  opens over the page (the page keeps its place under the scrim).
- **Phones (under 700 px):** a bottom bar (Overview, Containers, Logs,
  Alerts, More). "More" is the only way into the drawer: a real link to
  `/more`, a plain page with every page and every stack's containers (so
  everything is reachable with JavaScript blocked), which the script opens
  as the drawer instead. "More" is marked current on Host, History and
  Settings. A closed drawer is out of the tab order; an open one makes the
  page behind inert, has a Close button, and Esc or Back closes it (Forward
  then opens it again).
- **Top bar:** breadcrumb, search, CPU / MEM / DISK chips with sparklines
  (they open the Host page), theme, account. Signed out the account control
  is a plain "Sign in" link back to the page; signed in a menu with who you
  are and Sign out.
- **Command palette:** `Ctrl/⌘ K`. Pages, bookmarks, containers, settings
  sections and actions. A container opens its Hobnail page; under it come
  "Logs: X", "Open service ↗" (when it has a link) and, signed in, "Edit X"
  (its display name, group, icon, link). Signed in, "Add bookmark", "Add
  widget", "Add channel" and "Add alert rule" open those dialogs (in place
  when the page has them). One action per theme ("Theme: Light", …). A query
  that matches nothing selects nothing: Enter does not send it to the search
  engine; arrow down to the "search the web" row (`page.search_url`). Home,
  End, Page Up and Page Down move in the list; on touch it has a close
  button.
- **Overview:** "Needs attention" rows are whole-row links to the cause with
  the reason inline (a container: its page and logs icon; "unhealthy for 41
  min", never longer than the container has been running, plus the first
  line of the failing health check for a signed-in viewer; a firing alert
  about the host, a widget or a URL: that subject and its rule). The probe
  line is looked up for the first 5 unhealthy containers only (one `docker
  inspect` each, signed in only); it is the check's output when the page
  was loaded, kept through live updates while the same failing run goes on
  and dropped when it ends or a new one starts (the live updates are public
  and never carry probe output). Signed in, Bookmarks and Widgets have
  "Add", which opens the Settings dialog on the page. The memory treemap folds tiles too small for a
  name on a phone into "+N others", a link to the table sorted by memory; a
  tile big enough (64 × 40 px, 88 × 56 px on touch) carries its logs icon.
- **Alerts:** firing incidents and the alert log link to their subject (the
  container, the Host page, the widget, the URL), name the rule and show a
  clock time ("since …" in one right-aligned column); rows about a container
  have their logs at that moment. Signed in, channels and rules are the
  Notifications tab's own lists and dialogs ("Add channel", "Add rule",
  "Send test", the rule switches, edit, delete) and a firing rule's name
  opens its dialog; signed out the page shows the read-only tables it always
  did.
- **A config change elsewhere** reloads the open page (owner decision Y12),
  the Overview and Alerts included, except a logs page, a page with an open
  dialog or confirm, and a page with unsaved input: those keep what is on
  screen, and their save gets the 409 with "Reload and keep my input". A
  page's own save swaps in only what it changed (the Overview's Bookmarks
  and Widgets panels; focus stays on the same control).
- **Caching:** any page or fragment rendered with a session is served
  `Cache-Control: no-store` (it can hold the config hash, the CSRF token,
  settings data or probe output); signed out, pages are `no-cache`, Settings
  `no-store`.
- **Pages:** Overview (health strip, host vitals with 1 h charts, needs
  attention, widgets, top consumers, recent events, bookmarks), Containers
  (dense table: filter, sort, group by stack or flat, remembered; phones get
  stacked rows), container detail (charts, timeline, ports and links, labels,
  matching alert rules), History, Alerts, Settings, Login.
- **Container detail** tabs: Overview, Logs (the log viewer itself, at
  `/containers/<name>/logs`, under the same header and tabs), Metrics
  (network, block I/O, PIDs, throttling, OOM), Inspect (env values, commands
  and health-check output only when signed in), Processes (signed in), Events.
  The header has "Open" (when the service has a link) and, signed in, "Edit".
  "Why is this red?" sits on top of every tab: for a failing health check,
  since when and how many in a row, "Logs around HH:MM" and, signed in, the
  probe's first line and Docker's last five checks (folded on a phone);
  signed out it says "Sign in to see the check's output" and the page holds
  no probe output. "Since" is never before the container's own start: a
  container recreated under the same name does not inherit the old one's
  failing time.
- **Logs** one click from everywhere a container is named (see
  `docs/features/logs.md`); the Logs nav item is current on every logs page.
- **Keyboard:** `g` then `o c h l a s` jumps to a page, `j/k` moves in lists,
  `l` opens a container's logs, `/` filters the page where it has a filter
  (Containers, logs) and opens the palette elsewhere, `?` shows all shortcuts.
- **Themes:** Dark, Light, Ember, High contrast (the theme button steps
  through Auto, Dark, Light, Ember, High contrast, one click per step, and
  says the theme it switched to; or Settings → Appearance), all on the brand palette (`docs/BRANDING.md`). Auto follows
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
Keyboard: `Ctrl/⌘ K` palette, `g` then `o c h l a s` for pages, `j/k` rows,
`?` cheat sheet, `l` logs of the current container. The tab title and favicon
show the number of problems. Installable as an app (manifest and icons; no
service worker, no offline page, no data caching); a bottom bar on phones. Decisions: DESIGN.md §10.
