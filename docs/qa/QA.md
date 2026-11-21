# Hobnail QA list (ux-pass audit)

Historical audit: the original findings and file/URL references below describe
the retired Rust UI, not the current React client. Phase 2–5 outcomes are at the
end; [the results](../plans/react-port-phase-2-5.md) record current browser
checks and measurements. Original observations are retained as evidence.

Audit of branch `ux-pass` at build `02c21fb` (the app as it is on `main`, plus
the test fix), 2026-11-15. Five agents walked the app in headless Chromium
(playwright-core 1.64) at 1440 and 390 px, dark and light, signed out and
signed in, against a throwaway instance on `127.0.0.1:3210` seeded with groups,
overrides, bookmarks, widgets (one failing), three channels pointed at the mock
receiver (one failing) and four alert rules. Robustness ran on separate
instances behind a fake Docker API (0 and 150 containers, long and hostile
names). Every row was seen in the browser; code was read only for the
`file:line`. Eleven findings were re-checked by hand before this list was
accepted (see "Spot checks").

**267 findings: 1 blocker, 79 major, 123 minor, 64 polish**,
after merging 25 duplicates reported by more than one agent.

Severity: **blocker** a task cannot be completed or data is lost; **major** a
task is slow, confusing or error-prone for most people; **minor** an annoyance
with an easy way around it; **polish** looks or feels off, nothing is harder.

Priority is the owner priority a finding serves: P1 forms, P2 fewer clicks,
P3 remove what is useless, P4 client-side generosity (motion, loading, empty
and error states), P5 a UI that makes sense, P6 performance.

Status: `open`, `fixed <commit>` or `deferred: <reason>`. Line numbers are for
build `02c21fb`.

## The ten that matter most

| # | Finding | IDs |
|---|---|---|
| 1 | Add and edit use five different form patterns; editors open below the list, not at the thing clicked; Esc does nothing; unsaved input is dropped without a question | SET-01, SET-02, SET-03, SET-04 |
| 2 | Validation is on the server only, one error per round trip, often in a generic box that quotes a config-file line; bad values (a `javascript:` bookmark URL, "not a url") are saved | SET-05, SET-06, SET-07, SET-24, SET-35, SET-40 |
| 3 | The live log tail does not rest on the newest line (5,859 px short at 1440 px, 13,445 px at 390 px, with no "Jump to latest") | LG-12 |
| 4 | Logs are one click away only from the containers table and the detail header. Overview, the sidebar tree, Host and History need two; the "Logs" tab of a container holds no logs; the Logs nav item is a 40-checkbox picker | OV-01, OV-33, NAV-05, LG-04 |
| 5 | The Alerts page has no link or button at all: a firing alert cannot be followed to the cause, the rule or its settings | LG-55, LG-56, LG-57 |
| 6 | "Find last night's incident" does not work: the timeline ignores the chart range, shows the newest 60 events with relative times only, and chart markers do not lead to the event | LG-43, LG-44, LG-47 |
| 7 | Any config save reloads every open page (losing log search, filters, scroll, a half-filled form); the Host page replaces its whole live region every 3 s | LG-03, LG-34 |
| 8 | Phone: forms' Save button sits under the bottom bar, inputs are 12.5 px, 2,956 of 3,227 controls are under 44 px; Alerts, History and container detail scroll sideways; at 700 to 1100 px opening the sidebar squeezes the page to 56 px | SET-13, SET-14, A11Y-18, LG-58, LG-45, OV-40, NAV-21 |
| 9 | Without JavaScript (or before it loads on a slow line) "Change password" submits as GET and puts the passwords in the URL; Settings forms render empty and enabled before they work | ROB-15, ROB-11, SET-18 |
| 10 | No response compression (434 KB for 150 containers, 36 KB gzipped), a 1 s long task on the containers page at 4x CPU, and a dropped stream is nearly invisible | ROB-04, ROB-05, ROB-10, ROB-13 |

Controls to remove (owner priority 3): the Beszel button (OV-35), "Open in
Dozzle" and "Details" row icons (OV-20), the Dozzle palette rows (NAV-53), the
Settings index page (SET-22), the duplicate theme setting (SET-27), the Columns
and Layout options that no longer do anything (SET-26). Full list below.

## Click counts today

Start: Overview, signed in. One click or tap = 1; typing and scrolling are
named, not counted. Counted independently by two agents; where they differ the
range is given.

| Task | Path today | Clicks at 1440 | Clicks at 390 | What makes it long |
|---|---|---|---|---|
| Open a container's logs | Sidebar container → "Logs"; or Containers → row logs icon | 2 | 2 | Nothing on Overview links to logs |
| See why a container is unhealthy | Health strip "unhealthy" → row name → Inspect tab → scroll to "Health check" | 3 + scroll | 3 + long scroll | The detail page shows one word; the probe output is on Inspect, below the fold, signed in only |
| Add a bookmark | Settings → Bookmarks → Add bookmark → Save bookmark | 4 | 5 | Settings index page first |
| Add a widget | Settings → Widgets → Add widget → Add field → [Fetch now] → Save widget | 5 to 6 | 6 to 7 | Index page; 15 inputs before any field |
| Add a channel and send a test | Settings → Notifications → Add channel → [type: open, pick] → Save channel → Send test | 5 to 7 | 6 to 8 | A test is only possible after saving |
| Add an alert rule | Settings → Notifications → Add rule → tick "fire on" → tick channel → Save rule | 5 to 6 | 7 + scroll | The Alerts page lists rules but cannot add or edit one |
| Rename a container | Sidebar container → Customize → Save override | 3 | 4 | Leaves the container page for Settings |
| Change the theme (this browser) | Theme button, once per step through five themes | 1 to 4 | 1 to 4 | A blind cycle; one step back is four presses |
| Change the default theme | Settings → Appearance → Default theme (open, pick) → Save | 5 | 6 | Two settings for one thing |
| Find last night's incident | Alerts or History → scroll to the log / timeline; then container → Logs → Custom range → type From and To → Apply | 1 + scroll to see it, 6 + two typed date-times to reach the logs of that moment | same + More | Relative times only; rows are not links; impossible after 60 newer events |
| From a firing alert to the cause | Overview → Alerts, then nothing | no path | no path | The Alerts page has no links |

### Logs from each place a container is shown

Clicks counted from the element itself to that container's logs.

| Task | Start | Path today (each click) | Clicks at 1440 | Clicks at 390 | Keyboard path | Notes (what makes it long) |
|---|---|---|---|---|---|---|
| Logs from Overview "Needs attention" (container row) | the row | row name (detail page), "Logs" | 2 | 2 | `j` to the row, `l` | Row has one link, to the detail page |
| Logs from Overview "Needs attention" (alert row) | the row | row name goes to `/alerts`, which has no link onward | no path | no path | none | Dead end. Way around: Containers, find the row, logs icon |
| Logs from Overview treemap | a tile | tile (detail page), "Logs" | 2 | 2 | Tab to the tile, `l` | Tile is one link to the detail page |
| Logs from Overview top CPU / memory lists | a name | name (detail page), "Logs" | 2 | 2 | Tab to the name, `l` | Same |
| Logs from the Containers table | a row | logs icon in the row | 1 | 1 | `j`, `l` | Meets the goal. The icon is one of four unlabeled icons (open, logs, Dozzle, details) |
| Logs from the sidebar stack tree (container) | a tree item | item (detail page), "Logs" | 2 | 2 (3 from Overview: More first) | focus the item, `l` | Tree items have no logs action |
| Logs from the sidebar stack tree (whole stack) | a stack row | logs icon on the stack row | 1 | 1 | Tab to it, Enter | Icon is invisible until hover at 1440. Returns 404 for groups that are not compose projects |
| Logs from the container detail page | the page | "Logs" header button (full logs page) or the "Logs" tab (embedded panel) | 1 | 1 | `l` | Two controls with the same word open two different log views |
| Logs from the Alerts page | an incident or rule | nothing on the page is a link | no path | no path | none | Way around: Containers (1), row logs icon (2), after reading the name off the alert |
| Logs from the command palette | palette open, name typed | click "Logs: X" | 1 (2 from Overview: search button first) | 1 (2) | `Ctrl K`, type, arrow down (the container row ranks first), Enter | Enter on the top hit (the container) opens the container's own web page in a new tab, not Hobnail |
| Logs from the Host page container table | a name | name (detail page), "Logs" | 2 | 2 | none (`j` rows there have no link) | |
| Logs from the History "starts per day" table | a name | name (detail page), "Logs" | 2 | 2 | `j`, `l` | |
| Logs from the Logs nav page | `/logs` | tick the container's checkbox, "Show logs" | 2 (3 from Overview) | 2 (3) | Tab through up to 40 checkboxes, Space, Enter | The nav item "Logs" is a picker for side-by-side logs; names are not links; no filter box |

## Form inventory

There are **five different patterns** for the same job:

1. **Page-section form with its own Save** (General, System tiles, Theme, custom.css, Recording, Change password): always visible, one Save per section.
2. **Staged list plus Save** (Hidden containers, Groups): Add / remove / reorder only change the page; nothing is written until a separate Save button is pressed.
3. **Hidden editor appended under the table** (Bookmark, Widget, Channel, Rule): one shared form per section, shown after the last row, however far that is from the row you clicked.
4. **Inline editor inside the table** (container override): the form is moved into a new row under the clicked row.
5. **Immediate actions** (reorder arrows, bulk Hide / Show / Set group, Remove override, Send test): written at once, some with a native `confirm()`, some without.

Shared behaviour, the same in every form unless the inventory says otherwise:

- **Esc does nothing** anywhere. **Enter** in a text input submits (native form submit).
- **Validation is almost all on the server**, one error per round trip. The only client checks are "Required" (first empty field only), the environment-variable name pattern, the custom.css size and the three password checks.
- **Errors** arrive three ways: under the field (when the server names a field), as a generic box above the Save button (when it does not), or as a box at the top of the section (delete / reorder / bulk).
- **Success**: a toast bottom-right ("Saved — revision #N"), the editor closes, the list is re-fetched and re-rendered whole (no highlight of the new row), keyboard focus drops to `<body>`.
- **Unsaved-input guard**: only the browser's `beforeunload` prompt on leaving the page. Nothing guards Cancel, opening another editor, or filtering.
- **Delete** uses the browser's native `confirm()`.

"Clicks" counts from the tab already open, with the mouse, to a saved result; typing is listed separately.

| Form | Where it lives | How it opens | First focus | Fields | How it validates | How errors are reported | What happens on save | Enter / Esc | Unsaved-input guard | Clicks to complete |
|---|---|---|---|---|---|---|---|---|---|---|
| Page options | General, page section | Always open | None | 8: Title, Greeting, Columns, Layout, Theme, Search URL, 2 switches | Server only. Columns range checked; Search URL not checked at all | Field error for Columns 9 ("page.columns must be 1..=6, got 9"); generic box for 2.5 ("page: invalid type: floating point `2.5`, expected u8") | Toast, "Saved" stays next to the button, form re-fetched. Sidebar title not updated until a reload | Enter saves; Esc nothing | `beforeunload` only | 1 + typing |
| Hidden containers (docker excludes) | Containers and groups, page section, staged list | Always open | None | 1 text input + chips | None (`[bad` accepted) | None seen | Add puts a chip on the page only; Save writes. Toast. Text still in the input is dropped silently | Enter in the input adds a chip (does not save); Esc nothing | `beforeunload` only | 2 (Add, Save) + typing |
| Groups | Containers and groups, page section, staged rows | Always open | None | Per row: Title, Icon, Collapsed, Hidden, up, down, remove; plus a new-name input | None client; duplicate name gives an info toast | Toast "Group x is already listed" | Add / remove / move only change the page; "Save groups" writes. Toast | Enter in the new-name input adds a row; Enter in a Title input saves the whole form; Esc nothing | `beforeunload` only | 2 (Add group, Save groups) + typing |
| Container override ("Customize") | Containers and groups, inline row under the clicked row; also reached from the container page | "Customize" / "Edit" button in the row, or `/settings/containers?name=` | Display name (in view) | 8: Display name, Group, Icon, URL, Description, Hide, Order, Card size | Server only; URL and Icon not checked; Order 1.5 refused | Generic box: "set: invalid type: floating point `1.5`, expected i32" | Toast, editor closes, table re-rendered, page scroll jumps | Enter saves; Esc nothing | `beforeunload` only; typing in the search box closes the editor and drops the input | 2 (Customize, Save override) + typing |
| Remove override | Button inside the override editor | Open the editor first | n/a | none | n/a | n/a | Written at once, **no confirmation**, toast | n/a | none | 2 |
| Bulk edit | Containers and groups, bar above the table | Appears when a row checkbox is ticked | None | Group input + Set group, Hide, Show, Remove overrides, Clear selection | "Type a group name first" toast | Section box | Set group written at once (seen); "Remove overrides" asks with `confirm` | Not tried | none | 2 to 3 |
| Bookmark | Bookmarks, hidden editor under the table | "Add bookmark" in the section head, or the pencil in a row | Name | 5: Name, URL, Group, Icon, Description | Client: required (first empty field only). Server: none for the URL | "Required." under Name plus a box "Not saved: fill in the required fields." | Toast, editor closes, list re-fetched, focus to body | Enter saves; Esc nothing | `beforeunload` only; Cancel, another pencil or "Add bookmark" drop the input silently | 2 (Add bookmark, Save bookmark) + typing |
| Bookmark delete / reorder | Bookmarks, row icon buttons | Click | n/a | none | n/a | Section box at the top | Delete: native `confirm`, then toast. Each arrow click is a save and a revision | n/a | n/a | 2 (delete), 1 per step (reorder) |
| System tiles | Widgets, page section | Always open | None | 1 switch | n/a | n/a | Toast; unchanged gives "No changes" | n/a | `beforeunload` | 2 (switch, Save) |
| Custom API widget | Widgets, hidden editor under the table, 1086 px tall at 1440 (1667 px at 390) | "Add widget" or the pencil | Id | 9 inputs + Fields rows + Display (template or rows) + Status dot (2) + Fetch now | Client: required Id, URL (first only). Server: id pattern, duplicate id, URL, refresh minimum, template | Under the field when the server names it; durations go to a generic box with a config-file line number | Toast, editor closes, list re-fetched, page scrolls to the top | Enter in any input (also in a Fields row) saves; Esc nothing | `beforeunload` only | 3 to 5 (Add widget, Add field, [Fetch now], Save widget) + typing |
| Fetch now preview | Inside the widget editor | Button | n/a | uses the unsaved form | Same server validation | Field error or a line in the preview area | Rendered tile plus a table of extracted values; no raw response | n/a | n/a | 1 |
| Command widget | Widgets, read-only list "Defined on the host" | No controls | n/a | none | n/a | A lock note above the list | n/a | n/a | n/a | not possible, by design |
| Channel | Notifications, hidden editor under the Channels table | "Add channel" or the pencil | Name | Name, Type, then by type: Slack 1, Telegram 2, Webhook 2; Timeout | Client: required Name, env-variable name pattern. Server: name pattern, duplicate, "exactly one of", URL | Under the field; bad Timeout goes to a generic box with a line number | Toast, editor closes, list re-fetched (alphabetical, new row not marked) | Enter saves; Esc nothing | `beforeunload` only | 4 (Add channel, Type open + pick, Save channel) + typing |
| Send test | Notifications, button in each saved channel row | Click | n/a | none | n/a | Toast plus a line in the Status cell | "Test sent to x" toast and "✓ Test notification sent" in the row; mock receiver got it | n/a | n/a | 1, only after the channel is saved |
| Alert rule | Notifications, hidden editor under the Rules table, 967 px at 1440 (1446 px at 390) | "Add rule" or the pencil | Name | Name, Type, Enabled, then 5 to 8 condition fields by type, Channels, For, Cooldown, Message, Recovery | Client: required Name. Server: everything else, one error at a time | Under the field; bad durations go to a generic box with a line number | Toast, editor closes, list re-fetched | Enter saves; Esc nothing | `beforeunload` only | 3 to 4 (Add rule, [channel box], Save rule) + typing |
| Channel / rule delete | Row icon button | Click | n/a | none | Server refuses a channel still used by a rule | Section box with a raw validator message | Native `confirm`, toast | n/a | n/a | 2 |
| Default theme | Appearance, page section (same key as Theme in General) | Always open | None | 1 select | n/a | n/a | Toast, page switches theme at once | n/a | `beforeunload` | 3 (open, pick, Save) |
| custom.css | Appearance, page section | Always open | None | 1 textarea, live-preview checkbox, Revert | Client: 64 KiB limit. No syntax check | Under the textarea | Toast "Saved" (no revision) | Enter is a newline; Esc closes nothing | `beforeunload` | 1 + typing |
| Recording (history settings) | History and data, page section | Always open | None | 1 switch, Sample interval | Server only | Generic box (never on the field) | Toast | Enter saves; Esc nothing | `beforeunload` | 1 + typing |
| Export | History and data, link button | Click | n/a | none | n/a | n/a | Browser download `hobnail-export-<unix>.json` (about 23 KB here), no message | n/a | n/a | 1 |
| Clear history | History and data, danger section | Always open | None | Typed confirmation "clear history" (case-insensitive) | Button disabled until the text matches | n/a (not submitted) | Not exercised | Not exercised | Not exercised | 1 + typing |
| Raw config editor | Config file, page section | Always open | None | 1 textarea (28 rows), Save, Revert, Go to error | Server, shortly after typing stops, and again on save | Status line under the editor ("Line 6, column 21: …"); on save also a box | Toast, editor and revision list re-fetched | Ctrl+S saves; Tab indents; Esc closes nothing | `beforeunload`; flagged dirty again after a save (SET-58) | 1 + typing |
| Revision view / diff | Config file, panel under the whole revisions table | "View" / "Diff" in a row | None | Close, "Open in the editor" | n/a | n/a | Panel filled and scrolled into view | Esc does not close | n/a | 1 |
| Restore revision | Config file, row button | Click | n/a | none | n/a | Section box | Native `confirm` (3 lines), toast "Saved — revision #N", note "restored #M" | n/a | Confirm text mentions unsaved edits | 2 |
| Change password | Security, page section | Always open | None | Current, New, Repeat | Client: required, 8 characters, match, one at a time. Server: current password | Under the field ("the current password is wrong") | Not exercised (signs everyone out) | Not exercised | Not checked | 1 + typing |
| Sign out other sessions | Security, button | Click | n/a | none | n/a | n/a | Not exercised | n/a | n/a | 1 |
| Lock panel | Every tab signed out | n/a | n/a | "Sign in" link with `next=` back to the tab | n/a | n/a | n/a | n/a | n/a | 1 to reach the login |

## What is already right

So nobody "fixes" it.

- Every page: `lang`, one `main`, named `nav` landmarks, a working skip link (`Skip to content` moves focus to `#main`), `aria-current` on the active nav item, tab and segment.
- The accessibility tree has **no unnamed button, link, input, select or image** on any of the 25 pages, signed in or out, at 1440 or 390. Every visible input has a label.
- Command palette: `role=dialog aria-modal`, combobox + listbox + `aria-activedescendant`, Tab stays inside, Esc returns focus to where it was. Keyboard-shortcut dialog and account menu also return focus.
- `prefers-reduced-motion: reduce`: **zero** running CSS animations or transitions on 9 pages and with the palette open (51 selectors transition without it); the number tween is gated in JS.
- Body, secondary, muted, link and status text pass 4.5:1 on every surface in all four themes (table below). High contrast passes everything except one browser-default placeholder.
- Nothing is injected: names, images, stacks, labels, env values, health output and log lines containing `<script>`, `<img onerror>`, quotes and `&` render as text on every page, in the palette and in the sidebar filter.
- Docker going away is handled: a red note in the sidebar and on Overview, "showing the last known state" on the table and detail, and recovery without a reload once Docker answers again.
- A slow or failing widget never delays the page: `/` loaded in 69 ms with one widget pending for 20 s.
- Page zoom to 200 % (720 px wide viewport) reflows with no horizontal scroll on the five pages tried.
- The stale-file conflict works as documented (HTTP 409, a banner, nothing overwritten); command widgets are refused from the app and the raw editor (HTTP 403, clear sentence); Send test reaches the mock receiver.
- Containers table: filters, sort, group/flat and collapse work without a page load and are in the URL; sort and grouping are remembered; numbers update in place.

# Findings

## Settings and forms


### All forms (cross-cutting)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| SET-01 | major | Five different patterns are used for add / edit (page section, staged list plus Save, hidden editor under the table, inline row, immediate action); see the summary above. On one tab (Containers and groups) three of them sit under each other with three primary Save buttons | One dialog component for every add / edit, one primary action per view | `src/ui/settings/containers.rs:34`, `src/ui/settings/bookmarks.rs:21`, `static/settings.js:318` | P1 | fixed 9a8dd71 |
| SET-02 | major | Bookmark, Widget, Channel, Rule: the editor is one hidden form that is shown **after the last table row**, not at the row you clicked. With "Edit broken-widget" at 390 px the page scrolls to a form below the whole list and the row is out of sight | The form opens from the thing clicked (a dialog), the list stays visible behind it | `static/settings.js:323` | P1 | fixed 9a8dd71 |
| SET-03 | major | Esc does nothing in any editor (bookmark, widget, channel, rule, override) or in the revision viewer | Esc closes, with a guard when there is unsaved input | `static/settings.js:330` | P1 | fixed 9a8dd71 |
| SET-04 | major | Unsaved input is dropped silently by: Cancel; clicking "Add …" while editing; clicking the pencil of another row (the form is refilled with the other item); typing in the container search box. Example: Bookmarks, edit `qa-bm`, type a description, click "Add bookmark": the description is gone, no question asked. The only guard is the browser's "Leave site?" prompt when leaving the page | One guard ("Discard changes?") on every way out of a dirty form | `static/settings.js:329`, `static/settings.js:564` | P1 | fixed 9a8dd71 |
| SET-05 | major | Errors appear one at a time. Empty bookmark form: only Name gets "Required." although URL is empty too. Widget with a bad id and a bad URL: first save reports the id, second the URL. Every correction costs a server round trip | All problems of a form shown at once, each on its field, checked in the browser where possible (required, URL, duration, id pattern, number range) | `static/settings.js:313`, `static/settings.js:213` | P1 | fixed 9a8dd71 |
| SET-06 | major | A bad duration ("abc" in widget Refresh, channel Timeout, rule For, history Sample interval) is reported in a generic box above Save as `invalid duration "abc" (line 99, column 11)`. The line number refers to the config file, which the form never shows; the field is not marked and focus goes to `<body>` | "Use a duration like 30s or 5m" under the field that caused it | `static/settings.js:212` | P1 | fixed 9a8dd71 |
| SET-07 | major | A number of the wrong kind is answered with HTTP 400 and the raw deserialiser text in a generic box: Order 1.5 → "set: invalid type: floating point `1.5`, expected i32"; Columns 2.5 → "page: invalid type: floating point `2.5`, expected u8" | Whole-number inputs (`step`, `min`, `max`) and a plain message on the field | `src/settings/ops.rs:148`, `src/ui/settings/form.rs:59` | P1 | fixed 9a8dd71 |
| SET-08 | major | After every successful save keyboard focus is on `<body>` (or `<main>`), the editor is gone and the list was rebuilt. A keyboard user starts again from the top of the page: from there it is 11 Tab presses to "Add bookmark" with the skip link, and more than 80 without it | Focus returns to the control that opened the form (or to the new row) | `static/settings.js:329` | P1 | fixed 9a8dd71 |
| SET-09 | major | While a save is running the button is disabled but reads the same ("Save bookmark"); a small grey "Saving…" appears beside it. Cancel and all inputs stay active (seen with the request delayed 2.5 s) | The save button shows progress; the form is inert until the answer | `static/settings.js:221` | P1 | fixed 9a8dd71 |
| SET-10 | minor | The list behind is re-fetched and re-rendered whole after each save; the new or changed row is not marked, and Channels are sorted by name so a new one appears in the middle. The page scroll position jumps (Widgets: back to the top; Containers: 483 → 73 px) | The row is inserted or updated in place with a short highlight, scroll position kept | `static/settings.js:570`, `static/settings.js:617` | P4 | fixed 9a8dd71 |
| SET-11 | minor | "Saved" (or "No changes", "Reverted") stays next to the Save button of page-section forms until the next save; it looks like a current status after further edits | The inline status clears on the next input, or only the toast is used | `static/settings.js:236` | P5 | fixed 4eb67a3 |
| SET-12 | minor | Toasts stack (three at once after quick saves or reorder clicks), cannot be dismissed, and at 1440 px sit over the row action buttons bottom-right. At 390 px the toast (top 796 px) lies on the bottom navigation bar (top 787 px) | A small capped stack, dismissible, placed clear of the bottom bar | `static/settings.css:114`, `static/settings.css:119` | P4 | fixed c38e18b |
| SET-13 | major | Phone (390 px): opening the Bookmark or Channel editor leaves the Save button at y = 795 px, under the fixed bottom bar (787 to 844 px); "Save override" is half covered too. The Widget form is 2 screens long and the Rule form 1.7, with Save at the very end | The dialog's actions stay visible (sticky footer above the bottom bar) | `static/settings.js:325`, `static/settings.css:89` | P1 | fixed 9a8dd71 |
| SET-14 | major | Phone: every text input is 12.5 or 13 px and 32 px high, the Save button 32 px, row actions (pencil, ×, arrows, Send test, Add …) 26 px. Below 16 px iOS Safari zooms the page on focus; the targets are well under 44 px | 16 px inputs on phones, 44 px targets for the actions | `static/settings.css:61`, `static/app.css:180`, `static/app.css:213` | P1 | fixed 9a8dd71 |
| SET-15 | minor | Stale-file conflict (works as documented): with unsaved input, a change on disk shows within about 3 s a blue banner "The config file was changed elsewhere … saving these edits would be refused. [Reload]"; saving anyway answers HTTP 409, the banner turns yellow ("The config file changed on disk (maybe a hand edit). Reload to see it — your unsaved changes are shown below.") and the form gets "Not saved: the file changed on disk since this page loaded." The only offer is Reload, which first raises the browser's "Leave site?" prompt and then throws the input away. Without unsaved input the tab reloads itself with a toast "Reloaded: the config file changed" (good). Seen on bookmark, override, widget, rule, general, raw editor and custom.css | Offer "Reload and keep my input" (re-apply the form to the new file) next to "Discard and reload" | `static/settings.js:137`, `static/settings.js:203` | P1 | fixed 9a8dd71 |
| SET-16 | minor | In the 409 case the banner is at the top of the tab and the "Not saved" box is inside the editor at the bottom; with a long list (Bookmarks with nine rows) only one of the two is on screen | The conflict is reported in the dialog itself | `static/settings.js:204` | P1 | fixed 9a8dd71 |
| SET-17 | minor | Session gone while a form is open (this browser's cookie removed): red banner "Your session expired, sign in again. Nothing was saved. Copy any unsaved changes first: signing in leaves this page. [Sign in]" plus "Not saved: your session expired." Clicking Sign in raises "Leave site?" and the input would be lost | Sign in without leaving (inline password prompt or a new tab), then retry the save | `static/settings.js:140` | P4 | fixed 9a8dd71 |
| SET-18 | major | With JavaScript off (signed in) the list tabs show "Loading…" for ever and the General form is empty; there is no message | A `<noscript>` note at least; the brief asks for pages that still work without script | `src/ui/settings/form.rs:207`, `src/ui/settings/mod.rs:126` | P4 | fixed 9a8dd71 |
| SET-19 | polish | Deletes and Restore use the browser's native `confirm()` box, the staged lists use none, Clear history uses a typed phrase: three confirmation styles | One confirm-destructive component | `static/settings.js:591`, `static/settings.js:1150` | P5 | fixed 9a8dd71 |
| SET-20 | polish | Required fields are not marked (no asterisk); you learn which are required by submitting | Mark required fields | `src/ui/settings/form.rs:99` | P1 | fixed 9a8dd71 |

### Settings index and signed-out lock panel

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| SET-21 | minor | Signed out, `/settings` still shows all nine cards under the lock panel and the tab strip stays (nine links); each one leads to the same lock panel "Sign in to change settings". The Sign in link does carry `next=` back to the tab (good) | Signed out: the lock panel only (or cards visibly locked) | `src/ui/settings/mod.rs:140` | P3 | fixed 7d502a4 |
| SET-22 | major | `/settings` is a page of nine cards that repeat the nine tabs; it is an extra click on the way to every form (counted in every task above) (Also reported as NAV-12.) | Open the first tab (or the last used one) directly | `src/ui/settings/mod.rs:133` | P2 | fixed 7d502a4 |
| SET-23 | polish | Phone: the tab strip scrolls sideways with a hidden scrollbar; at 390 px two and a half of nine tabs are visible and nothing hints that there are more | A visible cue (fade, arrow) or a select | `static/settings.css:23` | P5 | fixed 7d502a4 |

### General

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| SET-24 | major | Search URL accepts anything: "not a url" was saved (HTTP 200, a revision) with no warning, also without `{q}` | Check for an http(s) URL containing `{q}` | `src/ui/settings/general.rs:21` | P1 | fixed 9a8dd71 |
| SET-25 | minor | Save a new Title: the toast says saved, but the sidebar and the tab title keep the old name until the page is reloaded | The chrome updates after the save | `static/settings.js:46` | P4 | fixed 9a8dd71 |
| SET-26 | minor | "Columns" ("Cards per row in a group") and "Layout" ("Cards or compact rows") are offered, but `docs/features/ui.md` says columns no longer has an effect (there is no card grid). "Show live CPU and memory on cards" likewise speaks of cards. Their effect was not tested here | Remove the controls that do nothing, or say what they do now | `src/ui/settings/general.rs:18` | P3 | fixed 9a8dd71 |
| SET-27 | polish | Theme can be set here and again under Appearance ("Default theme"); same key, two forms, different labels (Also reported as NAV-42.) | One place | `src/ui/settings/general.rs:20`, `src/ui/settings/appearance.rs:19` | P3 | fixed 9a8dd71 |
| SET-28 | polish | A 129-character title is accepted without limit or warning | A sensible length limit | `src/ui/settings/general.rs:16` | P1 | fixed 9a8dd71 |

### Containers and groups (and Customize from a container page)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| SET-29 | major | Hidden containers: "Add" (or Enter) only puts a chip on the page; a pattern still in the input when Save is pressed is ignored without a word (typed `qa-pending-not-added`, Save, toast "Saved", pattern not saved) | Adding a pattern saves it, or Save includes what is in the input | `static/settings.js:397` | P1 | fixed 9a8dd71 |
| SET-30 | major | Groups: Add group, remove (×) and the arrows only change the page; nothing is written until "Save groups". The × looks the same as the × that deletes a bookmark for real after a question | The same rule everywhere: either every action saves, or every list is staged and says so | `static/settings.js:398`, `static/settings.js:468` | P1 | fixed 9a8dd71 |
| SET-31 | minor | Groups: Enter in the "new group" input adds a row; Enter in a Title input one line above saves the whole form | Enter does the same thing in the same form | `static/settings.js:405` | P1 | fixed 9a8dd71 |
| SET-32 | major | "Remove override" in the override editor deletes all overrides of the container at once (name, group and icon of `hobnail-dev-mock` gone) with **no confirmation**, while the bulk "Remove overrides" asks first | Confirm, or offer undo | `static/settings.js:418` | P1 | fixed 9a8dd71 |
| SET-33 | major | Bulk bar: ticking the checkbox of the last of 40 rows shows the bar at the top of the section, 1191 px above the viewport at 1440 px and 4065 px above at 390 px. Nothing visible happens where you clicked. "Set group" then writes at once without confirmation | A bar that stays in view (sticky) with the count | `static/settings.css:205`, `static/settings.js:425` | P1 | fixed 9a8dd71 |
| SET-34 | major | Typing in the "Search containers" box while the override editor has unsaved input closes the editor as soon as its row is filtered out; the input is lost | Keep the editor (dialog) open independently of the filter | `static/settings.js:491` | P1 | fixed 9a8dd71 |
| SET-35 | major | Override URL and Icon are not checked: "not a url" and "no such icon !!" were both saved | Check the URL; check or preview the icon | `src/ui/settings/containers.rs:102` | P1 | fixed 9a8dd71 |
| SET-36 | minor | Exclude patterns are not checked (`[bad` was saved) and nothing shows which containers a pattern would hide before saving | A live "matches N containers" line | `static/settings.js:389` | P4 | fixed 05a7da8 |
| SET-37 | minor | "Customize" on a container page leaves the page for Settings → Containers and groups, scrolled to the row with the editor open (focus in Display name: good). After Save you stay in Settings; there is no link back to the container | Rename in a dialog on the container page | `src/ui/pages/container.rs:166` | P2 | fixed 9a8dd71 |
| SET-38 | polish | Group row headers "Collapsed" and "Hidden" repeat the label printed next to every checkbox; the group Title input has no length limit (a 240-character title was saved) | Label once | `static/settings.js:457` | P5 | fixed 9a8dd71 |
| SET-39 | polish | The button reads "Customize" for a container without overrides and "Edit" for one with; the heading says "Override for …"; the save button "Save override". Three words for one thing | One word | `static/settings.js:515`, `src/ui/settings/containers.rs:98` | P5 | fixed 9a8dd71 |

### Bookmarks

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| SET-40 | major | The URL is not checked although the hint says "http(s) link.": "not a url" and `javascript:alert(1)` were both saved as bookmarks (HTTP 200) | Accept only http(s) URLs, error on the field | `src/settings/ops.rs:877`, `src/ui/settings/bookmarks.rs:25` | P1 | fixed c38e18b |
| SET-41 | major | One long name without spaces (308 characters) breaks the tab: the Name column takes the width, every URL wraps three characters per line, one row grows to thousands of pixels and the page scrolls sideways (document 2973 px wide at 1440) | Long values wrap or are cut with an ellipsis; a length limit in the form | `static/settings.css:132` | P5 | fixed 4eb67a3 |
| SET-42 | minor | Each click on an up / down arrow is a full save: a request, a toast, a new revision ("bookmarks: reordered") and a rebuilt list. The same for widgets | Drag or arrows that save once | `static/settings.js:581`, `static/settings.js:639` | P2 | fixed in M2 (D17): stage arrow moves, then Save order or Cancel |
| SET-43 | minor | The same name can be added any number of times (five `qa-bm` rows); they cannot be told apart in the list or in the delete question | Warn on a duplicate name | `static/settings.js:567` | P1 | fixed 4eb67a3 |

### Widgets

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| SET-45 | major | The widget editor is one long page section: 9 inputs, Fields, Display, Status dot, preview; 1086 px high at 1440 (taller than the window) and 1667 px at 390. Save is at the very bottom, a field error can be more than a screen away from it | A dialog with a fixed footer; group or step the parts | `src/ui/settings/widgets.rs:79` | P1 | fixed 385940e |
| SET-46 | major | "Fetch now" shows the rendered tile and the extracted values only. The raw response is not shown, so the paths for "Fields" have to be found elsewhere before the form can be filled in | Show the response and let a value be picked to create a field | `static/settings.js:734` | P1 | deferred: owner decision at the design gate (Y19): the preview keeps returning extracted values only, never the raw response |
| SET-47 | minor | Field rows and display rows have no visible labels, only placeholders ("name", "path.to.value") that vanish once filled | Column labels | `static/settings.js:660` | P5 | fixed 385940e |
| SET-48 | minor | A long title (160 characters) makes the tab scroll sideways (document 1499 px wide at 1440) | Wrap or ellipsis | `static/settings.js:643` | P5 | fixed 4eb67a3 |
| SET-49 | minor | The Id can be changed while editing (saved `qa-w` as `qa-w2`); the hint says alert rules use the id, but nothing says what happens to them | Lock the id on edit, or say which rules refer to it | `src/ui/settings/widgets.rs:82` | P1 | fixed 385940e |
| SET-50 | polish | System tiles is a single switch with its own Save button; switching it does nothing until Save | A switch that saves | `src/ui/settings/widgets.rs:62` | P2 | fixed 385940e |
| SET-51 | polish | Command widgets: refused as designed. In the app they are a separate read-only list "Defined on the host" with a lock and the sentence "Defined in the config file on the host — command widgets can't be created, changed or enabled from the app."; the list has no buttons or inputs at all, and the Add form offers no Type. Clear | Keep; the sentence could link to the docs | `src/ui/settings/widgets.rs:14` | P5 | fixed 05a7da8 |

### Notifications

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| SET-52 | major | "Send test" exists only on saved channel rows. A new channel has to be saved first, then tested (7 clicks from the Overview); the widget form has its test ("Fetch now") inside the editor (Also reported as NAV-13.) | Test from inside the channel dialog before saving, as for widgets | `static/settings.js:808` | P2 | fixed 385940e |
| SET-53 | minor | Deleting a channel that a rule still uses: the question says "Rules that send to it must be changed first." (it says so for every channel, used or not); after OK the save is refused and the section shows `alert "qa-r-container": unknown channel "qa-hook" (configured: down-webhook, …)` | Say before asking: "Used by rule x: remove it there first" | `static/settings.js:823` | P1 | fixed 385940e |
| SET-54 | minor | The alert-rule form shows 13 fields for the default type and is 967 px high (1446 px at 390). To switch a rule off or on you open this whole form, flip "Enabled" and save | An on / off switch in the rule row; optional fields folded away | `src/ui/settings/notifications.rs:49` | P2 | fixed 385940e |
| SET-55 | minor | Host rules: with neither Above nor Below, and with both, the same message appears under Above: `alert "qa-r-host": set exactly one of above, below`. Channel messages are of the same kind: `channel "qa-slack" (slack): set exactly one of webhook_url_env, webhook_url_file`, naming a config key that is not in the form | Plain words that use the labels on screen | `static/settings.js:213` | P1 | fixed 385940e |
| SET-56 | polish | The yellow box "down-webhook: url is stored in the config file as plain text …" is shown on every visit for every plain-URL channel and cannot be dismissed; the same box is reused for the delete error, which replaces it | Show it once in the row (the "plain URL" pill already does) | `static/settings.js:826` | P3 | fixed 385940e |
| SET-57 | polish | Mock receiver after the tests: `qa-hook` (webhook) and `qa-slack` (Slack) arrived (`kind: test` / Slack text); the Telegram channel with an unset variable reported "misconfigured: environment variable … is not set"; `down-webhook` reported `HTTP 500: {"ok":false,"description":"mock failure"}`. All four results were shown as a toast and in the Status cell. Works; the failing text is the raw response body | Keep; trim the raw body | `static/settings.js:813` | P4 | fixed 46c3b46 |

### Appearance

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| SET-44 | minor | custom.css is saved without any check: text with stray braces ("}}} not css") was saved and applied | A syntax warning before saving | `static/settings.js:923` | P1 | fixed 4eb67a3 |
| SET-59 | polish | "Revert" goes back to the last saved text, which after SET-44 was the broken one; custom.css has no revisions (config saves do) | Keep the previous version | `static/settings.js:922` | P4 | fixed 05a7da8 |

### History and data

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| SET-61 | minor | Sample interval errors never land on the field: "1s" gives a generic box "history.sample_interval must be between 5s and 5m, got 1s", "abc" gives `invalid duration "abc" (line 145, column 19)`. A bare "20" is accepted and shown back as "20" with no unit | Error on the field; show the unit | `static/settings.js:974` | P1 | fixed 9a8dd71 |
| SET-62 | polish | Export starts a download and gives no feedback on the page; the file name is `hobnail-export-<unix seconds>.json` | A readable date in the name, a toast | `src/ui/settings/history.rs:26` | P4 | fixed 5cb678e |
| SET-63 | polish | Clear history: the button is disabled until "clear history" is typed (case-insensitive: "Clear History" enables it). Seen up to that point, then cancelled | Fine; use the shared confirm-destructive component | `static/settings.js:977` | P5 | fixed 9a8dd71 |

### Config file

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| SET-58 | major | After a successful save with Ctrl+S the form is flagged as changed again as soon as focus leaves the editor (flag false right after the save, true after one click elsewhere on the page). Seen result: the Restore question then says "Your unsaved edits in the editor will be replaced." although everything is saved | The dirty flag follows the text only | `static/settings.js:244`, `static/settings.js:1010` | P1 | fixed 9a8dd71 |
| SET-64 | major | The revisions table lists every revision (62 rows here) with three buttons each; the page is 3740 px high. "View" and "Diff" fill a panel placed **under the last row**: clicking Diff on a row near the top scrolls the page to the bottom, past all rows | Paged or collapsed list; view and diff in a dialog | `src/ui/settings/config.rs:44`, `static/settings.js:1109` | P1 | fixed 4eb67a3 |
| SET-65 | minor | Stale conflict in the raw editor shows both banners at once (the yellow "changed on disk" and the blue "changed elsewhere"), each with its own Reload button | One banner | `static/settings.js:1079` | P5 | fixed 9a8dd71 |
| SET-66 | minor | Changing a command widget in the raw editor (refused, as designed): while typing, the status line turns to a lock: "Would be refused: this changes the command widget "procs"; command widgets can only be added, changed or enabled in the config file on the host". Save answers HTTP 403 and adds a box with the same sentence prefixed "refused:". Clear, but nothing points at the lines that caused it ("Go to error" is hidden) | Mark the changed block; one message, not two | `static/settings.js:1092` | P1 | fixed 05a7da8 |
| SET-67 | minor | A syntax error is reported under the editor ("Line 6, column 21: invalid basic string, expected `"`") but the editor does not scroll to it; with the editor scrolled to the end the line is off screen until "Go to error" is pressed. A type error reads `invalid type: string "ninety", expected f64` | Scroll the first error into view; plain wording | `static/settings.js:1054` | P1 | fixed 4eb67a3 |
| SET-68 | polish | Restore asks with a three-line native `confirm`; after OK the toast says "Saved — revision #65", not "Restored #63" (the note in the list does) | Say what happened | `static/settings.js:1151` | P5 | fixed 9a8dd71 |
| SET-69 | polish | Phone: the editor is 296 px wide at 12.5 px with no wrapping, 600 px high; most lines are cut and the editor fills most of the screen, so the page and the editor compete for the scroll | A full-screen editor on phones | `src/ui/settings/config.rs:22` | P5 | deferred: polish, not reached in this pass; the raw editor works on a phone but does not wrap |

### Security

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| SET-70 | minor | Change password checks one thing at a time (empty current → "Required."; then "At least 8 characters."; then "Does not match the new password."), each needing another press of the button. A wrong current password (tried once) comes back under the field as "the current password is wrong" | Check as you type; show all problems together | `static/settings.js:1163` | P1 | fixed c38e18b |
| SET-71 | polish | "Sign out all other sessions" is a plain secondary button with no count of sessions next to it (not pressed, so whether it asks first is not known) | Say how many sessions there are | `src/ui/settings/security.rs:40` | P5 | fixed 9a8dd71 |

## Navigation, chrome and click paths


### Click paths (item 1)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| NAV-05 | major | Sidebar tree: a container item is one link to the detail page. Logs take 2 clicks at 1440 and 3 taps on a phone (More, item, Logs) | A logs action on each tree item | `src/ui/shell.rs:525` | P2 | fixed 7c58552 |
| NAV-07 | minor | Sidebar tree at 1440: the stack logs icon has opacity 0 until the row is hovered or the icon is focused. On touch it is drawn at 0.75 | Visible at rest (dimmed is fine) | `static/app.css:391` | P2 | fixed 7c58552 |
| NAV-11 | major | Why a container is unhealthy: the detail page's "Health check" cell shows one word and is not a link. The probe output is on the Inspect tab, 990 px down at 1440 and 1845 px down at 390, and only when signed in | The reason on the detail Overview tab next to the state, or the cell links straight to it | `src/ui/pages/container.rs:1181`, `src/ui/pages/container.rs:638` | P2 | fixed 7d502a4 |
| NAV-15 | minor | Host page "by container" table and History "starts per day" table: the name links to the detail page; logs are a second click | Same row action as the Containers table | `src/host/view.rs:307`, `src/ui/pages/history.rs:269` | P2 | fixed 7c58552 |

### Sidebar (item 2)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| NAV-21 | major | Widths 700 to 1100 (measured at 900; the toggle behaves the same at 720, 1000 and 1100): pressing the sidebar toggle opens the sidebar as an overlay and the page content collapses into the 56 px rail column. The screen is the sidebar on a black page until it is closed | The content keeps its place under the scrim | `static/app.css:143` | P5 | fixed 7d502a4 |
| NAV-22 | minor | Sidebar filter: Enter does nothing and the arrow keys do not move into the results | Enter opens the first match; Down moves into the list | `static/app.js:110` | P2 | fixed 7d502a4 |
| NAV-23 | minor | Sidebar filter is emptied by every navigation: type, click a match, and on the next page the full tree is back | The filter text survives navigation until cleared | `static/app.js:111` | P5 | fixed 7d502a4 |
| NAV-24 | minor | Opening a container far down the tree leaves its highlighted item out of sight (item at y = 1729 in a 900 px window) | The current item is scrolled into view on load | `static/app.js:100` | P5 | fixed 7d502a4 |
| NAV-25 | minor | Clicking a stack name in the tree only folds the stack. On Overview and on the detail page the same stack name links to `/containers?stack=`. Fold state is remembered and survives live updates (works) | The chevron folds; the name goes to the stack | `src/ui/shell.rs:510` | P5 | fixed 7d502a4 |
| NAV-26 | polish | Collapsed to the rail, the toggle still says "Collapse sidebar" (title and accessible name) with `aria-pressed="true"` | "Expand sidebar" | `src/ui/shell.rs:164`, `static/app.js:84` | P5 | fixed 7d502a4 |
| NAV-27 | minor | The rail (1440 collapsed, and always at 700 to 1100) drops the whole stack tree, the filter and the live text. Its only labels are native `title` tooltips. Collapse state is remembered across pages and reloads (works) | Styled tooltips on the rail icons; a way to reach stacks from the rail | `static/app.css:129`, `static/app.css:138` | P5 | fixed 4eb67a3 |

### Phone, 390 px (item 3)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| NAV-31 | minor | Bottom bar: on Host, History and Settings no item is marked current ("More" holds them but stays unmarked) | "More" is current when the page lives in the drawer | `src/ui/shell.rs:245` | P5 | fixed 7d502a4 |
| NAV-33 | minor | Drawer: no swipe to close and no edge swipe to open (tried with touch events). The Back button leaves the page instead of closing the drawer | Swipe left closes; Back closes | `static/app.js:79` | P4 | fixed 7d502a4 |
| NAV-34 | polish | Drawer: the only close control inside it is the rail toggle, labelled "Collapse sidebar". The phone styles mean to hide that button but it is still shown | A close button that says "Close" | `static/app.css:152`, `src/ui/shell.rs:164` | P5 | fixed 7d502a4 |
| NAV-35 | minor | Two buttons open the same drawer on every phone screen: the hamburger (top left) and "More" (bottom right). "More" holds Host, History, Settings and the stack tree, plus the four pages already in the bottom bar | One way in | `src/ui/shell.rs:200`, `src/ui/shell.rs:260` | P3 | fixed 7d502a4 |

### Top bar (item 4)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| NAV-40 | minor | The CPU, MEM and DISK chips all link to `/history`. The Overview's own Host panel links to `/host` | Chips go to the host page (or to that metric on it) | `src/ui/shell.rs:402` | P5 | fixed 7d502a4 |
| NAV-41 | minor | Theme button: each press steps auto, dark, light, ember, high contrast. Only the icon changes; the name is in the tooltip, which a phone never shows. From "auto" on a dark OS the first press changes nothing visible. One step back costs four presses (Also reported as SET-60.) | A menu listing the five themes with the current one marked | `static/app.js:54`, `src/ui/shell.rs:209` | P2 | fixed 7d502a4 |
| NAV-43 | minor | Account icon, signed out: it opens a menu with one sentence and one item, "Sign in". Signing in costs 3 clicks. Signed in, the menu holds "Settings" (already in the sidebar) and "Sign out" | Signed out, the control is a "Sign in" link | `src/ui/shell.rs:311` | P2 | fixed 7d502a4 |
| NAV-44 | polish | Breadcrumbs work (parents are links). On the two-level logs crumb at 390 the "Containers" crumb is squeezed to 0 px. The split view `/logs?c=` shows the single crumb "Logs" with no link back to the picker | A link back to the picker | `src/logs/pages.rs:289` | P5 | fixed d5f6bfd |

### Command palette (item 5)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| NAV-50 | major | Enter or a click on a container result opens that container's own web page in a new tab when it has a URL, and the detail page only when it has none. The detail page needs Shift+Enter. On a phone there is no Shift and the hint row is hidden. Everywhere else a container name opens its detail page | A container result opens the container in Hobnail; "Open service" is its own row or modifier | `static/app.js:552` | P5 | fixed 7d502a4 |
| NAV-51 | minor | Typing a container name ranks "Container" first and "Logs: X" third or lower (scored one point down). The top hit is the one that leaves Hobnail | Logs directly reachable from the top hit | `static/app.js:513` | P2 | fixed 7d502a4 |
| NAV-52 | minor | No actions for the top tasks: "add", "rule", "rename" and "restart" return only "search the web". Settings sections match by section name only | "Add bookmark", "Add widget", "Add channel", "Add alert rule", "Rename X" as actions | `static/app.js:480`, `src/ui/routes.rs:116` | P2 | fixed 7d502a4 |
| NAV-53 | minor | Every container gets an "Open in Dozzle: X" row next to "Logs: X" (typing "dozzle" lists 40). "Logs: X" itself opens the in-app logs page, so the docs line `"Logs: X" via Dozzle` is out of date | Drop the Dozzle rows; fix the docs | `static/app.js:512`, `docs/features/ui.md:14` | P3 | fixed 7c58552 |
| NAV-54 | minor | Empty palette: 7 pages, 4 actions, then all 40 containers, and the 3 bookmarks at positions 52 to 54. Settings sections are not listed until typed | Bookmarks before the container list | `static/app.js:507` | P5 | fixed 7d502a4 |
| NAV-55 | minor | "Toggle theme" moves one step in the five-theme cycle and closes the palette. Typing "light" finds it, and picking it gives dark | One action per theme | `static/app.js:485` | P2 | fixed 7d502a4 |
| NAV-56 | minor | `/` opens the palette only when no filter box is visible. At 1440 with the sidebar open it focuses the sidebar's "Filter containers"; on Containers the table filter; on a logs page the log search; in the rail or on a phone the palette. The docs say `/` is the palette | One meaning for `/` per page, stated in the cheat sheet | `static/app.js:629`, `docs/features/ui.md:13` | P5 | fixed 4eb67a3 |
| NAV-57 | minor | Enter on a query with no match opens a new tab on the configured search engine straight away. A mistyped container name is sent to an outside site. Works as documented | The web-search row is not preselected when it is the only row, or it asks first | `static/app.js:547` | P5 | fixed 7d502a4 |
| NAV-58 | polish | Arrow keys wrap, Tab stays in the box, Esc and a click or tap outside close, `Ctrl K` toggles (all work). Home, End and Page keys do nothing. No close button; on a phone the only hint is an "Esc" key cap. No page context (on a container page it offers nothing for that container) | A close button on touch; context rows first | `src/ui/shell.rs:346`, `static/app.css:271` | P4 | fixed 7d502a4 |

### Keyboard shortcuts (item 6)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| NAV-60 | minor | `j` / `k` select rows that cannot be opened: 59 rows on Host, 16 on Alerts, 73 on Settings Config file, 3 on Settings Bookmarks. Focus is not moved into the row and Enter does nothing | `j` / `k` only where Enter does something | `static/app.js:598` | P5 | fixed 4eb67a3 |
| NAV-61 | minor | There is no key for History: `g h` is Host. `g` plus `o c h l a s` work on every page tried except Login, where the password field has focus | A key for History, listed in the sheet | `static/app.js:593`, `src/ui/shell.rs:279` | P2 | fixed 4eb67a3 |
| NAV-62 | polish | Cheat sheet: "/ Filter this page" is wrong on Overview, Host, History, Alerts and Settings. It omits Shift+Enter in the palette and says "Ctrl K" on every platform. It opens with `?` and its button; Esc, the X and a click outside close it (works) | A sheet that matches the page | `src/ui/shell.rs:289` | P5 | fixed 4eb67a3 |
| NAV-63 | polish | `l` does nothing, silently, when the selected row is not a container (Overview stack rows and alert rows, Host rows). It works on table rows, on any focused container link (tree item, treemap tile) and on the detail page | No change needed if NAV-60 is fixed | `static/app.js:613` | P5 | fixed 4eb67a3 |
| NAV-64 | polish | Shortcuts are off while any input has focus, including checkboxes and selects: after ticking a box on `/logs`, `g o` does nothing. Typing in a text field correctly does not fire them (tried in the sidebar filter and the log search) | Off only for fields that take text | `static/app.js:16` | P5 | fixed 4eb67a3 |

### Installed-app view and offline page (item 7)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| NAV-70 | major | The offline page (`/offline`) is never shown. There is no service worker and nothing links to it. With the network off, tapping a nav link gives the browser's own error page | Either a minimal worker that serves `/offline` for failed navigations, or remove the page | `src/ui/pwa.rs:58`, `src/ui/pwa.rs:109` | P4 | fixed 7c58552 |
| NAV-72 | minor | The installed app's "Logs" shortcut opens the picker (NAV-10), and a 401 "Sign in to read logs" page when signed out | Shortcuts that land on something useful | `src/ui/pwa.rs:102` | P2 | fixed 4eb67a3 |
| NAV-73 | polish | Manifest is valid: standalone, scope `/`, SVG plus 192 and 512 px PNG icons and a 180 px apple-touch icon, all 200. `theme_color` and `background_color` are fixed dark values. Nothing in the CSS reacts to standalone display and there is no in-app back or reload control | Acceptable; a reload control once NAV-70 is decided | `src/ui/pwa.rs:93` | P4 | fixed 05a7da8 |

### Login and logout (item 8)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| NAV-80 | minor | Signing out always lands on Overview, whatever page you were on (tried from a container's Processes tab), with no message | Stay on the page when it can be shown signed out; say "Signed out" | `static/app.js:146` | P5 | fixed 4eb67a3 |
| NAV-81 | polish | Four different sign-in prompts for the same job: the account menu item, a "Sign in to reveal" text link (Inspect), a sentence with a "Sign in" link (Processes, Settings sections) and an empty-state button with HTTP 401 (logs). All carry the right `next` and return to the exact page and tab after signing in (works) | One prompt pattern | `src/ui/pages/container.rs:598`, `src/ui/pages/container.rs:842`, `src/logs/pages.rs:62`, `src/ui/shell.rs:327` | P5 | deferred: polish, not reached in this pass; the prompts differ in wording only and all lead to the same login |
| NAV-82 | polish | The login page has no cancel or back link; the way out is the sidebar. Visiting `/login` while signed in shows "Signed in as ... Continue or sign out" (works). The password field is focused on load (works) | A "Back" link to `next` | `src/ui/pages/login.rs:25` | P5 | fixed 4eb67a3 |

## Overview, containers table, container detail


### Overview `/`

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| OV-01 | major | No container shown on Overview has a logs link. From the memory treemap or Top CPU it is click 1 to the detail page, click 2 on "Logs". "Needs attention" rows link only to `/alerts`. (Also reported as NAV-01, NAV-02, NAV-03.) | Logs one click from every place a container is shown: a logs icon on Top CPU rows, on treemap hover, and on attention rows that name a container. | `src/ui/pages/overview.rs:302`, `:376`, `:189` | P2 | fixed 7d502a4 |
| OV-02 | major | "Top CPU → All" opens `/containers?sort=cpu` in the default grouped view. Rows are sorted only inside each stack and stacks stay alphabetical, so the busiest container is somewhere down the page. Same for "Memory by container → All". | These links open the flat view (`group=0`) so the first row is the top consumer, or grouped sort orders the stacks by their top row. | `src/ui/pages/overview.rs:320`, `:331`, `static/app.js:388` | P5 | fixed 7d502a4 |
| OV-03 | minor | "Last 24 h" says "running 100%" for every stack when 94 of 96 cells are "no data" (instance up a few minutes). The strip's accessible name says "running 100% of the last 24 h". | Percentage is of the time with data and says so, or shows "not enough history yet". | `src/ui/view/strip.rs:60`, `src/ui/pages/overview.rs:491` | P5 | fixed 1938f93 |
| OV-04 | minor | Needs-attention rows: only the name is a link; the rest of the row is dead. Every row goes to the generic `/alerts` list, so the failing-widget row does not lead to the widget. (Also reported as NAV-04.) | The whole row is the target and leads to the cause: the container, the widget's settings, or that incident. | `src/ui/pages/overview.rs:189` | P2 | fixed 7d502a4 |
| OV-05 | minor | Recent events show raw event kinds as labels: `start`, `delivery_failed`, `password_changed`, `fired`. `start` rows read "start · version 0.0.1 (…)" with no subject, five identical rows in a row. (Also reported as LG-49.) | Plain words ("Hobnail started", "Delivery failed", "Password changed", "Alert fired"); identical consecutive rows collapsed. | `src/ui/pages/mod.rs:129`, `src/ui/pages/overview.rs:699`, `src/main.rs:94` | P5 | fixed 1938f93 |
| OV-06 | minor | The alert `fired`, `delivery_failed` and login rows in Recent events are plain text and lead nowhere. Detail text is cut with "…"; at 390 px only about 10 characters survive ("broken-widg…", "HT…") with no way to read the rest. | Rows link to the alert, channel or container; long text wraps on phones or expands on tap. | `src/ui/pages/overview.rs:701`, `static/widgets.css:111` | P2 | fixed 05a7da8 |
| OV-07 | minor | Chart hover read-out is drawn on top of the chart's own header. On "Network in" and "Network out" it covers the current value ("5.5 K…", "42 …"); on container charts it covers the min/max text. (Also reported as LG-53.) | The read-out replaces the header value while hovering, or sits inside the plot area. | `static/widgets.css:61`, `static/app.js:221` | P4 | fixed 2d17ac0 |
| OV-09 | minor | Memory treemap at 390 px: names are clipped at the top edge of short cells, values run into the cell edge ("51.1 Mi"), several cells show a name and no number. | Cells too small for a line show nothing and rely on the title or tap; fewer cells on phones. | `src/ui/pages/overview.rs:378`, `static/widgets.css:165` | P5 | fixed 7d502a4 |
| OV-10 | minor | Treemap: "+N others" takes about a third of the area and is not a link, while every other cell is. Cell colours mean "stack" but nothing says so. One "tiny" cell is a coloured sliver with no label. | "+N others" links to `/containers?sort=mem&group=0`; a one-line legend or the stack name in the tooltip. | `src/ui/pages/overview.rs:387` | P5 | fixed 7d502a4 |
| OV-11 | minor | Host tile "Network" has an empty dotted sparkline on every load and stays flat for the first seconds, while the CPU and temperature tiles next to it are already drawn. | Seed the network sparkline like the CPU tile's, or show a skeleton until two samples exist. | `src/ui/pages/overview.rs:527`, `static/app.js:168` | P4 | fixed 1938f93 |
| OV-12 | minor | Signed in, Overview offers no way to manage what it shows: Widgets and Bookmarks have no "Add" or "Edit" link. | A small "Edit" / "+" in the section header when signed in. | `src/ui/pages/overview.rs:712` | P2 | fixed 7d502a4 |
| OV-13 | polish | Page shifts 3 px about 200 ms after load: the date/clock block is filled by JS and grows from 21 to 38 px (layout shift 0.05). | Render date and time on the server, or reserve the height. | `src/ui/pages/overview.rs:74`, `static/app.js:43` | P4 | fixed 5cb678e |
| OV-14 | polish | Top CPU bars are scaled to the top row, so 11% draws a full-width bar; values mix "9.4%" and "11%". The panel is as tall as the treemap but holds five rows, leaving about half empty. | Bars scaled to one core (or labelled relative); one number format; more rows or a shorter panel. | `src/ui/pages/overview.rs:307` | P5 | fixed 1938f93 |
| OV-15 | polish | The y-axis "0" label collides with the first x-axis time label on charts with wide y labels (Memory, Network). | Zero label aligned with the other y labels, clear of the x axis. | `src/ui/chart.rs:60` | P5 | fixed 2d17ac0 |
| OV-16 | polish | Health strip says "unhealthy"; the same filter is "Problems" on the Containers page; the tooltip says "Unhealthy, restarting or dead". | One word for one thing on both pages. | `src/ui/pages/overview.rs:157`, `src/ui/pages/containers.rs:311` | P5 | fixed 1938f93 |
| OV-17 | polish | The red event marker on the 1 h charts is an 11 px wide link to `/alerts` explained only by a native `title` tooltip (none on touch). | Marker text shown in the chart read-out when the crosshair is on it. | `src/ui/pages/overview.rs:643` | P4 | fixed 2d17ac0 |
| OV-18 | polish | Stack names in "Last 24 h" and table group headers are monospace; the sidebar shows the same names in the UI sans. | Mono for identifiers only; stack titles in one face everywhere. | `src/ui/pages/overview.rs:488` | P5 | fixed 1938f93 |

### Containers table `/containers`

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| OV-19 | major | At 390 px the table header is hidden and nothing replaces it: the list cannot be sorted on a phone (0 sort controls visible). | A "Sort" control in the toolbar below 700 px. | `static/cards.css:83` | P5 | fixed 1938f93 |
| OV-20 | major | Each row has four icon-only actions, two of them duplicates: "Open in Dozzle" (a second logs button that opens another app) next to "Logs", and a "Details" chevron that does what the name link does. "Open service" also repeats the port chip. The two log icons differ only by tooltip. | One "Logs" action and at most one "Open". Remove the Dozzle and Details buttons. | `src/ui/pages/containers.rs:519` | P3 | fixed 7c58552 |
| OV-21 | major | Flat view at 1440 px truncates the columns people read ("Standal…", "running 2 we…", "healthy 26 h…", "Mock receive…") while the 24 h strip keeps a fixed 136 px and about 60 px between Ports and the actions is empty. Grouped view also cuts "running 12 minu…". | State is never truncated (short forms "12 min", "2 wk"); name and state get the spare width. | `static/cards.css:45`, `static/app.css:300` | P5 | fixed 1938f93 |
| OV-22 | minor | Sorting by CPU or Memory while grouped sorts inside each stack only. The header arrow says "CPU ↓" but the top row is not the highest. | Sorting by a number switches to flat, or orders stacks by their top row. | `static/app.js:388` | P5 | fixed 1938f93 |
| OV-23 | minor | Rows highlight on hover but a click anywhere except the name does nothing; the cursor stays an arrow. | Whole row opens the detail page, or no hover highlight. | `static/cards.css:29` | P2 | fixed 1938f93 |
| OV-24 | minor | Empty state is the same sentence for every cause: "No containers match this filter. Clear filters", also for "Stopped 0" and "Problems 0" where the answer is good news. The header row re-flows when the table empties. | "Nothing is stopped." / "No problems." for the state filters; fixed column widths. | `src/ui/pages/containers.rs:442` | P4 | fixed 5cb678e |
| OV-25 | minor | Port chips come in two looks with no explanation: green links and grey plain text. The difference is only in the tooltip. | Grey chips say what they are or get a copy action. | `src/ui/pages/containers.rs:512` | P5 | fixed 05a7da8 |
| OV-26 | minor | Images without a tag show as `sha256:0123abcd…`, which identifies nothing. | Show a repository or compose image name when the image is a bare digest; digest in the tooltip. | `src/ui/pages/containers.rs:481` | P5 | fixed 1938f93 |
| OV-27 | minor | CPU and memory sparklines are empty dotted lines on every page load and fill in only after several live samples. Each is scaled to its own maximum, so a row at "0.0%" draws a peak as tall as a row at 12%, and every memory sparkline is the same flat line. | Seed from history; one shared scale per column, or drop the memory sparkline. | `static/app.js:157`, `src/ui/chart.rs:671` | P4 | deferred: seeding each row's sparkline from history costs a database read per row on every page load, against the idle-CPU budget; revisit after the bench phase |
| OV-28 | minor | A state filter reached from Overview (the "0 stopped" tile) is written to the remembered view (`rh-ctr-view` gets `state`), but the sidebar link then opens "All". Sort and grouping are restored; state and stack are stored and ignored. | Remember only sort and grouping, and do not store the rest. | `static/app.js:325` | P5 | fixed 1938f93 |
| OV-29 | polish | Toolbar controls are all 32 px high but use three font sizes (13, 12.5, 14 px), and segment buttons are 26 px inside a 32 px frame. | One control size and one font size. | `src/ui/pages/containers.rs:307` | P5 | deferred: polish, not reached in this pass |
| OV-30 | polish | "Showing 40 of 40 containers" stays at 40 when a group is collapsed and 38 rows are visible. | Count what is visible, or say "2 hidden in collapsed stacks". | `static/app.js:388` | P5 | fixed 1938f93 |
| OV-32 | polish | The stack filter is a native `<select>` beside custom segmented controls. | Same control style as the rest of the toolbar. | `src/ui/pages/containers.rs:313` | P5 | deferred: polish, not reached in this pass |

### Container detail: header and tabs

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| OV-33 | major | The "Logs" tab contains no logs: one sentence, a primary button "Open logs of <name>" and a configuration footnote. The header already has a "Logs" button. Reaching logs through the tab is 2 clicks. (Also reported as NAV-08, LG-02.) | The tab shows the live tail inline, or the tab itself links to the logs page. | `src/ui/pages/container.rs:284` | P2 | fixed 7c58552 |
| OV-34 | major | Signed in, the only primary button in the header is "Customize", the least frequent action; on the Logs tab a second primary appears below it. "Open" and "Logs" are secondary. | One primary per view: "Logs" (or "Open"). "Customize" becomes a quiet button. | `src/ui/pages/container.rs:166`, `:285` | P5 | fixed 7c58552 |
| OV-35 | major | "Beszel" header button opens the whole host in another app, not this container, next to a Metrics tab that already has the charts. | Remove (already decided by the owner). | `src/ui/pages/container.rs:164` | P3 | fixed 7c58552 |
| OV-36 | major | At 390 px the six tabs do not fit: "Processes" is cut at the screen edge and "Events" is off-screen. The bar scrolls sideways but the scrollbar is hidden and nothing hints at it. | Tabs fit, or a fade/arrow shows there is more. | `static/detail.css:2` | P5 | fixed 1938f93 |
| OV-37 | minor | Changing tab or range is a full page load that jumps to the top. The range is kept across Overview, Logs and Metrics but dropped by Inspect, Processes and Events, so Metrics at 30 d → Inspect → Metrics is back at 6 h. The Logs tab link carries a `range` that means nothing there. | Range kept in every tab link; tab and range changes keep the scroll position. | `src/ui/pages/container.rs:184` | P4 | fixed 1938f93 |
| OV-38 | minor | At 390 px the header buttons wrap to two rows with "Customize" alone on the second. | Buttons on one row once Beszel is gone. | `static/cards.css:109` | P5 | fixed 1938f93 |
| OV-39 | polish | The display name in the page heading is monospace although it is a title, not an identifier. | Display name in the UI sans; raw name stays mono in the sub-line. | `src/ui/pages/container.rs:161` | P5 | fixed 1938f93 |

### Container detail: Overview tab

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| OV-40 | major | At 390 px the page scrolls sideways (document 455 px wide) for a container with a port published on a non-loopback address: the "Ports and links" table is wider than its panel and pushes the whole page, bottom bar included. Breaks the "no horizontal page scroll at 390 px" rule. | The table stacks on phones, or the link column wraps or ellipsises. | `src/ui/pages/container.rs:1189`, `:266` | P5 | fixed 1938f93 |
| OV-41 | minor | Two numbers under the same label on one screen: the tile says "CPU 14%" (live) and the chart header below says "CPU 6.1%" (static, from history). Same for Memory (4.8 vs 4.4 MiB). | Chart header says what it is ("avg 6 h") or follows the live value. | `src/ui/pages/container.rs:1162`, `src/ui/chart.rs:586` | P5 | fixed 46c3b46 |
| OV-42 | minor | Tiles: "Exits, last hour · 0 · since the app started watching" is hard to parse; the container id sits under "Health check"; for a healthy container "State: healthy" and "Health check: healthy" say the same thing twice. | "Restarts (1 h)"; id moves to the header sub-line; State shows running/stopped, Health shows health. | `src/ui/pages/container.rs:1172`, `:1183` | P5 | fixed 1938f93 |
| OV-43 | minor | Ranges with little data draw almost nothing: 30 d is a single 2 px dot per chart, and 6 h (the default) is a sliver at the right edge for a container started minutes ago. No hint why. | Default to the shortest range that has data; say how much history exists when the range is mostly empty. | `src/ui/pages/container.rs:197` | P4 | fixed 1938f93 |
| OV-44 | minor | Labels panel dumps every compose label (config hash, config files, working dir, version…) at full length; on a compose container it is the tallest panel, and about a screen of hashes and paths at 390 px. | Collapsed by default, or compose bookkeeping labels behind "Show all". | `src/ui/pages/container.rs:1214` | P3 | fixed 1938f93 |
| OV-45 | minor | "Alert rules" row is cut: "watching: unhealthy, restarting, exit_no…" at 1440 px and just "watching…" at 390 px. | Wrap to two lines. | `src/ui/pages/container.rs:1250` | P5 | fixed 1938f93 |
| OV-46 | polish | The live tile sparklines are empty dotted lines on load; at 390 px the tile grid ends with an empty bordered sixth cell. | Seed from history; five tiles without an empty cell. | `static/widgets.css:124` | P4 | fixed 1938f93 |
| OV-47 | polish | The "running" / "healthy" value in the State tile is indented about 6 px against its label and the other tiles. | Same left edge. | `src/ui/pages/container.rs:1153` | P5 | fixed 1938f93 |
| OV-48 | polish | A link line between charts and panels ("Network, disk I/O, PIDs and throttling on the Metrics tab") repeats the tab bar. | Remove. | `src/ui/pages/container.rs:252` | P3 | fixed 1938f93 |

### Container detail: Metrics tab

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| OV-49 | minor | Counts are formatted as decimals: "PIDs 3.00 · min 1.00 · max 3.00", "OOM kills (this run) 0.00". | Whole numbers for counts. | `src/ui/chart.rs:52` | P5 | fixed 2d17ac0 |
| OV-50 | minor | Charts of things that are almost always zero take a full slot each ("Throttled periods 0.0%", "OOM kills 0.00"); "Block write" draws a y-axis of "1 B/s, 0 B/s, 0". CPU and Memory repeat the Overview tab. | Zero-only series collapse to one line of text; axis ticks never repeat. | `src/ui/pages/container.rs:395`, `:409` | P3 | fixed 1938f93 |
| OV-51 | minor | Footnote "Error and warning counts are recorded for containers watched by a `log_match` alert or listed in `[logs] count`." is config-file jargon, shown on the Metrics and Logs tabs about a chart that is not there. | Shown only as that chart's empty state, in plain words with a link to the setting. | `src/logs/pages.rs:508` | P5 | fixed 46c3b46 |
| OV-52 | polish | At 390 px the min/max text is cut ("min 1.8 MiB · ma…"). "PIDs" is a lone full-width chart after rows of two. | min/max wraps under the title on phones; even grid. | `src/ui/chart.rs:586`, `src/ui/pages/container.rs:425` | P5 | fixed 2d17ac0 |
| OV-53 | polish | "Restarts" tile sub-line wraps to "by policy · 0 starts in 24 h, 0 in 7 / d". | Shorter text. | `src/ui/pages/container.rs:340` | P5 | fixed 1938f93 |

### Container detail: Inspect tab

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| OV-54 | minor | At 390 px the Networks table is cut inside the MAC column and Aliases is off-screen; it scrolls inside its panel with no hint. | Stack to label/value pairs on phones like the blocks above it. | `src/ui/pages/container.rs:566` | P5 | fixed 1938f93 |
| OV-55 | polish | Mounts "Mode" says the same thing twice for read-only ("read-only" pill plus "ro") and uses plain text for "read-write"; the row order changes between page loads. | One pill per row, both states styled the same; stable order. | `src/ui/pages/container.rs:587` | P5 | fixed 1938f93 |
| OV-56 | polish | Signed out, hidden values appear as the quoted string `"hidden: sign in to view"`, while the Environment panel has a separate "Sign in to reveal" link: two patterns on one tab. | One pattern: a muted "Sign in to view" link in place of each hidden value. | `src/ui/pages/container.rs:806`, `:598` | P5 | fixed 1938f93 |
| OV-57 | polish | The note "Values are never part of this page; revealing fetches them for your session only." touches the last variable row. | Normal paragraph spacing. | `src/ui/pages/container.rs:608` | P5 | fixed 1938f93 |

### Container detail: Processes tab

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| OV-58 | minor | At 390 px the table is cut after "RSS": "Elapsed" is half visible and "Command" is off-screen with no scroll hint. | Command first, or on its own line under each row on phones. | `src/ui/pages/container.rs:771`, `static/detail.css:28` | P5 | fixed 1938f93 |
| OV-59 | polish | Heading says "1 processes". | "1 process". | `src/ui/pages/container.rs:769` | P5 | fixed 1938f93 |
| OV-60 | polish | RSS values are right-aligned in the UI sans under a left-aligned header, between monospace left-aligned columns. | Numeric columns right-aligned with right-aligned headers, one face. | `src/ui/pages/container.rs:771` | P5 | fixed 1938f93 |

### Container detail: Events tab

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| OV-61 | minor | Empty state reads "No all events recorded for this container." (the filter name is pasted into the sentence). | "No events recorded for this container yet." for All; "No OOM events." for a filter. | `src/ui/pages/container.rs:888` | P4 | fixed 5cb678e |
| OV-62 | polish | Seven filter buttons (All, Lifecycle, Alerts, Exits, Health, OOM, Starts) sit above an empty list; "Starts" and "Exits" look like subsets of "Lifecycle". | Hide filters when there is nothing to filter; fewer, non-overlapping kinds. | `src/ui/pages/container.rs:80` | P3 | fixed 1938f93 |

## Logs, Host, History, Alerts


### Logs: reaching them (picker, split view, links)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| LG-01 | major | On `/logs`, `/containers/<name>/logs`, `/stacks/<x>/logs` and `/logs?c=…` the sidebar highlights **Containers**, not **Logs**; the phone bottom bar does the same. Clicking "Logs" appears to select nothing. (Also reported as NAV-20.) | The Logs item is current on every logs page. | `src/logs/pages.rs:174` (also 80, 199, 230, 289, 360); `Nav::Logs` exists at `src/ui/shell.rs:105` | P5 | fixed 7c58552 |
| LG-04 | major | The picker at `/logs` is a flat list of checkboxes (40 here) with "Show logs" at the bottom; on a phone the button is below 40 rows. No filter, no grouping by stack, no way to open a whole stack, and it shows raw container names while the sidebar shows display names. (Also reported as NAV-10.) | A filter box, stacks as entries, one click on a name opens that container's logs, the action stays in view. | `src/logs/pages.rs:294` | P2, P5 | fixed 7c58552 |
| LG-05 | minor | Picker: "Show logs" with nothing ticked reloads the picker (`/logs?`) with no message. Ticking six shows the first four and silently drops the rest; boxes are never disabled at four. | Disable the button until one is ticked; stop at four with a visible count. | `src/logs/pages.rs:247`, `src/logs/pages.rs:307` | P4 | fixed d5f6bfd |
| LG-06 | major | `/logs?c=<one name>` (what the picker gives for one tick, and what "split view" in the single-log footer links to) shows one pane with **no container name anywhere**: the breadcrumb is "Logs" and the pane header is only drawn for two or more panes. The footer "split view" link on that page links to itself. | Always show which container a pane belongs to; "split view" opens the picker with this container pre-ticked. | `src/logs/pages.rs:443`, `src/logs/pages.rs:469` | P5, P2 | fixed d5f6bfd |
| LG-07 | major | The split view has no way to change what is shown: no add, no close on a pane, no link back to the picker. Changing one of four panes means going back to `/logs` and ticking all four again. | Close button per pane and an "Add container" control. | `src/logs/pages.rs:440` | P2 | fixed d5f6bfd |
| LG-08 | minor | `/logs?c=hobnail-dev,nope` shows one pane and says nothing about the missing name; `/logs?c=nope` silently shows the picker. The "Not found: …" banner can never appear, because unknown names are filtered out before it is computed. | Say which names were not found. | `src/logs/pages.rs:247`, `src/logs/pages.rs:267` | P4 | fixed d5f6bfd |
| LG-09 | major | The stack-logs icon in the sidebar is drawn for every sidebar group, including groups defined in the config (the seeded "monitoring" group). For those it links to `/stacks/<group>/logs`, which answers 404 "No such stack". 2 of 11 icons were dead. The icon is invisible until the row is hovered. (Also reported as NAV-06.) | Only offer stack logs where a compose project exists, or make the route accept a config group. | `src/ui/shell.rs:517`, `src/logs/pages.rs:186` | P2, P5 | fixed 7c58552 |
| LG-10 | minor | The only other way to a stack's logs is a small footer link "whole stack …" under a single container's log. Not reachable from the picker or the split view. | Stack logs offered wherever a stack is named. | `src/logs/pages.rs:475` | P2 | fixed d5f6bfd |
| LG-11 | minor | Signed out, every logs URL answers 401 with "Sign in to read logs" and a reason. The reason quotes config syntax (`[logs] require_login`) to a visitor, and "Sign in" is a small inline text link. The round trip works (`next=`), but a `#t=` deep link loses its fragment. | Plain sentence, a real button, and the fragment carried through the login. | `src/logs/pages.rs:62`, `static/app.js:669` | P5, P4 | fixed 5cb678e |

### Logs: the viewer (single, stack, split)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| LG-12 | blocker | **The live tail does not sit at the newest line.** After loading `/containers/hobnail-dev-mock/logs` with Wrap on (the default), the pane rests 3,065 px above the bottom at 1440 px and 10,264 px above it at 390 px, showing the oldest loaded lines. It still believes it is following: no "Jump to latest" pill, and new lines arrive out of sight. Clicking "Jump to latest" after a burst left it 5,483 px short. With Wrap off the gap is about 1 px per line. The stack view (single-height lines) was fine. Likely cause: lines are `content-visibility:auto` with a 19 px placeholder, so `scrollHeight` is an estimate when `scrollTop` is set once. | Following means the last line is visible, after load, after each batch and after "Jump to latest". | `static/logs.css:53`, `static/logs.js:340`, `static/logs.js:201` | P4 | fixed 7c58552 |
| LG-03 | major | Saving any setting anywhere (another tab, another person) reloads every open page. On a logs page that throws away the search, level filters, Pause, the chosen range, the scroll position and the lines held while paused. Seen as reloads at 40, 42, 44, 46, 52 and 55 s while another agent saved settings. Host and History reload too. (Also reported as NAV-16.) | A config change updates what it affects; a log viewer is left alone, or keeps its state in the URL. | `src/live.rs:23`, `static/app.js:868` | P4 | fixed 7c58552 |
| LG-13 | major | Search only highlights. With "45 lines · 1 match" the view does not move to the match and there is no next/previous; the only way to find it is "Only matching". The search is not in the URL, so a reload or a shared link loses it. | Enter / Shift+Enter step through matches; the first match scrolls into view; the query is kept in the URL. | `static/logs.js:557` | P2 | fixed d5f6bfd |
| LG-14 | major | When filters hide everything ("Only matching" with no match, or every level chip off) the pane is blank. The counter says "0 of 45 lines" in small grey text at the far right. | An empty state in the pane: "No lines match", with a way to clear the filter. | `static/logs.js:424`, `static/logs.js:215` | P4 | fixed 5cb678e |
| LG-15 | major | A range with no lines never says so. A custom range two to three days back shows "Waiting for lines…" for ever; the code's "No lines in this range." is unreachable because "Connecting…" has already been replaced. The counter and level chips keep the previous numbers ("2,003 lines") over an empty pane. | "No lines in this range." and counters reset to 0. | `static/logs.js:258`, `static/logs.js:279` | P4 | fixed 5cb678e |
| LG-16 | minor | Custom range accepts From later than To, and a From in the future, without a word: empty pane, "Waiting for lines…". | Validate on Apply and say what is wrong on the field. | `static/logs.js:600` | P1, P4 | fixed d5f6bfd |
| LG-17 | minor | The two custom-range inputs have no visible labels and ask for seconds. Choosing "Custom range…" pushes them into the second toolbar row and shifts Wrap / Time / Pause / Download sideways. At 390 px they make the whole page scroll sideways (document 552 px wide). | Visible "From" / "To", minute precision, a fixed place, no page overflow on a phone. | `src/logs/pages.rs:426`, `static/logs.css:23` | P1, P5 | fixed d5f6bfd |
| LG-18 | major | A burst blocks the page. 1,500 lines sent to `hobnail-dev-mock` in a few seconds produced 48 long tasks of 60 to 780 ms in the viewer (single pane, no search active). | Rendering stays under a frame budget during bursts. | `static/logs.js:310`, `static/logs.js:347` | P6 | deferred: bench phase stopped before the per-frame render budget was measured |
| LG-19 | major | Phone, 390 px: the toolbar is five rows (187 px) and, in the stack view, the chip header adds 173 px, so the log starts at 435 px of an 844 px screen and runs under the bottom bar. Toolbar controls are 24 to 26 px tall. The level chips row scrolls sideways with no scrollbar and "Other" is cut off. | A one-row toolbar on phones (search plus a "Filters" sheet); chip header collapsed; larger controls. | `static/logs.css:106`, `src/logs/pages.rs:394` | P5 | fixed d5f6bfd |
| LG-20 | major | Copy line and permalink exist only as hover buttons. They cannot be reached by keyboard and are switched off entirely on touch screens, so a phone cannot copy a line or get a `#t=` link. | Reachable by tap and by keyboard. | `static/logs.css:86`, `static/logs.js:630` | P4 | fixed d5f6bfd |
| LG-21 | minor | There is no way to clear the view, and no way to load older lines by scrolling up: at the top of the pane nothing happens. Older lines need the range selector, which reloads everything. | "Load older" at the top (or on scroll); Clear if the owner wants it. | `static/logs.js:182` | P2 | fixed d5f6bfd |
| LG-22 | minor | The level gutter of each line is drawn as a 12 px rounded block with a pointer cursor and a hover background instead of the 3 px bar the logs stylesheet asks for: the class `.lg` is also the chart-legend button in the main stylesheet, and those rules leak in. The same leak restyles the dots in the level chips. | Distinct class names; a 3 px bar that does not look clickable. | `static/app.css:335`, `static/logs.css:56`, `static/logs.js:363` | P5 | fixed d5f6bfd |
| LG-23 | minor | Stack view on a phone: container chips on each line are cut to 10 characters, so several containers of one stack read the same ("web-app…", "web-app…"). At 1440 px the 16-character cut does the same to longer names, and message text starts at a different column per line. | Chips that stay distinguishable and a fixed chip column. | `static/logs.css:67`, `static/logs.css:120` | P5 | fixed d5f6bfd |
| LG-24 | polish | Stack pane title "16 containers, merged by time" is cut to "16 containers,…" because the chips take the row. | Title on its own line or shortened. | `src/logs/pages.rs:215`, `static/logs.css:40` | P5 | fixed d5f6bfd |
| LG-25 | minor | Stack view loads the last 500 lines of each container and merges them. In the 5,000 lines held, busy containers covered a few minutes and quiet ones 18 hours, so the upper part of the "merged" timeline contains only the quiet containers. | Merge a common time window, or mark where each container's history starts. | `static/logs.js:225` | P5 | fixed d5f6bfd |
| LG-26 | minor | Four panes at 1440 px are about 280 px each. With timestamps on, and a date prefix on older lines, the message column is about 110 px and every line wraps into eight or more rows. | Timestamps above the message or off by default in narrow panes. | `static/logs.css:37` | P5 | fixed d5f6bfd |
| LG-27 | polish | Lines from another day show the date as `10-26 23:44:36` (month-day, no year). | A date format that cannot be misread, e.g. "26 Oct". | `static/logs.js:128` | P5 | fixed d5f6bfd |
| LG-28 | minor | A JSON line without a `msg` key shows the whole raw JSON as its summary and then repeats the first fields as `key=value` under it, so each line takes three rows and a search hit is highlighted twice. | Summary from the fields only when there is no message. | `static/logs.js:381` | P5 | fixed d5f6bfd |
| LG-29 | minor | Pause leaves every pane header saying "LIVE" with a green dot; only the grey counter says "paused". | Header shows "paused" and how many lines are waiting. | `static/logs.js:571` | P4 | fixed 5cb678e |
| LG-30 | minor | A `#t=` link is read only on page load. Pasting another permalink into the same tab changes the address and nothing else. A link to a time before the oldest line highlights the first line as if it were the target; a link to the future shows "Waiting for lines…". | React to `hashchange`; say when the linked time is not in the log. | `static/logs.js:675`, `static/logs.js:446` | P4 | fixed d5f6bfd |
| LG-31 | polish | Toolbar at 1440 px is two ragged rows: search, Only matching, context, levels, then the range select alone at the far right; Wrap, Time, Pause, Download on a second row. Buttons are 26 px, the search 32 px, chips 24 px. "Only matching" and its "No context" select are two controls for one job. | One row with consistent control height; context folded into "Only matching". | `static/logs.css:12`, `src/logs/pages.rs:394` | P5 | fixed d5f6bfd |
| LG-32 | polish | Download ignores the search and level filters and, in "Live", fetches 5,000 lines while the page shows 500. The tooltip says "the selected range". | Say what will be downloaded, or offer "what I see". | `static/logs.js:585`, `src/logs/pages.rs:435` | P5 | fixed d5f6bfd |
| LG-33 | polish | Single log page at 1440 x 900 scrolls by 29 px although the pane is sized to fill the window. | No page scroll. | `static/logs.js:663` | P5 | fixed d5f6bfd |

### Host `/host`

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| LG-34 | major | The live region is replaced as a whole every 3 s. "Show N bridges" closes itself within 3 s of being opened; any sideways scroll inside a table jumps back; a text selection is lost. Measured layout shift over 18 s: 0.57. Column widths of the two process tables differed on each of six refreshes. | Patch numbers in place; keep open/scroll state; fixed column widths. | `src/host/view.rs:29`, `src/host/view.rs:508` | P4 | fixed 5cb678e |
| LG-35 | major | "Top processes", 1440 px, signed in: the table is 911 px wide in a 551 px box. The CPU header shows as "CP", and RSS and Container are off to the right, behind a scroll that resets every 3 s. Signed out (no command lines) it fits. | Command line truncated to the space left; CPU, RSS and Container always visible. | `src/host/view.rs:354`, `static/host.css:30` | P5 | fixed 2d17ac0 |
| LG-36 | major | Phone, 390 px: every table on the page is wider than its panel (Sensors 358, By container 386, File systems 377, Devices 362, Network 397 to 424 in 332 px). In "By container" the Memory column is cut to one character; in Network "Drops" is out of view. Each needs a sideways scroll that resets every 3 s. | Tables that fit 390 px. | `src/host/view.rs:336`, `src/host/view.rs:488`, `static/host.css:41` | P5 | fixed 2d17ac0 |
| LG-37 | minor | File systems (671 px) and Devices (626 to 633 px) also overflow their 551 px halves at 1440 px. | Fit, or let the Disks panel use one column. | `src/host/view.rs:388`, `static/host.css:31` | P5 | fixed 2d17ac0 |
| LG-38 | minor | The panel "GPU, S.M.A.R.T., systemd" is mostly two paragraphs explaining what is not shown. | Drop the two "not shown" rows; keep GPU only when there is something to show. | `src/host/view.rs:523` | P3 | fixed 2d17ac0 |
| LG-39 | minor | History charts on Host have hover but no drag-to-zoom, no event markers and no compare, while the same-looking charts on History have all three. The range links reload the whole page (262 KB of HTML). | Same chart behaviour on both pages. | `src/host/view.rs:710` | P5, P6 | fixed 46c3b46 |
| LG-40 | polish | 7 d and 30 d on a young database draw a one-pixel sliver at the right edge with no hint that history starts today. | "History starts <date>" or a range that fits the data. | `src/host/view.rs:680` | P4 | fixed 2d17ac0 |
| LG-41 | polish | Chart groups whose count is not a multiple of four leave lighter empty cells in the grid. The CPU panel is shorter than Pressure beside it, leaving a gap under it. | Even grid. | `static/widgets.css:40`, `src/host/view.rs:83` | P5 | fixed 05a7da8 |
| LG-42 | polish | The intro says numbers refresh every 3 s; the footer says "Rates over the last 14 s" (4 s and 15 s on other loads). | One statement of the averaging window. | `src/host/view.rs:95` | P5 | fixed 2d17ac0 |

### History `/history`

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| LG-43 | major | The incident timeline ignores the chart window. After zooming to 15:37 to 15:44 the list below still starts with an event from 41 s ago. It always shows the newest 60 rows with no "older" link, so an incident older than the last 60 events cannot be reached from the page. | The timeline follows the range / zoom, and pages back. | `src/ui/pages/history.rs:81` | P2 | fixed 2d17ac0 |
| LG-44 | major | Event markers on the charts do not lead to the event. An alert marker links to `/alerts`; a container marker links to `/history#events`, the top of the unfiltered list, with no row marked. What happened is only in a native tooltip. | Clicking a marker shows that event: the row highlighted, or the timeline filtered to that time. | `src/ui/chart.rs:300`, `src/ui/chart.rs:381`, `src/ui/chart.rs:619` | P2 | fixed 2d17ac0 |
| LG-45 | major | 390 px: the page scrolls sideways (839 px wide). Timeline rows never wrap, so the detail text runs off the screen and the Incidents / All events switch is pushed out of view. | Rows wrap or stack on a phone; no page overflow. | `static/widgets.css:107` | P5 | fixed 2d17ac0 |
| LG-46 | major | Drag-to-zoom is mouse only. On a touch screen the charts cannot be zoomed, and the hint "Drag across a chart to zoom in" is still shown. | Touch zoom, or hide the hint. | `static/app.js:248`, `src/ui/pages/history.rs:115` | P4 | fixed 2d17ac0 |
| LG-47 | minor | Timeline times are relative only ("13m ago"); the clock time is in a tooltip. (Also reported as NAV-14.) | Clock time shown, at least for events older than an hour. | `src/ui/pages/overview.rs:698`, `src/ui/fmt.rs:35` | P2 | fixed 2d17ac0 |
| LG-48 | minor | Timeline rows link only container events (to the container page). Alert, delivery and widget events are plain text; no row links to logs at that time. | Each row links to its subject and, for containers, to logs at that moment (`#t=`). | `src/ui/pages/overview.rs:701` | P2 | fixed 2d17ac0 |
| LG-50 | minor | A drag shorter than five minutes does nothing and says nothing. A zoomed window cannot be moved earlier or later; the only way out is "Reset zoom". | Snap to the minimum with a note; earlier / later buttons. | `src/ui/pages/history.rs:179` | P4 | fixed 2d17ac0 |
| LG-51 | polish | Legend toggles (avg / max, from 24 h up) act on one chart only and are forgotten on reload; in 1 h and 6 h there is no legend. | Same toggle for all charts, remembered. | `static/app.js:267` | P5 | fixed 2d17ac0 |
| LG-52 | polish | "Compare with yesterday" disappears without explanation at 7 d and 30 d, and the toolbar shifts. It looks like a checkbox but is a link that reloads the page. | Disabled with a reason, in a fixed place. | `src/ui/pages/history.rs:102` | P5 | fixed 2d17ac0 |
| LG-54 | polish | 390 px: the note under "Container starts per day" breaks into three narrow columns. | One wrapped sentence. | `static/app.css:247`, `src/ui/pages/history.rs:280` | P5 | fixed 2d17ac0 |

### Alerts `/alerts`

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| LG-55 | major | The page has no link and no button at all. A firing incident ("memory 51.8% > 1%", "broken-demo widget error…") cannot be clicked: not to Host, not to the widget, not to the container, not to its rule. Overview's firing alerts link here, so the path ends on this page. (Also reported as NAV-09.) | Each incident links to the thing that is wrong and to the rule that fired. | `src/ui/alerts.rs:66` | P2 | fixed 7d502a4 |
| LG-56 | major | The firing list shows the subject but not the rule; the rule name is only in a native tooltip. The Rules table says "1 firing" per rule, so matching a firing line to its rule is by guessing. | Rule name visible on the incident. | `src/ui/alerts.rs:66`, `src/ui/pages/alerts.rs:146` | P5 | fixed 7d502a4 |
| LG-57 | major | Rules and channels cannot be acted on here: no edit, no add, no "Send test", no link to Settings → Notifications. | Rule and channel names link to their edit dialog; "Add rule" and "Send test" on the page for a signed-in user. | `src/ui/pages/alerts.rs:138`, `src/ui/pages/alerts.rs:169` | P2, P1 | fixed 7d502a4 |
| LG-58 | major | 390 px: the page scrolls sideways (822 px wide) because Alert log rows do not wrap. In the Rules table "Watches" breaks mid-word ("unhealth / y"); in Channels the "Last ok" column is cut to "LA". (Also reported as NAV-30.) | No page overflow; stacked rows on a phone. | `static/widgets.css:107`, `src/ui/pages/alerts.rs:133`, `src/ui/pages/alerts.rs:164` | P5 | fixed 7d502a4 |
| LG-59 | minor | Failure counts are retry attempts: one alert that cannot be delivered read "0 sent · 9 failed", then 11, 28, 29 within half an hour. It looks like 29 lost notifications. | "1 notification undelivered (29 attempts)". | `src/ui/alerts.rs:36`, `src/ui/pages/alerts.rs:180` | P5 | fixed 2d17ac0 |
| LG-60 | minor | Thresholds have no unit: "memory above 1", "disk above 90". The rule's `for` time and cooldown are not shown. | "memory above 1 %", "for 5 min". | `src/ui/pages/alerts.rs:109` | P5 | fixed 46c3b46 |
| LG-61 | minor | The alert log repeats itself and uses raw kinds: "fired · memory · memory-high: 47.4% > 1% (47.4)", `delivery_failed`. Rows are not links, and times are relative only. | One readable sentence per event, linked, with a clock time. | `src/ui/pages/alerts.rs:36`, `src/ui/pages/overview.rs:699` | P5 | fixed 2d17ac0 |
| LG-62 | polish | Idle channels say "not used yet" in a grey pill, the same weight as a state; firing rows and rule rows have no hover state. | Quieter idle state; hover on rows once they are links. | `src/ui/pages/alerts.rs:175`, `static/alerts.css:16` | P4 | fixed 5cb678e |

## Accessibility and robustness


### Keyboard and focus (all pages)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| A11Y-01 | major | At 390 px the closed sidebar drawer is only moved off screen (`transform`), so its 70-odd links stay in the tab order. From the top of any page, Tab goes: skip link, then about 70 invisible stops (brand, collapse button, 7 nav items, filter, every stack and container) before the first visible control. The focus ring is off screen the whole time. | A closed drawer is `inert` (or `visibility:hidden`) so Tab goes from the skip link to the top bar. | `static/app.css:149`, `static/app.js:77` | P5 | fixed 7d502a4 |
| A11Y-02 | major | Focus is lost when a live update replaces the element that has it. Overview, 1440 px: focus a container link in "Needs attention", wait for the next update (under 12 s): `document.activeElement` is `body` and the next Tab starts from the top of the page. Same on `/host` (a sort link in a table header) every 3 s. Only elements with an `id` are refocused. Rows in the containers table and the sidebar tree keep focus. | After a fragment swap, focus returns to the matching element (same href / same key), or the focused subtree is not swapped while it has focus. | `static/app.js:725`, `static/app.js:743`, `src/host/view.rs:29` | P4 | fixed 5cb678e |
| A11Y-03 | minor | When the phone drawer is open the page behind is not inert: Tab leaves the drawer into the top bar and page after its last item, and the menu button keeps the name "Open sidebar" while `aria-expanded=true`. The bottom bar's "More" button opens the same drawer but has no `aria-expanded`. Esc closes and returns focus correctly. (Also reported as NAV-32.) | Open drawer traps focus or makes `main` inert; button name follows state; "More" carries `aria-expanded`. | `static/app.js:77`, `src/ui/shell.rs:260` | P5 | fixed 7d502a4 |
| A11Y-04 | minor | The focus ring is cut off on the four health-strip cells on Overview (only the right and bottom edges show), on the memory treemap tiles, and on the name link in the containers table (left edge hidden under the icon cell), because the parent clips overflow and the ring sits 2 px outside. Seen in `shots/focus-hscell.png`, `shots/focus-cname.png`. | Ring fully visible: `outline-offset:-2px` inside clipping parents. | `static/widgets.css:3`, `static/widgets.css:158`, `static/app.css:65` | P4 | fixed 5cb678e |
| A11Y-05 | minor | The sidebar (up to 70 stops on the test host, 320 with 150 containers) comes before the page in the tab order on every page. The skip link is the only way past it and it is the first stop, so this is workable, but the collapsed-rail and filter state do not shorten it. | Keep the skip link; consider collapsing stacks out of the tab order when the rail is collapsed. | `src/ui/shell.rs:189` | P2 | fixed 05a7da8 |
| A11Y-06 | polish | Focus indication is two different things: a 2 px outline with 2 px offset on links and buttons, a 1 px box-shadow plus border colour on text inputs and selects (`.in`, the table filter, login, log search). The 1 px version is much weaker next to the 2 px one. Both pass 3:1 in every theme. | One focus style for every control. | `static/settings.css:67`, `static/cards.css:17`, `static/alerts.css:44`, `static/app.css:65` | P5 | fixed 05a7da8 |

### Names, roles, structure

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| A11Y-08 | minor | State shown by colour (plus a hover title) only: the channel chips on `/alerts` ("failing", "not used yet" are a red or grey dot; the words are in a `title` on a non-focusable `span`, though the failing one also prints "failing"); the level stripe on each log line (`span.lg` with `title="warning (stderr)"`, 239 on one page); incident dots on `/alerts`. Container status dots are fine (they carry `role=img` + `aria-label`, and the state word is printed next to them). | The state as text or as `role=img` + `aria-label`; never only `title`. | `src/ui/alerts.rs:52`, `static/logs.js:363` | P5 | fixed d5f6bfd |
| A11Y-09 | minor | Overview has two `h1`: the page title "Overview" in the top bar and the greeting. Signed out, the settings pages and both logs pages jump from `h1` to `h3`. | One `h1` per page, no skipped level. | `src/ui/pages/overview.rs:69` (the second `h1`; the first is the top-bar title in `src/ui/shell.rs`) | P5 | fixed 4eb67a3 |
| A11Y-10 | minor | Tables without `scope` on their header cells: both tables on `/alerts`, all Settings lists (bookmarks, containers, widgets, notifications, config revisions, sessions) and several on `/host`; two `/host` tables and the Settings containers table have an empty header cell. The containers table is correct (`scope`, caption, `aria-sort`). | `scope=col` / `scope=row` everywhere; a visually hidden name for the empty cells ("Select", "Actions"). | `src/ui/pages/alerts.rs:133`, `src/ui/pages/alerts.rs:164`, `src/ui/settings/containers.rs:80`, `src/host/view.rs:40` | P5 | fixed 4eb67a3 |
| A11Y-11 | polish | The theme button changes its `aria-label` ("Theme: dark. Click to change" to "Theme: light...") but nothing is announced after pressing it. `#pal-q` has `aria-expanded=true` while the palette is closed. | Announce the new theme (`aria-live` or a toast); `aria-expanded` follows the list. | `static/app.js:62`, `src/ui/shell.rs:352` | P5 | fixed 4eb67a3 |

### Contrast (measured, today's themes)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| A11Y-12 | minor | Controls outlined with `--border-2` are under 3:1 against every surface in Dark (1.45 to 1.73), Light (1.41 to 1.63) and Ember (1.53 to 1.81): the two selects in the log toolbar, port chips, secondary buttons. Inputs that use `--ctl` pass (3.2 to 4.2). | Input and select boundaries use `--ctl` (or the brand equivalent at 3:1 or more). | `static/logs.css:22`, `static/cards.css:55`, `static/app.css:14` | P5 | fixed abd90f7 |
| A11Y-13 | minor | Three search inputs fall back to the browser's default placeholder grey `#757575` because only the sidebar filter, palette and `.in` set a `::placeholder` colour: the containers filter, the log search and the Settings container search. 3.87:1 in Dark, 3.89 in Ember, 4.30 in High contrast, 4.22 on `--bg` in Light. | One global `::placeholder{color:var(--text-3);opacity:1}`. | `static/app.css:102`, `static/app.css:260`, `static/settings.css:65` | P5 | fixed abd90f7 |
| A11Y-14 | minor | Log timestamps' milliseconds are `--text-3` at 70 %: 3.41:1 Dark, 2.99 Light, 3.61 Ember (12.5 px text). | 4.5:1, or drop the fade. | `static/logs.css:60` | P5 | fixed d5f6bfd |
| A11Y-15 | minor | Light theme only: the alert count badge in the sidebar and in the phone bottom bar is `#1a0505` on `--bad` `#dc2626` = 4.07:1 at 10 to 11 px bold (the dark text colour is hard-coded). | Badge text from a token (`--on-bad`), 4.5:1 in each theme. | `static/app.css:89` | P5 | fixed abd90f7 |
| A11Y-16 | polish | The letter in a container's fallback avatar is white on `hsl(h 45% 38%)`: 3.5 to 4.3:1 depending on hue, at 9 px bold. Decorative (the name is next to it), so not a blocker. | Darker fill (lightness about 30 %) or dark text on a light fill. | `static/app.css:250`, `static/cards.css:33` | P5 | fixed 4eb67a3 |
| A11Y-17 | major | Brand palette (table below): every text, accent and status colour passes 4.5:1 on Surface and on Raised in dark and light, and text-on-accent passes on both accent fills. Two things do not carry over: **Border** is 2.71:1 on Surface and 2.20 on Raised in dark, 3.04 and 2.66 in light, so it cannot outline an input, select or secondary button on its own; and **Accent and Error have the same luminance** (1.02:1 dark, 1.11 light; Accent vs Warning 1.09 in light), so in greyscale or for red-green colour blindness the orange accent, the crimson error and (light) the warning are one colour. | Add a control-border token at 3:1 or more on both surfaces (dark needs about `#8F6A5C` or lighter, light about `#9A7566` or darker); every status keeps a word or a shape, and the accent is not used as a status colour. | `docs/BRANDING.md:38`, `static/app.css:14` | P5 | fixed 4eb67a3 |

### Hit targets at 390 px

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| A11Y-18 | major | 2,956 of 3,227 interactive elements measured across the 25 pages at 390 px are under 44 x 44; 529 are under 24 x 24 (table below). The ones a thumb meets most: the four top-bar buttons are 32 x 32; row actions in the containers table are 26 x 26, side by side (Open, Logs, Details); Edit and Delete in every Settings list are 26 x 26 and adjacent; segmented controls and `.btn.sm` are 26 high; inputs and buttons 32 high; the on/off switch is 32 x 18. Only the bottom bar (76 x 48) meets 44. (Also reported as OV-31, NAV-36.) | At `(pointer:coarse)` or under 700 px: 44 px for icon buttons, row actions, segments and form controls; destructive buttons not adjacent to others at this size. | `static/app.css:178`, `static/app.css:180`, `static/app.css:213`, `static/app.css:217`, `static/settings.css:61`, `static/settings.css:77` | P5 | fixed 4eb67a3 |
| A11Y-19 | minor | Under the WCAG 2.2 AA floor of 24 x 24 and not inline in a sentence: container name links in the table (15 high), sortable column headers on Processes (15 high), "All" / "Host details" panel links (17 high), port chips (20 high), chart legend toggles avg / max (20 high), breadcrumb links (22 high), the per-stack logs button in the sidebar (22 x 22), row checkboxes in Settings containers (15 x 15), event markers on the charts (11 wide). | 24 x 24 minimum through padding; whole-row targets for list rows. | `static/cards.css:35`, `static/cards.css:55`, `static/app.css:391` | P5 | deferred: fixed on phones and touch (no target under 24 px, 44 px by size since 4eb67a3); at desktop widths sidebar stack links, sort headers, legend toggles and log toolbar toggles remain under 24 px and need their own pass |

### Zoom, reflow, motion

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| A11Y-20 | minor | Every font size is in `px` (root `14px`): setting the root font size to 200 % changed 0 of 246 text elements on Overview (0 of 299 on Containers). Browser page zoom works; a larger default font size set in the browser or OS is ignored. | Type scale in `rem`. | `static/app.css:11`, `static/app.css:56` | P5 | fixed 05a7da8 |
| A11Y-21 | minor | At 320 px wide the containers page scrolls sideways by 20 px and container detail by 68 px (the tab strip). At 390 px no page pans sideways, although `/history` and `/alerts` report a `scrollWidth` of 522 and 822 (see ROB-03 for the cause). | No horizontal scroll at 320 px (WCAG 1.4.10). | `static/cards.css:31`, `static/detail.css:3` | P5 | fixed 1938f93 |
| A11Y-22 | polish | Two JS scrolls use `behavior:"smooth"` unconditionally (after opening an edit form and when showing a revision); the CSS reduced-motion rule does not reach them. | Gate on the same `calm` media query the tween uses. | `static/settings.js:325`, `static/settings.js:1122` | P4 | fixed 5cb678e |

### Robustness: empty host (0 containers)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| ROB-01 | minor | Overview on an empty host: "Memory by container" is a 230 px empty box and "Top CPU" a panel, both saying "Waiting for the first sample…" and still saying it after 40 s; it never resolves because there is nothing to sample. The same text stays forever on a host where per-container stats are not readable. Health strip says "All clear. 0 containers, nothing needs you." | Distinguish "no sample yet" from "nothing to show": hide both panels or say "No containers yet" with a pointer to what Hobnail lists (running Docker containers; Settings for hidden ones). | `src/ui/pages/overview.rs:300`, `src/ui/pages/overview.rs:351`, `src/ui/pages/overview.rs:261` | P4 | fixed 5cb678e |
| ROB-02 | minor | `/alerts` with no rules: "Add `[[alerts]]` rules and `[channels.*]` to the config to get notified." No link, and it sends the person to hand-edit TOML although Settings, Notifications has forms for both. Containers ("No containers. Nothing to show. Hidden containers and excluded names are not listed."), Logs, the sidebar ("No containers.") and the Settings lists have clear empty states. | Empty state with a button to Settings, Notifications. | `src/ui/pages/alerts.rs:60` | P4 | fixed 5cb678e |

### Robustness: 150 containers in 13 stacks

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| ROB-03 | major | Every page grows a blank scroll area below its content: with 150 containers (20 stopped) `/history` is 1,245 px of content but the document is 4,666 px tall, and scrolling down shows an empty dark page (`shots/big-history-bottom.png`). Cause: `.sr` is `position:absolute` with no positioned ancestor, so the visually hidden " (stopped)" text of sidebar rows far down the scrolling tree is laid out at its un-scrolled offset and stretches the document. The same mechanism inflates `scrollWidth` at 390 px on `/history` and `/alerts`. On the shared instance (40 containers) it adds about 50 px. | `.sr` clipped inside its row (`position:relative` on the row, or the modern clip pattern with `inset`/`margin:-1px`). | `static/app.css:70`, `src/ui/shell.rs:530` | P5 | fixed 4eb67a3 |
| ROB-04 | major | No response compression. `/containers` with 150 containers is 434 KB of HTML sent as-is (`content-length: 434010`, no `content-encoding` with `Accept-Encoding: gzip, br`); gzip -9 of the same body is 35.6 KB. Overview is 160 KB, and every page carries about 60 KB of inline CSS before the first byte of body. At 400 kbit/s this is the whole load time (ROB-10). | gzip or brotli for HTML, JS and the SSE fragments (pre-compressed for the static files). | `src/http/server.rs:225` | P6 | deferred: gzip needs a new crate (miniz_oxide), awaiting the owner |
| ROB-05 | major | `/containers` with 150 rows: 6,035 DOM nodes. Desktop, no throttle: DOMContentLoaded 294 ms, one 170 ms long task, scroll p95 53 ms per frame with 3 frames over 50 ms. Phone profile at 4x CPU: DCL 1.27 s, load 1.69 s, a **1,025 ms** long task on load, scrolling at p50 65 ms and p95 203 ms per frame (40 of 60 frames over 50 ms), and each live update is a 90 to 155 ms long task. Overview at 4x CPU: a 50 to 146 ms long task on every update (7 in 16 s). | Under 50 ms per update and no long task on load at 4x: render collapsed stacks lazily or `content-visibility:auto` on row groups, patch only changed cells, defer the per-row health strips. | `src/ui/pages/containers.rs:422`, `static/app.js:722` | P6 | deferred: bench phase stopped before the table change was measured |
| ROB-06 | minor | Overview "Needs attention" lists all 25 problems with no cap and shows the short service name only, so with several stacks it reads "svc05 unhealthy", "svc05 restarting", "svc03 unhealthy", "svc03 stopped" with nothing to tell which stack each belongs to. The palette does show the stack ("svc00 … stack00") but caps at 60 results with no "more" line. | Stack shown next to the name when names repeat; cap the list (say 8) with "17 more" linking to Containers, Problems; palette says when results are cut. | `src/ui/pages/overview.rs:174`, `static/app.js:485` | P5 | fixed 1938f93 |

### Robustness: long and hostile names

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| ROB-07 | major | A 115-character stack name is not truncated in the containers table's group header: at 390 px the page gets 550 px of horizontal scroll and the header runs off screen (`shots/nasty-_containers-390.png`); at 1440 px it pushes the 24 h strip to a sliver. The sidebar truncates the same name with an ellipsis. | Group header truncates with an ellipsis and a `title`, like the sidebar. | `src/ui/pages/containers.rs:427`, `static/cards.css:31` | P5 | fixed 1938f93 |
| ROB-08 | minor | A 120-character container name: Settings, Containers at 1440 px keeps the name on one line and the table overflows the page by 560 px (Display name and Group columns pushed off); container detail, Logs tab at 390 px, the "Open logs of NAME" button overflows by 639 px; on the detail header the name is printed twice (title and subtitle) and fills the first screen on a phone (wraps, nothing lost). Long image names, 30 ports ("+27"), long labels and the table row itself are handled. | `overflow-wrap:anywhere` or ellipsis in the Settings table cell and on the button; do not repeat the name when there is no display-name override. | `static/settings.css:132`, `src/ui/pages/container.rs:285` | P5 | fixed 4eb67a3 |

### Robustness: Docker unreachable

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| ROB-09 | minor | Wording when Docker stops answering: Overview shows the red "Docker unreachable: cannot connect to /var/run/docker.sock: Connection refused (os error 111)" directly above a green tick "Nothing needs attention."; the containers table says "Docker: cannot connect cannot connect to /var/run/docker.sock: …" (the phrase twice); `/host`, `/alerts`, `/history` and Settings say nothing in the page (only the sidebar note, which is in the closed drawer on a phone); the tab title stays "Overview · …" with no problem count; the sidebar footer still says "live". Counts become "—" and the last known list stays, which is right. | One sentence in plain words with what to check ("Hobnail cannot reach Docker. Is the socket mounted?"), the raw error in a details line, no green tick next to it, and the same banner on every page. | `src/ui/pages/overview.rs:166`, `src/ui/pages/overview.rs:262`, `src/ui/pages/containers.rs:390`, `src/ui/shell.rs:501` | P4 | fixed 5cb678e |

### Robustness: slow network (400 kbit/s, 400 ms latency, 4x CPU)

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| ROB-10 | major | First load: a blank screen until first paint at 1.8 s (3 containers) to 2.7 s (150 containers) because the inline CSS has to arrive first; then the page fills in top to bottom as HTML streams. `/containers` with 150 rows is not complete until **9.5 s** (DOMContentLoaded 9.3 s) and the table is not interactive until then. No flash of unstyled content; layout shift is small (CLS 0.02 to 0.05, from the health panel and the top-bar vitals at about 3.9 s). | See ROB-04 (compression takes the 434 KB page to about 36 KB). Reserve the health panel's height. | `src/http/server.rs:225`, `static/widgets.css:133` | P6 | deferred: depends on ROB-04 (compression) |
| ROB-11 | major | Settings forms are rendered empty and filled by a later `GET /api/settings/…`; `settings.js` is `defer`. On the throttled connection the form is visible and focusable about 4 s before it works: text typed into "Greeting" right after the page appeared was wiped when the values arrived, and Save did nothing. Nothing shows that the form is still loading (fields are enabled, no skeleton, no `aria-busy`). | Values rendered by the server into the HTML (the server has them), or fields disabled with a skeleton until loaded; input typed early is never overwritten. | `src/ui/settings/form.rs:207`, `static/settings.js:118`, `src/ui/shell.rs:236` | P1 | fixed 9a8dd71 |
| ROB-12 | minor | No feedback on navigation: a click on a detail tab (a full page load, 2.5 s on this connection) shows nothing until the new page paints; the old page stays as it was, the clicked tab is not marked, there is no progress bar or busy cursor. Once `settings.js` is running, Save does show "Saving…" with the button disabled, then "Saved" and a toast. | A top progress line or a pressed / pending state on the clicked link (View Transitions or a 150 ms delayed bar). | `static/app.js:640`, `static/detail.css:3` | P4 | fixed 46c3b46 |

### Robustness: stream dropping and reconnecting

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| ROB-13 | major | Stream blocked or the server stopped: the pages keep showing the last numbers as if current. Overview turns "updated 20s ago" into "stale · updated …" after 15 s; Containers, container detail, Host, History and Alerts have no "updated" indicator at all, so the only signal is "reconnecting" in the sidebar footer (see A11Y-07; invisible on a phone). The log viewer keeps its lines and its "40 lines" count with no note that the tail stopped. Clicking any link while the server is down gives the browser's own error page, not the offline page. Recovery is good: 1 to 3 s after the stream is reachable again, no reload, session kept. (Also reported as NAV-71, A11Y-07.) | The same freshness indicator on every live page; data dimmed or flagged when stale; the log pane says "disconnected, retrying"; navigation while offline lands on the `/offline` page (it exists, but there is no service worker to serve it, so it never appears here). | `static/app.js:795`, `static/app.js:869`, `static/logs.js:305`, `src/ui/pwa.rs:2` | P4 | fixed 5cb678e |

### Robustness: widgets

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| ROB-14 | minor | A widget that has not answered yet shows "loading…" as plain text for as long as its timeout allows (20 s in the test) with no spinner, elapsed time or last value; a failed one shows the raw transport error ("connect 127.0.0.1:9: Connection refused (os error 111)", "timed out after 5000ms") and "0 ms" as its timing. The page itself never waits. (Also reported as OV-08.) | Skeleton tile while loading; on failure a short plain message ("No answer", "Refused"), the raw error behind a disclosure, and the last good value kept with its age. | `src/widgets/mod.rs:446`, `src/ui/widgets.rs:43`, `src/ui/widgets.rs:16` | P4 | deferred: the widget tile shows a skeleton and an error state since 5cb678e; keeping the last good value needs server-side state, against the idle-RAM budget |

### Robustness: JavaScript blocked

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| ROB-15 | major | Without JS, "Change password" in Settings, Security submits as a plain GET to the same page: the URL becomes `/settings/security?current=…&new=…&repeat=…` with all three passwords in clear, in the address bar, the browser history and any proxy log. The same happens when someone presses Enter before `settings.js` has loaded on a slow connection (ROB-11). Every other Settings form does the same harmless-but-useless GET (`/settings/general?title=…`), reloads empty and says nothing. | Forms carry `method="post"` and a real action (or `onsubmit` is not needed because the submit button is `disabled` until JS is ready); password fields can never end up in a URL; a `noscript` line says Settings needs JavaScript. | `src/ui/settings/security.rs:23`, `src/ui/settings/form.rs:207` | P1 | fixed c38e18b |
| ROB-16 | major | Without JS on a phone, History, Host (apart from one link on Overview) and Settings cannot be reached: the sidebar is a drawer that only JS opens, and the bottom bar's "More" is a `button` with no fallback. On desktop the sidebar is always there, so all pages are reachable. | "More" is a link to a plain page (or the drawer opens with a `details` / `:target` / checkbox fallback). | `src/ui/shell.rs:260`, `static/app.css:149` | P5 | fixed 7d502a4 |
| ROB-17 | minor | Dead controls with no explanation when JS is off: collapse sidebar, keyboard shortcuts, the search / command button, theme, the account menu (so no Sign out and no Settings link from the top bar), stack collapse buttons in the containers table, avg / max legend toggles, "Add bookmark / widget / channel / rule", "Sign out all other sessions", every log toolbar button. Handled well: the containers filter has an "Apply" button, the log page says "The log viewer needs JavaScript. You can still download the last 5,000 lines.", `/logs` side by side works, charts and all read-only pages render, a correct password signs in. A wrong password lands on a bare `{"error":"wrong password"}` page. | JS-only controls carry `.js-only` (the class exists) so they are not shown; the account menu is a `details`; the login form posts to a route that answers with the login page and a message. | `src/ui/shell.rs:189`, `src/ui/pages/login.rs:44`, `static/app.css:72` | P5 | deferred: script-only controls are hidden without script since 4eb67a3; a wrong password without script still lands on bare JSON, and that fix is in src/auth.rs, which this pass must not change |

### Robustness: login back-off

| ID | Severity | What happens | What should happen | Where | Priority | Status |
|---|---|---|---|---|---|---|
| ROB-18 | major | Wrong password, then again: the first answer is 401 "wrong password", the second 429 "too many failed attempts" with `Retry-After: 1` (then 2 s, 4 s …). The form prints only those words: no wait time, although the server sends it in the header and as `retry_after` in the body; the button stays enabled, so pressing it again during the wait earns another 429; the field keeps the wrong password, unselected; `aria-invalid` is never set. Attempts alternate 401 / 429, so every second honest retry is rejected unexplained. The message is in a `role=alert` region and is announced. | "Too many attempts. Try again in 4 s." with a countdown, the button disabled until then, the field selected, `aria-invalid=true`; sentences start with a capital like the rest of the UI. | `static/app.js:672`, `src/auth.rs:663`, `src/ui/pages/login.rs:48` | P4 | fixed 5cb678e |

## Duplicates merged

| Dropped | Kept as |
|---|---|
| NAV-01 | OV-01 |
| NAV-02 | OV-01 |
| NAV-03 | OV-01 |
| NAV-04 | OV-04 |
| NAV-08 | OV-33 |
| LG-02 | OV-33 |
| NAV-20 | LG-01 |
| NAV-06 | LG-09 |
| NAV-09 | LG-55 |
| NAV-16 | LG-03 |
| NAV-10 | LG-04 |
| NAV-30 | LG-58 |
| NAV-14 | LG-47 |
| NAV-71 | ROB-13 |
| A11Y-07 | ROB-13 |
| NAV-13 | SET-52 |
| NAV-12 | SET-22 |
| LG-49 | OV-05 |
| LG-53 | OV-07 |
| NAV-32 | A11Y-03 |
| NAV-42 | SET-27 |
| OV-31 | A11Y-18 |
| NAV-36 | A11Y-18 |
| SET-60 | NAV-41 |
| OV-08 | ROB-14 |

## Controls nobody would miss

### In the chrome and navigation

| Control | Where it is | Why |
|---|---|---|
| "Open in Dozzle" icon in every Containers row | `src/ui/pages/containers.rs:523` | Logs are in the app; the icon sits beside the in-app logs icon |
| "Open in Dozzle: X" palette rows | `static/app.js:512` | Same; they double every container hit |
| "Details" chevron at the end of every Containers row | `src/ui/pages/containers.rs:525` | The name in the same row is already that link |
| "Beszel" button on the detail header | `src/ui/pages/container.rs:164` | Already on the owner's list |
| Hamburger on phones | `src/ui/shell.rs:200` | "More" in the bottom bar opens the same drawer |
| "Settings" in the account menu | `src/ui/shell.rs:323` | Already in the sidebar and the drawer |
| The account menu itself when signed out | `src/ui/shell.rs:326` | One item; could be the link |
| The Settings index of cards | `src/ui/settings/mod.rs:140` | Repeats the section list beside it and costs a click on every Settings task |
| "Theme" select in Settings General | `src/ui/settings/general.rs:20` | Same setting as "Default theme" in Appearance |
| "Toggle sidebar" palette action | `static/app.js:487` | The toggle is always on screen |
| `/offline` page | `src/ui/pwa.rs:109` | Cannot be reached when it would matter (NAV-70) |
| The second "Logs" on the detail page (tab or header button) | `src/ui/pages/container.rs:163` | Two views of the same thing (NAV-08) |

### On Overview, Containers and container detail

- Beszel header button (OV-35)
- "Open in Dozzle" and "Details" row icons (OV-20)
- The Logs tab as an intermediate screen (OV-33)
- The "… on the Metrics tab" link line (OV-48)
- Always-zero charts (OV-50)
- The compose label dump (OV-44)
- Event filters on an empty list (OV-62)
- The memory sparkline column (OV-27)

## Same job, different pattern

| Job | Patterns seen |
|---|---|
| Open a container's logs | Row icon (Containers table); header button (detail); tab (detail, different view); hover-only icon (tree, stacks only); "Logs: X" row (palette); checkbox plus submit (`/logs`); `l` key; nothing (Overview, Alerts, tree containers) |
| Click a container's name | Detail page everywhere, except the palette, which opens the container's own web page in a new tab |
| Click a stack's name | Filtered Containers table (Overview "Last 24 h", detail header); fold the stack (sidebar tree) |
| Go to the host | Top-bar chips go to History; Overview "Host details" goes to Host |
| Filter a list | "Filter by name, stack or image" plus state buttons (Containers); "Filter containers" (sidebar, cleared on navigation); "Search name, project, image" (Settings containers); none on the 40-checkbox logs picker; `/` focuses a different one per page |
| Ask for sign-in | Account menu; inline text link; sentence with link; 401 empty state with a button |
| Close an overlay | Palette: Esc or outside, no button. Cheat sheet: X, Esc or outside. Drawer: backdrop, Esc, or a "Collapse sidebar" button. Account menu: Esc or outside |
| Edit a container's display | "Customize" (detail header), "Customize" or "Edit" (Settings list, depending on whether an override exists) |
| Show when something happened | "firing 9m", "last hour", "7m ago", "9m"; absolute time only in hover titles |
| Mark where you are | Sidebar and bottom bar disagree with the page on logs pages; bottom bar marks nothing for drawer pages |
| Set the theme | Cycle button; palette action (one step); two Settings selects |

## Measurements: contrast today: theme tokens (`static/app.css:14-45`), WCAG ratio

Text needs 4.5:1; UI components, large text and focus rings need 3:1. `--bg` is the page, `--bg-side` the sidebar, `--surface` panels, `--surface-2` hovered rows / raised chips.


**Dark**

| Token | on `--bg` #0b0e13 | on `--bg-side` #0d1117 | on `--surface` #121820 | on `--surface-2` #19212b |
|---|---|---|---|---|
| `--text (body)` #e8edf4 | 16.43 | 16.09 | 15.16 | 13.80 |
| `--text-2 (secondary)` #a9b3c1 | 9.12 | 8.93 | 8.41 | 7.66 |
| `--text-3 (muted, labels, placeholders)` #8792a2 | 6.13 | 6.00 | 5.66 | 5.15 |
| `--accent-fg (links)` #2dd4bf | 10.38 | 10.17 | 9.58 | 8.72 |
| `--ok-fg (status text)` #34d399 | 10.05 | 9.84 | 9.28 | 8.44 |
| `--warn-fg (status text)` #fbbf24 | 11.58 | 11.34 | 10.69 | 9.72 |
| `--bad-fg (status text)` #fca5a5 | 10.18 | 9.97 | 9.40 | 8.55 |
| `--accent (focus ring, active marker; 3:1)` #2dd4bf | 10.38 | 10.17 | 9.58 | 8.72 |
| `--ok (dot; 3:1)` #34d399 | 10.05 | 9.84 | 9.28 | 8.44 |
| `--warn (dot; 3:1)` #fbbf24 | 11.58 | 11.34 | 10.69 | 9.72 |
| `--bad (dot; 3:1)` #f87171 | 6.99 | 6.84 | 6.45 | 5.87 |
| `--off (stopped dot; 3:1)` #64748b | 4.06 | 3.98 | 3.75 | 3.41 |
| `--ctl (input border; 3:1)` #6b7686 | 4.20 | 4.11 | 3.87 | 3.53 |
| `--border-2 (select / button border; 3:1)` #323c4a | 1.73 **FAIL** | 1.69 **FAIL** | 1.60 **FAIL** | 1.45 **FAIL** |
| `--border (panel border, not required)` #222b36 | 1.35 | 1.32 | 1.25 | 1.13 |
| `--on-accent` #03201c on `--accent-bg` #14b8a6 (primary button) | 6.87 | | | |
| nav badge text `#1a0505` on `--bad` #f87171 (`static/app.css:89`) | 7.11 | | | |
| log milliseconds: `--text-3` at 70 % on `--surface` = #646d7b (`static/logs.css:60`) | | | 3.41 **FAIL** | |
| browser-default placeholder `#757575` (inputs without a `::placeholder` rule) | 4.20 **FAIL** | | 3.87 **FAIL** | |
| avatar letter `#fff` on `hsl(h 45% 38%)` (`static/app.css:250`), worst hue measured #8b8d35 | 3.52 **FAIL** | | | |

**Light**

| Token | on `--bg` #f4f5f7 | on `--bg-side` #eceef2 | on `--surface` #ffffff | on `--surface-2` #eff1f4 |
|---|---|---|---|---|
| `--text (body)` #121720 | 16.47 | 15.46 | 17.96 | 15.87 |
| `--text-2 (secondary)` #465163 | 7.35 | 6.91 | 8.02 | 7.09 |
| `--text-3 (muted, labels, placeholders)` #5d6979 | 5.12 | 4.81 | 5.58 | 4.93 |
| `--accent-fg (links)` #0f766e | 5.02 | 4.71 | 5.47 | 4.84 |
| `--ok-fg (status text)` #047857 | 5.03 | 4.72 | 5.48 | 4.85 |
| `--warn-fg (status text)` #a14a06 | 5.50 | 5.17 | 6.00 | 5.31 |
| `--bad-fg (status text)` #b91c1c | 5.93 | 5.57 | 6.47 | 5.72 |
| `--accent (focus ring, active marker; 3:1)` #0d9488 | 3.43 | 3.22 | 3.74 | 3.31 |
| `--ok (dot; 3:1)` #059669 | 3.45 | 3.24 | 3.77 | 3.33 |
| `--warn (dot; 3:1)` #b45309 | 4.60 | 4.32 | 5.02 | 4.44 |
| `--bad (dot; 3:1)` #dc2626 | 4.43 | 4.16 | 4.83 | 4.27 |
| `--off (stopped dot; 3:1)` #7a8597 | 3.42 | 3.21 | 3.73 | 3.30 |
| `--ctl (input border; 3:1)` #7b8595 | 3.42 | 3.21 | 3.73 | 3.30 |
| `--border-2 (select / button border; 3:1)` #c4cbd6 | 1.50 **FAIL** | 1.41 **FAIL** | 1.63 **FAIL** | 1.44 **FAIL** |
| `--border (panel border, not required)` #dde1e8 | 1.20 | 1.13 | 1.31 | 1.16 |
| `--on-accent` #ffffff on `--accent-bg` #0f766e (primary button) | 5.47 | | | |
| nav badge text `#1a0505` on `--bad` #dc2626 (`static/app.css:89`) | 4.07 **FAIL** | | | |
| log milliseconds: `--text-3` at 70 % on `--surface` = #8e96a1 (`static/logs.css:60`) | | | 2.99 **FAIL** | |
| browser-default placeholder `#757575` (inputs without a `::placeholder` rule) | 4.22 **FAIL** | | 4.61 | |
| avatar letter `#fff` on `hsl(h 45% 38%)` (`static/app.css:250`), worst hue measured #8b8d35 | 3.52 **FAIL** | | | |

**Ember**

| Token | on `--bg` #120e0b | on `--bg-side` #16110d | on `--surface` #1c1611 | on `--surface-2` #271f18 |
|---|---|---|---|---|
| `--text (body)` #f5ebe0 | 16.32 | 15.93 | 15.22 | 13.77 |
| `--text-2 (secondary)` #cdbba8 | 10.31 | 10.06 | 9.61 | 8.70 |
| `--text-3 (muted, labels, placeholders)` #a8927d | 6.47 | 6.31 | 6.03 | 5.46 |
| `--accent-fg (links)` #fdba74 | 11.39 | 11.12 | 10.62 | 9.61 |
| `--ok-fg (status text)` #bef264 | 14.70 | 14.35 | 13.71 | 12.41 |
| `--warn-fg (status text)` #fde047 | 14.57 | 14.22 | 13.59 | 12.30 |
| `--bad-fg (status text)` #fca5a5 | 10.12 | 9.88 | 9.44 | 8.54 |
| `--accent (focus ring, active marker; 3:1)` #fb923c | 8.49 | 8.28 | 7.92 | 7.16 |
| `--ok (dot; 3:1)` #a3e635 | 12.74 | 12.43 | 11.88 | 10.75 |
| `--warn (dot; 3:1)` #facc15 | 12.54 | 12.24 | 11.70 | 10.59 |
| `--bad (dot; 3:1)` #f87171 | 6.94 | 6.78 | 6.48 | 5.86 |
| `--off (stopped dot; 3:1)` #78685a | 3.59 | 3.51 | 3.35 | 3.03 |
| `--ctl (input border; 3:1)` #85705c | 4.09 | 3.99 | 3.81 | 3.45 |
| `--border-2 (select / button border; 3:1)` #4a3c2f | 1.81 **FAIL** | 1.77 **FAIL** | 1.69 **FAIL** | 1.53 **FAIL** |
| `--border (panel border, not required)` #33291f | 1.35 | 1.32 | 1.26 | 1.14 |
| `--on-accent` #1f0d02 on `--accent-bg` #f97316 (primary button) | 6.71 | | | |
| nav badge text `#1a0505` on `--bad` #f87171 (`static/app.css:89`) | 7.11 | | | |
| log milliseconds: `--text-3` at 70 % on `--surface` = #7e6d5d (`static/logs.css:60`) | | | 3.61 **FAIL** | |
| browser-default placeholder `#757575` (inputs without a `::placeholder` rule) | 4.17 **FAIL** | | 3.89 **FAIL** | |
| avatar letter `#fff` on `hsl(h 45% 38%)` (`static/app.css:250`), worst hue measured #8b8d35 | 3.52 **FAIL** | | | |

**High contrast**

| Token | on `--bg` #000000 | on `--bg-side` #000000 | on `--surface` #0a0a0a | on `--surface-2` #1f1f1f |
|---|---|---|---|---|
| `--text (body)` #ffffff | 21.00 | 21.00 | 19.80 | 16.48 |
| `--text-2 (secondary)` #f0f0f0 | 18.43 | 18.43 | 17.37 | 14.46 |
| `--text-3 (muted, labels, placeholders)` #d4d4d4 | 14.17 | 14.17 | 13.36 | 11.12 |
| `--accent-fg (links)` #7ff0de | 15.42 | 15.42 | 14.54 | 12.10 |
| `--ok-fg (status text)` #86efac | 14.96 | 14.96 | 14.10 | 11.74 |
| `--warn-fg (status text)` #fef08a | 18.05 | 18.05 | 17.01 | 14.16 |
| `--bad-fg (status text)` #ffb3b3 | 12.35 | 12.35 | 11.64 | 9.69 |
| `--accent (focus ring, active marker; 3:1)` #5eead4 | 14.20 | 14.20 | 13.38 | 11.14 |
| `--ok (dot; 3:1)` #4ade80 | 12.05 | 12.05 | 11.36 | 9.46 |
| `--warn (dot; 3:1)` #fde047 | 15.93 | 15.93 | 15.02 | 12.50 |
| `--bad (dot; 3:1)` #ff8a8a | 9.25 | 9.25 | 8.72 | 7.26 |
| `--off (stopped dot; 3:1)` #a3a3a3 | 8.33 | 8.33 | 7.85 | 6.53 |
| `--ctl (input border; 3:1)` #bdbdbd | 11.18 | 11.18 | 10.54 | 8.77 |
| `--border-2 (select / button border; 3:1)` #bdbdbd | 11.18 | 11.18 | 10.54 | 8.77 |
| `--border (panel border, not required)` #8c8c8c | 6.25 | 6.25 | 5.89 | 4.90 |
| `--on-accent` #000000 on `--accent-bg` #5eead4 (primary button) | 14.20 | | | |
| nav badge text `#1a0505` on `--bad` #ff8a8a (`static/app.css:89`) | 8.67 | | | |
| log milliseconds: `--text-3` at 70 % on `--surface` = #979797 (`static/logs.css:60`) | | | 6.78 | |
| browser-default placeholder `#757575` (inputs without a `::placeholder` rule) | 4.56 | | 4.30 **FAIL** | |
| avatar letter `#fff` on `hsl(h 45% 38%)` (`static/app.css:250`), worst hue measured #8b8d35 | 3.52 **FAIL** | | | |

## Measurements: contrast of the brand palette (`docs/BRANDING.md:38-50`), WCAG ratio

Each colour as text (needs 4.5:1) and as a UI component / focus ring / large text (needs 3:1) on Surface and on Raised surface. **FAIL** marks under 4.5; *(<3)* marks under 3.


**Brand Dark**

| Colour | on Surface #1D1210 | on Raised #35231D | Verdict |
|---|---|---|---|
| Text #FFF0E5 | 16.44 | 13.37 | ok as text on both |
| Muted text #CDB4A8 | 9.33 | 7.58 | ok as text on both |
| Accent #F77848 | 6.75 | 5.49 | ok as text on both |
| Accent, hover #FF956B | 8.52 | 6.93 | ok as text on both |
| OK #83CCA7 | 9.74 | 7.92 | ok as text on both |
| Warning #F4D05E | 12.24 | 9.95 | ok as text on both |
| Error #FF678B | 6.60 | 5.37 | ok as text on both |
| Border #795245 | 2.71 **FAIL** *(<3)* | 2.20 **FAIL** *(<3)* | under 3:1 on Surface: not enough for an input border; **under 3:1 on Raised**: an input or button outlined with Border on a raised panel does not meet 1.4.11 |
| Text on accent #26120B on Accent #F77848 | 6.59 | | ok |
| Text on accent #26120B on Accent hover #FF956B | 8.32 | | ok |
| Accent fill vs Surface (button edge, 3:1) | 6.75 | 5.49 | |
| Accent hover fill vs Surface | 8.52 | 6.93 | |
| Raised vs Surface (panel edge without a border) | 1.23 | | panels are told apart by the border only |
| Accent vs Error (hue pair, for colour-only status) | 1.02 | | luminance nearly equal: never rely on colour alone |
| Accent vs Warning | 1.81 | | |

**Brand Light**

| Colour | on Surface #FFF8F3 | on Raised #F4E8E0 | Verdict |
|---|---|---|---|
| Text #2B1912 | 15.97 | 13.97 | ok as text on both |
| Muted text #725448 | 6.49 | 5.68 | ok as text on both |
| Accent #AC3712 | 6.04 | 5.28 | ok as text on both |
| Accent, hover #922B0A | 7.77 | 6.79 | ok as text on both |
| OK #216648 | 6.53 | 5.71 | ok as text on both |
| Warning #806000 | 5.56 | 4.86 | ok as text on both |
| Error #AD1747 | 6.68 | 5.84 | ok as text on both |
| Border #AD8877 | 3.04 **FAIL** | 2.66 **FAIL** *(<3)* | passes 3:1 as an input border on Surface; **under 3:1 on Raised**: an input or button outlined with Border on a raised panel does not meet 1.4.11 |
| Text on accent #FFFFFF on Accent #AC3712 | 6.35 | | ok |
| Text on accent #FFFFFF on Accent hover #922B0A | 8.16 | | ok |
| Accent fill vs Surface (button edge, 3:1) | 6.04 | 5.28 | |
| Accent hover fill vs Surface | 7.77 | 6.79 | |
| Raised vs Surface (panel edge without a border) | 1.14 | | panels are told apart by the border only |
| Accent vs Error (hue pair, for colour-only status) | 1.11 | | luminance nearly equal: never rely on colour alone |
| Accent vs Warning | 1.09 | | |

## Measurements: hit targets at 390 px (signed in, 25 pages)

3,227 interactive elements measured; **2,956 under 44 x 44**, **529 under 24 x 24**. Grouped by kind; size is width x height in CSS px ("wide" = wider than 200). Inline text links inside a sentence are exempt from the 24 px rule and left out.

| Control | Size | Under 24 | Where it appears |
|---|---|---|---|
| Event markers on charts | 11 x 90 to 146 | yes (width) | Overview, History |
| Container name link in the table | wide x 15 | yes | Containers |
| Sortable column header links | 90 x 15 | yes | Detail, Processes |
| Row checkbox | 15 x 15 | yes | Settings, Containers |
| Needs-attention and stack list links | wide x 15 | yes | Overview |
| Panel "All" / "Host details" / "All events" links | 32 to 88 x 17 | yes | Overview, detail |
| "Show N bridges" disclosure | wide x 17 | yes | Host |
| "Use the default" link-button | 123 x 17 | yes | Settings, Appearance |
| On/off switch | 32 x 18 | yes | Settings: General, Widgets, History |
| Smallest treemap tile | 18 x 94 | yes (width) | Overview |
| Event list name link | 90 x 18 | yes | History |
| "sign out" link-button | 52 x 19 | yes | Login (signed in) |
| Port chip | 43 to 50 x 20 | yes | Containers |
| Chart legend toggle (avg / max) | 47 to 51 x 20 | yes | History |
| Breadcrumb link | 62 to 91 x 20 to 22 | yes | Detail, Settings, Logs |
| Per-stack logs button (sidebar) | 22 x 22 | yes | drawer, every page |
| Log level chips | 76 to 86 x 24 | no | Log viewer |
| Match case / regex toggles | 28 x 24 | no | Log viewer |
| Checkbox with label | 148 to 189 x 24 | no | Settings |
| Sidebar container row | wide x 26 | no | drawer, every page |
| Row action icon (Open, Logs, Details) | 26 x 26 | no | Containers |
| Row action icon (Edit, Delete, Move up / down, Remove) | 26 x 26 | no | Settings lists |
| Small button (Add, Add bookmark, Reveal values, Customize, Edit, Diff, Wrap, Time, Pause, Download) | 41 to 130 x 26 | no | Settings, detail, log viewer |
| Segmented control item (1h, 6h, Stacks, Flat, All, Running …) | 36 to 54 x 26 | no | Containers, detail, Host, History |
| Selects in the log toolbar | 100 to 159 x 26 | no | Log viewer |
| Sidebar stack header | wide x 28 | no | drawer |
| Side-by-side container checkbox row | wide x 29 | no | Logs |
| Stack group header button | wide x 30 | no | Containers |
| Log search, sidebar filter, Settings section links | wide x 30 | no | Log viewer, drawer, Settings |
| Top bar: menu, search, theme, account | 32 x 32 | no | every page |
| Sidebar nav item, collapse button, brand | wide or 32 x 32 | no | drawer |
| Text inputs and selects (`.in`), table filter, stack select | wide x 32 | no | Settings, Containers |
| Buttons (Save, Cancel, Open, Logs, Customize, Sign in, Show logs) | 70 to 198 x 32 to 36 | no | everywhere |
| Skip link | 126 x 32 | no | every page |
| Bookmark tile | wide x 34 | no | Overview |
| JSON log line disclosure | wide x 36 | no | Log viewer |
| Password field | wide x 38 | no | Login |
| Detail tabs | 71 to 86 x 40 | no | Container detail |
| Treemap tiles | 65 to 89 x 31 to 41 | no | Overview |
| **Bottom bar items** | **76 x 48** | **meets 44** | every page |


## Measurements: load with 150 containers (fake Docker, 13 stacks, 20 stopped, 12 unhealthy, 5 restarting, one event every 5 s)

| Page | Profile | HTML (uncompressed = on the wire) | DOM nodes | DOMContentLoaded | Load | FCP / LCP | Long tasks on load | Live updates in 16 s | Scroll frame p50 / p95 / max |
|---|---|---|---|---|---|---|---|---|---|
| `/` | 1440, 1x CPU | 160 KB | 1,611 | 62 ms | 64 ms | 100 ms | none | 325 mutations, one 144 ms task | 16.6 / 25.7 / 36.6 ms |
| `/containers` | 1440, 1x | 434 KB | 6,035 | 294 ms | 383 ms | 208 ms | 170 ms | 101 mutations, one 105 ms task | 19.4 / 53.2 / 133.6 ms |
| `/containers?view=flat` | 1440, 1x | 434 KB | 6,036 | 535 ms | 596 ms | 408 ms | 126 ms | 101 mutations, one 96 ms task | 21.8 / 64.2 / 67.1 ms |
| `/logs` | 1440, 1x | 157 KB | 1,936 | 81 ms | 81 ms | 140 ms | 63 ms | 98 mutations | 16.7 / 32.4 / 52.4 ms |
| `/history` | 1440, 1x | 151 KB | 1,418 | 87 ms | 90 ms | 148 ms | none | 114 mutations | 16.8 / 20.8 / 31.1 ms |
| `/` | 390, 4x CPU | 162 KB | 1,611 | 760 ms | 767 ms | 644 ms | 81 ms | 239 mutations, 7 tasks of 51 to 146 ms | 23.1 / 110.9 / 563.6 ms |
| `/containers` | 390, 4x CPU | 434 KB | 6,036 | 1,272 ms | 1,693 ms | 628 ms | **1,025 ms**, 155 ms | 111 mutations, tasks of 155 and 90 ms | **65.3 / 202.6 / 386.2 ms** |
| `/` | 390, 4x CPU, 400 kbit/s + 400 ms | 170 KB | 1,611 | 3.86 s | 3.87 s | 2.73 s / 2.85 s | | | |
| `/containers` | 390, 4x CPU, 400 kbit/s + 400 ms | 434 KB | 6,036 | **9.30 s** | 9.47 s | 2.72 s / 2.85 s | | | |

`app.js` is one more request of 45 KB (cached by build hash). The sidebar tree holds all 150 rows on every page (1,400 to 1,900 nodes on pages that show nothing else about containers). The palette opened with 60 results and filtered in 46 ms.

## Spot checks

Re-run by hand (a second script, not the agents') before the list was accepted:

| Finding | Result |
|---|---|
| LG-12 log tail | Confirmed: pane rests 5,859 px (1440) and 13,445 px (390) above the bottom after load |
| OV-33 Logs tab | Confirmed: the tab holds a sentence and a button, no log lines. (One agent described the tab as an embedded log panel; that was wrong and the row is merged here) |
| LG-09 stack logs for a config group | Confirmed: `/stacks/monitoring/logs` answers 404 |
| LG-55 Alerts page | Confirmed: 0 links and 0 buttons in `main` |
| NAV-21 sidebar at 900 px | Confirmed: `main` is 56 px wide after the toggle |
| OV-40 detail at 390 px | Confirmed: document 455 px wide |
| LG-58 Alerts at 390 px | Confirmed: document 822 px wide |
| ROB-15 password form | Confirmed: the form has no `method` and no `action` |
| NAV-50 palette | Confirmed: container rows rank above "Logs: X" |
| ROB-04 compression | Confirmed: no `content-encoding` with `Accept-Encoding: gzip, br` |
| ROB-03 blank scroll area | Not reproduced with 40 containers (document and content both 1,786 px); reported with 150 and about 50 px with 40. Kept, to be re-checked on the fix |

## Not checked

### Settings and forms

- **Change password (success path)** and **Sign out other sessions**: not run, by instruction. Their validation and the wrong-current-password answer were seen (one wrong password only).
- **Clear history**: opened, typed the phrase, saw the button enable, cancelled. Whether Enter in the field submits, and what the result looks like, was not exercised.
- **Whether the bad values that were accepted do harm elsewhere** (a `javascript:` bookmark on the Overview, a bad search URL in the palette, an unknown icon on a card): not followed up; those pages belong to other agents, and the values were removed.
- **Bulk Hide / Show / Remove overrides**: only "Set group" was run (on one container), so as not to hide containers under the other agents.
- **Renaming a widget that an alert rule refers to**, and editing a widget or channel that has secret headers (read-only URL): the seed has neither a secret-bearing widget nor a bound channel.
- **Secret-file paths and secret binding guards**: not probed, by instruction. Only the command-widget refusal was looked at.
- **`[http] allow_hosts` refusal in Fetch now**: not triggered.
- **Real Slack and Telegram**: only the mock receiver.
- **Flows at 390 px and in light theme**: every editor was opened, measured and screenshotted there, and the empty-submit error was seen; the full add / edit / delete / stale sequences were run at 1440 dark only.
- **Contrast, focus rings, screen-reader output, slow network**: other agents. Tap-target sizes and input font size were measured because they decide whether a form can be filled in on a phone.
- **iOS Safari zoom on focus** (SET-14) is stated from the measured 12.5 / 13 px font size; no iOS device was used.
- **High contrast and Ember themes** on the forms: not looked at.
- **Lock panel before any password is set** (the "run this on the host" variant with Copy): the instance has a password, so only the "Sign in" variant was seen.
- **Columns / Layout / "on cards" switches** (SET-26): whether they still change anything was not tested.
- **200 revisions**: seen with 62 to 76.

### Navigation

- **An unhealthy container.** None existed (0 unhealthy, 0 stopped throughout) and the rules forbid creating one. The "why unhealthy" path was walked with a healthy container that has a health check; what an unhealthy row in "Needs attention" says was not seen.
- **Display-mode standalone.** The CDP media-feature override did not take effect (`matchMedia("(display-mode: standalone)")` stayed false), so the installed view was judged from the manifest, the absence of any `display-mode` rule in the CSS, and the pages at 390 px. No real install on Android or iOS.
- **Real phones.** Everything at 390 px is Chromium touch emulation. Not seen: iOS Safari, the home-indicator area (the viewport meta has no `viewport-fit=cover`, so the bottom bar's safe-area padding is probably 0 on an iPhone; unverified), the on-screen keyboard over the palette.
- **Server really unreachable.** The container was not stopped. Navigation was tested with the browser offline; the dead stream was simulated by aborting `/api/stream`. Playwright's offline switch did not cut the already-open stream.
- **Light, Ember and High contrast** for the chrome: only the theme cycle itself was exercised; flows ran in dark.
- **macOS.** The `⌘K` label swap and Cmd+K were not exercised.
- **Rail tooltips** are native `title` attributes; headless screenshots do not render them.
- **Saving anything.** Every Settings path stops with the form open; the save click is counted, not pressed. "Send test" was not pressed. Whether a widget saves with no field added was not verified.
- **Wrong password, rate limit, session expiry**: not tried.
- **Cause of the self-reloads (NAV-16)** was not isolated between "config changed" and "stream lagged"; the instance was shared and busy.
- **Widths between 390 and 699** other than 699, and landscape phones.
- Out of scope and left to the other agents: page content, the Settings forms themselves, contrast and focus-order measurements, robustness.

### Overview, containers, detail

- **Stopped, unhealthy, restarting or dead containers.** Every container on the host was running, so the "Problems" filter with rows, a stopped row, a stopped container's detail page and container-type "Needs attention" rows were not seen.
- **Timeline and Events with content.** No container events existed, so the timeline, container chart markers and Events filters were seen only empty.
- **"Since you last looked"** on Overview never appeared; I don't know whether that is a bug or no change.
- **Containers with more than three ports** (the "+N" chip) and the `local` port chip: none on the host.
- **Processes and Inspect with many rows:** the containers looked at had one process and at most four mounts.
- **Without JavaScript:** the toolbar's "Apply" button path was not exercised.
- **900 px (icon-rail) width, Ember and High-contrast themes.**
- **Signed-in at 390 px in light** was not looked at; one batch labelled "in" was actually signed out because the login helper returned before the redirect. Signed-in detail tabs were re-shot at 1440 dark, and at 390 dark for a second container (Overview, Inspect, Processes, Events).
- **Interaction tests ran in dark only;** light was checked by screenshot (Overview and the table in full, container detail on the Overview tab only).
- **Metrics-tab chart hover:** my first attempt missed the chart, so hover was confirmed on Overview and the detail Overview tab only.
- **The "Open" header button, bookmark links and port links** were not followed; their targets are other services on the host.
- **Drag-to-zoom, legend toggles and the "yesterday" line** on these pages.
- **`OV-26`:** the suggested fix assumes a usable image name is available elsewhere; I did not check where.

### Logs, Host, History, Alerts

- **A stopped container's logs** (the "stopped" banner and last lines): every container was running, and I was not to stop any.
- **The "Log alerts are not watching N containers" banner** and the level chart in the Logs tab with data: no `log_match` rule or `[logs] count` in the seed, and I did not edit the config.
- **Docker unreachable** and the stream dropping or reconnecting: belongs to the robustness agent.
- **7 d and 30 d ranges, the "yesterday" line, min/max bands and "Container starts per day" with real data**: the database was under an hour old. The controls were exercised; what they draw with a month of data was not seen.
- **Touch gestures on charts** were not driven; LG-46 rests on the handler accepting mouse pointers only (`static/app.js:248`) and the hint still showing at 390 px.
- **Ember and High contrast themes** on these pages, including ANSI colours in logs under them.
- **ANSI colour rendering and truncated (16 KiB) lines**: no container produced them during the session.
- **Clipboard in a real browser profile over plain HTTP**: Copy / Link worked in headless Chromium with clipboard permission granted; the fallback path was not exercised.
- **Saving a download to disk**: the URL was fetched (200, `attachment; filename="hobnail-dev-mock-<time>.log"`), the browser's save flow was not.
- **Logs with JavaScript blocked**: the `noscript` download link was not opened.
- **Safari and Firefox**: Chromium only. LG-12 depends on `content-visibility`, so it may behave differently elsewhere.
- **Sensors in high or critical state, PSI unavailable, no hwmon**: only this machine's state was seen.

### Accessibility and robustness

- **A real screen reader.** Names, roles and live regions were read from Chromium's accessibility tree; nothing was listened to with NVDA, VoiceOver or TalkBack, so announcement order and verbosity are unverified.
- **Safari and Firefox.** Everything is Chromium. Focus-visible heuristics, `color-mix`, native checkbox / select rendering and iOS text-size adjustment differ there. The installed-app (standalone) view was not opened.
- **Contrast of native controls and charts.** Native checkboxes, select arrows and scrollbars cannot be measured from computed styles and were left out; chart series colours (`--ch-*`), the health-strip cells and the treemap were not measured against 3:1. Text over the gradient health panel was measured against the flat colour underneath.
- **Contrast at 390 px in themes other than Dark signed out.** All four themes were measured signed in at both widths; signed out only Dark.
- **Keyboard walk of every Settings form's inner controls** (edit forms opened, validation errors, delete confirmations): Settings behaviour belongs to another agent, and nothing was saved on the shared instance. Tab order was walked on General, Bookmarks, Notifications and Containers without opening editors.
- **Focus after a live update on every fragment.** Tested: containers table row, sidebar row, table group header (kept); Overview "Needs attention" link and a `/host` table header link (lost). Charts, widgets and the alert list were not tested one by one.
- **A slow widget that eventually succeeds.** The host firewall drops traffic from containers to a listener on the Docker bridge, so the 10 s endpoint was never reached; what was seen is a widget pending until its timeout (5 s and 20 s), one refused and the page not waiting. A late success replacing "loading…" was not observed.
- **Real per-container CPU and memory under load.** The fake Docker serves containers with no cgroups, so those columns were "—"; the cost of live updates with 150 changing CPU / memory cells and sparklines is therefore understated.
- **Duplicate log lines after a reconnect.** After the instance restarted the viewer showed 80 lines instead of 40, but the fake log endpoint ignores `since`, so this may be the fake's doing. Needs a check against a real container.
- **Docker socket missing at start-up** (as opposed to refusing connections) was seen once by accident with the same message; a permission-denied socket and a Docker that hangs without answering were not tried.
- **Stream drop on Host, History and the log viewer in detail**: Overview, Containers and one log page were watched; the others were only checked for the absence of a freshness indicator.
- **Login back-off beyond five attempts** (how long the wait grows, when it resets) and the lock-out message on a phone keyboard.
- **Text spacing overrides (WCAG 1.4.12), forced-colors / Windows high-contrast mode, and 400 % zoom.**

## React port: outcomes of the deferred findings

These are implementation outcomes, not a blanket closure of the historical
findings. Current measured passes and misses are in the
[Phase 2–5 results](../plans/react-port-phase-2-5.md).

| Finding | Current outcome |
|---|---|
| SET-42 | Board documents use private drafts and explicit Save/Cancel. Legacy TOML collections stage arrow moves and submit the final order once on **Save order**. The Settings browser check asserts three moves write zero revisions, Cancel writes zero, and Save writes exactly one. |
| SET-46 | Still deferred. Fetch now returns structured display/values, not the raw upstream response. There are no provider integrations to replace this workflow. |
| SET-69 | The React raw editor wraps on phones (`web/src/routes/settings/settings.css`); tested by settings-check. |
| NAV-81 | The client has a shared sign-in/credential-entry flow. Application data requires login; obsolete public/read-only lock prompts were removed. |
| OV-27 | Container sparklines seed from batched history and share a scale per column (`liveCells.ts`, Containers route). This is implemented; cost is included in the browser measurements, which still have performance misses. |
| OV-29 | Toolbar controls use shared wrapper/styles; phone sort controls are exercised by slice-check. |
| OV-32 | Sort uses the wrapped UI SDK Select; no native-select substitution. |
| LG-18 | Logs use a bounded ring, windowed rows and a shared 128-line frame budget across panes. Burst tasks are measured by logs-check; see the results rather than assuming the implementation passes the 100 ms budget. |
| A11Y-19 | Shared hit-target sizing and real hit testing are enforced by a11y-check. Its measured pass/fail count is in the results; this does not verify screen-reader behavior. |
| ROB-04 | Embedded assets ship raw/gzip. Dynamic JSON and SSE remain uncompressed; measured response sizes and the D9 recommendation are in the results. |
| ROB-05 | The Containers table progressively mounts rows and defers offscreen heavy cells. Phone load and live tasks remain measured acceptance criteria, with misses recorded in the results. |
| ROB-10 | A fresh React load needs its JS and CSS before content. Immutable assets help repeat visits; cold-browser LCP still misses budgets. A 400 kbit/s first visit was not measured in M5. |
| ROB-14 | Structured widget bodies and plain errors are rendered by React, with result age. Read refresh retains the current result, but an error replaces it: a separate last-good-value cache and raw-error disclosure are not built. Partly addressed. |
| ROB-17 | Superseded by D6: the application requires JavaScript and says so in noscript. Health, API and authenticated log downloads still answer; old no-script UI controls are deleted. |
