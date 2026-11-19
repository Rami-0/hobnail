# Design v2: UI, history and in-app settings

Status: **implemented; updated for the React port** (V1 to V4, 2026-11-14). Written as the spec after M0 to
M5; where the implementation showed the spec was wrong or incomplete, the text
below was fixed and the change is listed in section 9. It is the contract the
implementation work is built against. When the code and this file disagree, fix
whichever is wrong.

Section 11 (the ux-pass design system) is newer: a design awaiting the
owner's sign-off, which supersedes the parts of sections 2, 3 and 10 it
names.

Why a v2: the first UI was a clone of Homepage. White cards on pale grey, one big
tile per container (36 containers meant a very long scroll), a system font with no
hierarchy, and no memory: the page only knew "now". Three things change:

1. **A different, denser UI** in the style of Dozzle (sidebar, compact lists,
   dark first, command palette), with charts.
2. **History** in a small local database, so the page can answer "what happened
   last night?" and show trends.
3. **Settings inside the app**, behind a login, so the config file is no longer the
   only way to change anything.

The hard constraints from `GOALS.md` still apply: tiny RAM, tiny image, one static
binary, no Node at run time, nothing leaves the machine unless the user
configured it.

## 1. Decisions

| Topic | Decision | Reason |
|---|---|---|
| UI technology | React 19, TypeScript, Vite; UI SDK, Lucide and Recharts; hand-written CSS on §11 tokens. No Tailwind or WASM. | One embedded client and same-origin Rust API. |
| Charts | Drawn in the browser from JSON series; Recharts for full charts, plain SVG for small graphics. | Lazy chart code. |
| Icons | Lucide React, imported per icon. Opt-in dashboard-icons for service icons. | No icon font or CDN requirement. |
| Fonts | System stacks only, `tabular-nums` for every number. | Works offline, no download. |
| Database | SQLite via `rusqlite` (bundled), WAL mode, page cache capped at 2 MiB. | Range queries and rollups are exactly what it is good at. Costs about 1 MB of binary. |
| Config editing | Lossless edits with `toml_edit` (keeps comments, order, unknown keys), validated with the existing `config::parse` before every write. | A form save must never destroy the user's hand-written comments. |
| Auth | **Viewing stays open** (network-level, as today). **Any write needs a login.** | See section 6. An editor that anyone on the LAN can use is a remote-code-execution hole in this container. |
| Command widgets | Can only be created or changed **in the file**, never from the app. | They run shell commands. The app shows them read-only with a lock icon. |

## 2. Visual language

Reference points: Dozzle (layout, density, dark default), Linear and Grafana
(restraint, chart style). Not Homepage.

### Tokens (CSS variables; user `custom.css` can override all of them)

> **Superseded by §11.2** (brand palette, `--ctl-border`, new token names).
> Kept as the record of v2.

Dark is the default. Light follows `prefers-color-scheme`, and the toggle overrides it.

| Token | Dark | Light |
|---|---|---|
| `--bg` page | `#0b0e13` | `#f5f6f8` |
| `--bg-side` sidebar | `#0e1218` | `#eceef2` |
| `--surface` | `#141a22` | `#ffffff` |
| `--surface-2` (hover, inputs) | `#1a212b` | `#f0f2f5` |
| `--border` | `#242c37` | `#dfe3ea` |
| `--text` | `#e7ecf3` | `#151a22` |
| `--text-2` | `#a3adbb` | `#4a5565` |
| `--text-3` | `#6b7686` | `#7b8696` |
| `--accent` | `#2dd4bf` | `#0d9488` |
| `--ok` / `--warn` / `--bad` / `--off` | `#34d399` / `#fbbf24` / `#f87171` / `#64748b` | `#059669` / `#d97706` / `#dc2626` / `#94a3b8` |
| chart series | cpu `--accent`, memory `#8b9bff`, net rx `#34d399`, net tx `#f59e0b`, temp `#fb923c` | darker equivalents |

Text on every background must reach WCAG AA contrast (4.5:1 body, 3:1 large and UI).

### Type, space, shape

> **Superseded by §11.2 and §11.3** (rem type scale, 4 px space tokens,
> radius, elevation, motion tokens, 44 px touch targets).

- Base 14 px, line-height 1.45. Labels 11 px uppercase with 0.06em tracking.
  Headings 600 weight. Numbers: `font-variant-numeric: tabular-nums`.
- Mono stack (`ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`) for
  container names, images, ports, ids, config text.
- 4 px spacing scale. Table rows 36 px (compact mode 30 px). Radius: 8 px panels,
  6 px controls, 999 px pills. 1 px borders, no heavy shadows (only popovers get one).
- Focus: 2 px accent ring, always visible. Motion: 120 to 180 ms, and none under
  `prefers-reduced-motion`.

### Components

Sidebar, top bar, stat tile with sparkline, status dot (pulses while `starting` or
`restarting`), pill badge, dense table, segmented control (time range), command
palette, toast, drawer/modal, form controls (input, select, toggle, textarea),
tabs, empty state, loading skeleton, inline chart with hover read-out.

## 3. Layout and pages

One client-rendered application with real URLs for pages and dialogs. The SSE
channel carries JSON events, with topics following mounted views.

**Shell**
- **Sidebar** (240 px): title and logo, nav (Overview, Containers, History,
  Alerts, Settings), then a **stack tree**: every compose project with running/total
  counts, expandable to its containers with status dots. A filter box at the top of
  the tree. Stopped containers are dimmed. Collapsible to a 56 px icon rail
  (remembered). Under 700 px it becomes an off-canvas drawer behind a menu button.
- **Top bar:** page title or breadcrumb, a search/command button (`Ctrl/⌘ K` or
  `/`), three vitals chips (CPU, MEM, DISK with 60-second sparklines), theme
  toggle, account menu (Sign in, or the signed-in name and Sign out).
- **Command palette:** searches containers, pages, bookmarks, settings sections
  and actions (toggle theme, open logs for X). Arrow keys, Enter, Esc. If nothing
  matches, Enter does the existing web search.

**Pages**

| Route | Content |
|---|---|
| `/` Overview | Health strip (running / stopped / unhealthy / alerts firing, each a link to a filtered list). System vitals with 1 h charts (CPU, memory, disk, network, temperature, uptime). "Needs attention" list: stopped, unhealthy, restart-looping, firing alerts. Widgets. Top consumers (CPU and memory). Recent events (last 15 from the database). Bookmarks. |
| `/containers` | Dense table, one row per container: status dot, name, stack, image, state and uptime, CPU and memory with inline sparklines, ports as links, row actions (logs, details). Filter by text, state and stack. Sort by any column. Group by stack (collapsible) or flat. Remembers the view. |
| `/containers/<name>` | Header: status, health, image, uptime, restart count. 6 h / 24 h / 7 d charts for CPU and memory. Event timeline for this container. Ports and links. Labels. Matching alert rules. A "Customize" button (name, group, icon, url, hide) when signed in. |
| `/history` | Host charts with a range picker (1 h, 6 h, 24 h, 7 d, 30 d): CPU, load, memory, swap, disk, network, temperature. Container restarts per day. Incident timeline. |
| `/alerts` | Rules with state, channels with delivery status, the firing list, and the alert log from the database (fired / resolved / failed delivery). |
| `/login` | Password form. |
| `/settings/*` | Section 5. Requires login. |

Responsive: at 1100 px and above the sidebar is open; from 700 to 1100 px it is
the icon rail; below 700 px it is a drawer. Tables become stacked rows on a phone.
No horizontal page scroll at 390 px.

Accessibility: everything reachable by keyboard, visible focus, real `<button>`
and `<a>`, `aria-label` on icon buttons, status never conveyed by color alone (dot
plus text), `prefers-reduced-motion` honored.

## 4. History in a local database

**File:** `data/hobnail.db` (override with `RH_DB`). SQLite, WAL,
`synchronous=NORMAL`, `cache_size=-2048` (2 MiB), `mmap_size=0`,
`journal_size_limit` set, `foreign_keys=ON`. Opened once; schema versioned in
`PRAGMA user_version` with forward-only migrations in code.

**Sampler:** one task samples the host and every container every **15 s**, with or
without a viewer (history needs it; the cost is a handful of file reads, and the
cost is measured in `docs/measurements.md`). Interval is configurable
(`[history] sample_interval = "15s"`, minimum 5 s).

**Data and retention**

| Table | Holds | Raw kept | Rolled up |
|---|---|---|---|
| `sys_samples` | host cpu, load1, mem used, swap used, disk used, net rx/tx, temperature | 6 h at 15 s | 1 min (avg+max) kept 7 d, 10 min kept 90 d |
| `ctr_samples` | per container name: cpu %, memory bytes | 6 h at 15 s | same rollups |
| `events` | docker lifecycle (start, stop, die with exit code, oom, health change), alert fired/resolved/delivery failed, config saved/restored, login ok/failed, app start | n/a | kept 180 d, capped at 100k rows |
| `config_revisions` | full config text, author, note, time | n/a | newest 200 kept |
| `sessions` | hash of the session token, created, expires | n/a | pruned when expired |
| `kv` | small settings (password hash, schema extras) | n/a | n/a |

Containers are keyed by **name** (ids change on every recreate). A maintenance job
runs hourly: roll up, prune, `PRAGMA optimize`; weekly `incremental_vacuum`.

**Budgets:** DB file under 50 MB at the defaults on this host (about 36
containers). Idle RAM with the DB and sampler running: under 8 MiB (target remains
15). If a number misses, say so in `docs/measurements.md`.

**Read API (JSON, authenticated; see [api.md](api.md) for roles and scopes)**

- `GET /api/history/system?metric=cpu&range=24h`
- `GET /api/history/container/<name>?metric=mem&range=6h`
- `GET /api/events?limit=50&before=<id>&subject=<name>&kind=<kind>`
- Metrics: system `cpu mem swap disk net_rx net_tx temp load1`; container
  `cpu mem`. Ranges: `1h 6h 24h 7d 30d`. Responses are
  `{"range":"24h","step":60,"points":[[unix,value],...]}` with at most ~400 points
  (downsample on the server).

## 5. Settings inside the app (`/settings`, login required)

Everything in `config.toml` that is safe to change from a browser can be changed
here. Environment settings (listen address, data dir, socket path) stay in compose.

| Tab | What it edits |
|---|---|
| General | `[page]`: title, greeting, columns, layout, theme, search URL, show stopped, show stats |
| Containers and groups | `[docker]` excludes, `[[groups]]` (order, title, collapse), per-container overrides (name, group, icon, url, hide, order, size); search and bulk edit |
| Bookmarks | `[[bookmarks]]` add / edit / reorder / delete |
| Widgets | `custom-api` widgets: form with a live **Fetch now** preview. System tiles on/off. `command` widgets listed read-only (lock icon, "defined in file") |
| Notifications | Channels (webhook, Slack, Telegram): shows the **env var name** and whether it is set, never the value; **Send test**. Alert rules: add / edit / delete, with channels and cooldown |
| Appearance | Theme, `custom.css` editor with live preview (no density setting: there is no config key for it yet) |
| History and data | Retention, sample interval, database size and row counts, export a snapshot, clear history |
| Config file | Raw TOML editor with validation as you type (error with line and column), **Revisions** (list, diff against current, restore) |
| Security | Change password, sign out other sessions, recent logins |

**Editing rules**
- Edits go through `toml_edit`: comments, ordering and unknown keys survive.
- Every save: patch, validate with `config::parse`, write atomically (temp file in
  the same directory, then rename), insert a `config_revisions` row, reload the
  config at once (do not wait for the 2 s poll).
- If the file changed on disk since the form was loaded (content hash mismatch),
  refuse and ask to reload, so a hand edit is never silently overwritten.
- An invalid result is never written. The field that failed is highlighted.
- Secrets are never returned by the app. Config forms hold env-var names or
  file paths; browser-entered integration secrets live in SQLite with their
  bound origin, outside config revisions.
- Restoring a revision creates a new revision; nothing is deleted from history.
- Every change is an event in the timeline ("config saved by admin: widgets").

## 6. Security model

Application data requires login; the React shell, assets, health probe,
session probe and credential-entry endpoints are public exceptions. The route
table and scope gate in `src/api/access.rs` are enforced before dispatch.

| Rule | Detail |
|---|---|
| Accounts | One Owner plus invited Admin and Viewer accounts. Only the Owner manages users, roles, invitations and scopes. |
| First-run | The first visitor to `/app` sets the owner username and password in the browser. `hobnail set-password [username]` is optional host recovery. A migrated legacy-password database locks browser claims until host recovery. |
| Password storage | Salted argon2id in SQLite, 19,456 KiB, two passes, one lane. One shared hashing lock bounds concurrency and can return 429. |
| Sessions | 32 random bytes, only SHA-256 stored. HttpOnly, SameSite=Strict, Path=/, seven-day sliding expiry and cookie renewal; revocable. No Secure cookie flag with the plain-HTTP default. |
| Roles | Viewer reads and manages their account. Admin edits settings. Owner also manages accounts; the sole Owner cannot be deleted, disabled or demoted. |
| Scopes | Owner grants boards/integrations. Scoped accounts see containers via granted boards only; host-wide pages are refused. Scoped Admin edits are restricted to granted boards/integrations; no global Settings or entity creation/deletion. |
| Writes | Matching Origin (or Referer) and Host on every write; authenticated writes require X-RH-CSRF, obtained through GET /api/v1/session. Credential entry checks origin without an existing session. |
| Host guard | Unknown hostnames return 421; IP literals, localhost, RH_HOSTNAME and RH_ALLOWED_HOSTS globs are accepted. Health is exempt. |
| Brute force | Per-peer-IP and global failed-login back-off, exponential to 60 s; five free global failures. Separate per-account password-change limiter. Proxy clients share the proxy peer IP. |
| Command widgets | File-only: cannot be added, edited, enabled or converted from browser forms or raw config. Deletion is allowed. |
| Config secrets | New file references must resolve inside data/secrets. Secret-bearing destination/reference pairs must already exist in the host file. Preview uses the same guards. Inline channel fields are redacted from Settings, raw config, revisions and diffs; an invalid config must be repaired on the host before the raw editor can show it. |
| Integration secrets | Write-only SQLite credentials bound to the normalized origin; sent only there. Changing origin requires a new credential or clearing it. Paths are not part of the binding. |
| Viewer output | No configuration, credential values, integration paths, environment values, process commands or probe diagnostics; auth events are administrative. Streams recheck sessions and grants. |
| Outbound requests | http.allow_hosts is host policy. Integration tests additionally reject restricted addresses unless explicitly allowed, stop at redirects and trust public certificate roots only. No pinning yet. |

## 7. Module contract (so parallel work merges cleanly)

New code lives in new files wherever possible.

```
web/                React client, UI wrappers, charts, routes and unit/browser checks
src/api/            versioned JSON contracts, access gate, boards/integrations/accounts
src/read_model/     presentation data shared by API handlers
src/http/assets.rs  embedded raw/gzip client, cache and CSP headers
src/db/             SQLite history, accounts, dashboard data and migrations
src/auth.rs         argon2id, sessions, rate limits and recovery CLI
src/settings/       lossless TOML editing, guards, revisions and legacy JSON API
```

`AppState` gets `pub db: Arc<db::Db>`. `db::Db` is a synchronous handle (internal
mutex around one connection). Async callers wrap read queries in
`tokio::task::spawn_blocking`; the sampler's 15 s batch write is small enough to
run inline. The public API is fixed in `src/db/mod.rs` (signatures only until the
storage work lands). The UI must compile and render against the stub, showing an
empty chart state, and fill in once the real implementation is merged.

## 8. Quality bar and how it is checked

- `scripts/cargo.sh fmt --check`, `clippy -D warnings`, `test`, plus client type, lint, unit and bundle-size checks gate every build (as now).
- Unit tests: rollups and retention, series downsampling, password hash and
  session expiry, rate limit, `toml_edit` patches that preserve comments, config
  validation refusing bad input, the `command` widget guard, CSRF rejection.
- Visual check in a real browser (headless Chromium) at 1440, 900 and 390 px wide,
  dark and light, with screenshots reviewed by a person, not only "no errors".
- Measure again: idle RAM, RAM with a viewer, image size, DB size after 24 h,
  page load, and the cost of the 15 s sampler. Record in `docs/measurements.md`.

## 9. Corrections after implementation

| Where | Spec said | Built / why |
|---|---|---|
| §4 events API | response shape not specified | `{"events":[{id,ts,kind,level,subject,detail}]}`, newest first, paged with `before`. Anonymous viewers never get `auth.*` events. |
| §4 retention | implied configurable | `[history]` has `enabled` and `sample_interval` (5 s to 5 min). Retention is fixed in code (6 h raw, 7 d at 1 min, 90 d at 10 min, events 180 d / 100k). |
| §4 series | — | Each point is labelled with the start of its bucket; rollup rows carry a sample count so averages over mixed raw/rollup buckets are weighted correctly. |
| §5 change password | — | Signs out **every** session, including the caller's (the page then goes to the login). |
| §5 Appearance | "density" | Not built (no config key). |
| §5 command widgets | "refuse add/change/enable" | Also: any widget whose `type` is not exactly `custom-api`, or that has `command`/`env` keys, counts (fail closed). **Deleting** one from the app is allowed (it only removes capability). |
| §6 secrets | env var names only | Plus the secrets-directory allowlist and destination binding (see the §6 table). Found by security review. |
| §6 brute force | global back-off | Exempts clients that already signed in (see §6). |
| §3 `page.columns`, group `columns`/`layout` | (v1 options) | No effect in the v2 table; `layout = "list"` makes the flat table the default. |
| §7 assets | one CSS/JS bundle | The main CSS is inlined in every page; settings pages load `/static/settings.css` and `/static/settings.js` separately so other pages do not carry them. |
| §1 JS size | ~15 KB | `app.js` is ~30 KB (palette, live table patching, charts hover, drawer/rail). Recorded as missed in `docs/measurements.md`. |

## 10. Design system v3 (Wave 1 design pass)

> **Partly superseded by §11** (ux-pass). Still valid: panel hierarchy, calm
> zeros, health strips, charts, keyboard, alarm tab. Replaced: Type (§11.3),
> Motion (§11.2), Themes (§11.2: Ember retired), Installable (§11.12: no
> offline page), the teal accent.

From the Wave 1 design review. The
owner signs off the look; these are the builder's decisions until then.

- **Type.** No web font: a ≤ 40 KB variable Latin subset of a good sans is
  borderline, and the platform faces (SF Pro on Apple, Segoe UI Variable on
  Windows, Roboto on Android) are already excellent. Instead a strict scale:
  display 32/600 (one per page), value 24/600, title 15/600, body 14/400,
  label 11/600 uppercase +0.06em, micro 12/400. **Values use the UI sans with
  `tabular-nums` and −0.02em tracking**; mono only for identifiers (names,
  images, ports, ids, config, logs). Units small and muted next to values.
- **Panel hierarchy.** *Primary* (raised surface, header row, used once or
  twice per page), *secondary* (flat surface, 1 px border), *ghost* (no box,
  only a section label). Overview: health strip + attention primary; host and
  charts secondary; widgets, events, bookmarks ghost/secondary.
- **Calm by default.** Zero counts render in `--text-3` at value size − 8 px;
  non-zero problem counts take their status color. Status colors are reserved
  for status; teal is accent (links, focus, selection, primary series) only.
- **Signature element: health strips.** A thin bar of the last 24 h (default
  96 cells × 15 min, or 7 d × 2 h), each cell green / amber / red / grey
  (unknown or not running) from the history database. Used per container
  (table, detail, sidebar on hover), per stack and on Overview.
- **Charts.** Shared crosshair across all charts of a page; drag to zoom a
  range (zoomed range in the URL); event markers (restarts, alerts) clickable;
  optional "same time yesterday" faint line; min/max band on rollups; legend
  toggles series.
- **Motion.** 160 ms ease-out; numbers that change briefly tween; skeletons
  instead of blanks; "updated N s ago" turns amber when stale (> 3 × refresh).
  Everything off under `prefers-reduced-motion`.
- **Themes.** Dark (default), Light, Ember (warm dark: browns, amber accent),
  High contrast (≥ 7:1 body). All are CSS variable sets, AA-checked by a test.
- **Keyboard.** `g o/c/h/l/a/s` pages, `j/k` rows, `/` filter, `?` cheat sheet,
  `l` on a container opens its logs, `Ctrl/⌘ K` palette.
- **Alarm tab.** Problem count in the document title and as a favicon badge.
- **Installable.** Web manifest, SVG/PNG icons, standalone display, a phone
  bottom bar (Overview, Containers, Logs, Alerts, More), an offline page; no
  service worker that caches data.

## 11. ux-pass design system (tokens, components, flows)

Status: **design, awaiting the owner's sign-off** (2026-11-15, revised after
an independent review the same day). Written against the audit in
`docs/qa/QA.md` (267 findings). It is the contract for the five build waves:
(1) tokens and components alone; (2) every add / edit form onto the dialog,
starting with ROB-15; (3) navigation, one-click logs and the removals,
starting with LG-12; (4) motion, loading and empty states; (5) the rest by
severity. Where this section and an earlier one disagree, this one wins; the
earlier text is marked "superseded".

**Everything beyond the owner's agreed removals is a proposal** and is listed
once in §11.13 "Changes that need the owner's yes". Nothing on that list is
built until the owner says yes to it.

A static prototype of the components and two composed screens was built and
screenshotted outside the repository (the ux-pass scratch area). Its
`proto.css` and `proto.js` are written as they should ship: the part between
`SHIP` and `END SHIP` historically supplied the retired UI; the current
client styles live in `web/src/styles/`; the rest is prototype scaffolding. A behaviour script checks
every rule marked "checked" below (70 checks).

### 11.1 Rules that hold everywhere

- No web font or CDN requirement. React, UI SDK, Lucide and Recharts are
  the client libraries; additions need a measured bundle cost. UI SDK packages
  are pinned exactly to 0.1.0-beta.10 (2026-11-18), the owner's sole seven-day
  age exception; all primitives are wrapped in `web/src/ui/`. Other npm
  dependencies use min-release-age=7 and ignore-scripts; new crates use the
  same seven-day policy.
- **The application needs JavaScript.** The noscript message names the health
  probe, JSON API and authenticated log download that still answer.
- **Nothing a signed-out visitor receives carries settings data.** Dialogs,
  `data-item` JSON, widget URLs, environment-variable names and any form
  value are rendered only for a session (§11.14).
- One primary action per view. Status is never colour alone: a word, a shape
  or an icon always goes with it. The accent is never a status colour (brand
  Accent and Error have the same luminance, A11Y-17).
- Labels in plain words; the same word for the same thing on every page
  ("Edit", "Logs").
- **Multi-field changes happen in a dialog, which is a draft until Save.**
  Settings that are a single control (a switch, a theme choice) save when
  changed, with a toast; a page section with several fields keeps one Save.
- Long values never widen a page: table cells wrap with
  `overflow-wrap:anywhere`; single-line chrome (group headers, buttons,
  titles) ellipsises and carries the full text in `title` (SET-41, SET-48,
  ROB-07, ROB-08). Inputs have `maxlength` where the config has a sensible
  limit (names 80, descriptions 200; SET-28).
- `prefers-reduced-motion: reduce` sets every duration token to 0, stops
  every animation and every smooth scroll (A11Y-22). Nothing that carries
  meaning depends on motion.

### 11.2 Colour tokens

The brand palette (`docs/BRANDING.md`) mapped onto the theme variables.
**Bold** values are brand values used as they are; the others are derived
(a mix of two brand values, or a step lighter / darker) and were checked for
contrast. Theme blocks hold plain hex only (the contrast test parses them).
Selectors: `:root,[data-theme=dark]`, `[data-theme=light]`,
`[data-theme=contrast]`, and the OS-light block
`@media (prefers-color-scheme:light){:root:not([data-theme]),[data-theme=auto]}`
which must equal Light. The bare `[data-theme=x]` form lets a preview show
another theme inside a page.

| Token | Role | Dark | Light | High contrast |
|---|---|---|---|---|
| `--bg` | page | **#1D1210** (Surface) | #FAF0EA | #000000 |
| `--bg-side` | sidebar, bottom bar | #1A100E | #F7ECE5 | #000000 |
| `--surface` | panels, tables | #241714 | **#FFF8F3** (Surface) | #0A0706 |
| `--surface-2` | hover, selected, chips, secondary button | **#35231D** (Raised) | **#F4E8E0** (Raised) | #241A16 |
| `--raised` | dialogs, menus, toasts | #281A16 | #FFFBF8 | #120D0B |
| `--field` | input fill | #1A100E | #FFFDFB | #000000 |
| `--border` | panel outlines, decorative separators only | **#795245** | **#AD8877** | #A08A80 |
| `--line` | table row hairlines, dividers | #4E342C | #DAC6BB | #5A4A44 |
| `--ctl-border` | **new**: inputs, selects, checkboxes, outlined buttons, switches | #9A7566 | #8E6B5C | #D9C8BF |
| `--text` | body | **#FFF0E5** | **#2B1912** | #FFFFFF |
| `--text-2` | secondary | **#CDB4A8** (Muted) | **#725448** (Muted) | #FFF0E5 |
| `--text-3` | labels, hints, placeholders, icons at rest | #B09486 | #7E6053 | #E6D3C9 |
| `--accent` | primary fill, active marker | **#F77848** | **#AC3712** | #FF9F7A |
| `--accent-hover` | primary hover | **#FF956B** | **#922B0A** | #FFB89B |
| `--accent-fg` | links, accent text and icons | **#F77848** | **#AC3712** | #FFB08F |
| `--on-accent` | text on the accent | **#26120B** | **#FFFFFF** | #000000 |
| `--focus` | the focus ring | **#F77848** | **#AC3712** | #FFFFFF |
| `--ok` / `--ok-fg` | running, healthy, saved | **#83CCA7** | **#216648** | #8FF0B9 |
| `--warn` / `--warn-fg` | restarting, stale | **#F4D05E** | **#806000** | #FFE27A |
| `--bad` / `--bad-fg` | unhealthy, failed, errors, danger | **#FF678B** | **#AD1747** | #FF9AB2 |
| `--on-bad` | text on a danger fill or badge (A11Y-15) | #26120B | #FFFFFF | #000000 |
| `--off` | stopped (hollow dot) | #8F7A6F | #94796D | #BFB0A8 |
| `--ch-*` | chart series (cpu, mem, rx, tx, temp, load, swap, disk) | #F77848 #A99BFF #83CCA7 #F4D05E #FFB27A #7CC4F0 #D69CF0 #F28FB8 | #AC3712 #5A48C8 #216648 #806000 #A4520F #1F6E9E #7B3FA0 #A3245E | #FF9F7A #C4B8FF #8FF0B9 #FFE27A #FFC59B #9AD8FF #E9C2FF #FFB3D1 |

Non-hex tokens per theme: `--tint` / `--tint-2` (hover and press overlays),
`--scrim` (dialog backdrop), `--shadow-1/2/3` (High contrast: outlines).

**Old names stay working.** `--ctl` and `--border-2` (now `--ctl-border`),
`--accent-bg` (now `--accent`), `--glow` (now transparent) and `--pop` (now
`--shadow-2`) are declared as aliases on `:root,[data-theme]`, so a user
`custom.css` that **reads** them keeps working. A `custom.css` that only
**sets** an old name no longer recolours the new components; the Appearance
editor shows a warning naming the new token when it sees one of the five old
names being set. The contrast test in `src/ui/shell.rs` moves to these
selectors and names. (Ember: §11.13.)

**Control border, measured** (`--ctl-border` against every surface it can
sit on; needs 3:1):

| Theme | on `--bg` | on `--bg-side` | on `--surface` | on `--surface-2` | on `--raised` | on `--field` | worst |
|---|---|---|---|---|---|---|---|
| Dark #9A7566 | 4.46 | 4.55 | 4.23 | 3.63 | 4.09 | 4.55 | **3.63** |
| Light #8E6B5C | 4.25 | 4.10 | 4.53 | 3.96 | 4.63 | 4.70 | **3.96** |
| High contrast #D9C8BF | 12.96 | 12.96 | 12.40 | 10.50 | 11.91 | 12.96 | **10.50** |

The brand Border is 2.20 to 2.76 (dark) and 2.66 to 3.15 (light): fine for a
decorative line, never for a control.

**Other measured minimums** (worst of the six surfaces unless named):

| Pair | Dark | Light | High contrast | Needs |
|---|---|---|---|---|
| `--text` | 13.37 | 13.97 | 17.01 | 4.5 (7 in HC) |
| `--text-2` | 7.58 | 5.68 | 15.27 | 4.5 (7) |
| `--text-3` | 5.27 | 4.74 | 11.77 | 4.5 (7) |
| `--accent-fg` | 5.49 | 5.28 | 9.61 | 4.5 |
| `--ok-fg` / `--warn-fg` / `--bad-fg` | 7.92 / 9.95 / 5.37 | 5.71 / 4.86 / 5.84 | 12.42 / 13.30 / 8.52 | 4.5 |
| `--focus`, `--accent` (UI) | 5.49 | 5.28 | 17.01 / 8.50 | 3 |
| `--off` (stopped dot) | 3.68 | 3.35 | 8.09 | 3 |
| `--on-accent` on `--accent` / `--accent-hover` | 6.59 / 8.32 | 6.35 / 8.16 | 10.49 / 12.58 | 4.5 |
| `--on-bad` on `--bad` | 6.44 | 7.02 | 10.52 | 4.5 |
| Pill text (`.pill.warn/.bad/.ok`) on `--bg-side` and `--surface-2`; pills have **no tinted background**, only a coloured outline | 9.95 / 5.37 / 7.92 | 4.86 / 5.84 / 5.71 | 13.30 / 8.52 / 12.42 | 4.5 (12 px) |
| Sidebar logs icon at rest (`--text-3`, full opacity) on `--bg-side`; `--accent-fg` on hover / focus | 6.60 | 4.91 | 14.53 | 3 |
| Unchecked checkbox outline (`--ctl-border`, drawn by CSS, not the browser) on `--surface` / `--field` | 4.23 / 4.55 | 4.53 / 4.70 | 12.40 / 12.96 | 3 |
| Check mark (`--on-accent`) on a checked box (`--accent`) | 6.59 | 6.35 | 10.49 | 3 |

Rules that keep these true: no text or icon gets an `opacity` below 1 (a
disabled control is the only exception, and it is not required to pass);
placeholders use `--text-3` through one global
`::placeholder{color:var(--text-3);opacity:1}` (A11Y-13); A11Y-14 is fixed by
the first rule in wave 5. Without `color-mix()` support, pills and notes
fall back to a full-strength status border and no tint (declared first, then
overridden).

### 11.3 Type, space, shape, elevation, motion

**Headings and the display face.** No web font. Every heading uses the
platform UI stack (`--font`) at weight 650 with −0.01em tracking (−0.025em at
display size). Big Shoulders Display appears only as outlines inside the
wordmark SVG (`docs/brand/wordmark*.svg`), which the app does not ship; the
sidebar keeps the mark glyph plus "Hobnail" in the UI sans. Mono (`--mono`)
is for identifiers only. A display name is a title and uses the UI sans
(OV-39).

**Type scale**, in rem so a larger browser default scales the UI (A11Y-20);
the root is 87.5 % (14 px at the default 16 px). Line height `--lh` 1.45,
headings `--lh-tight` 1.2, uppercase labels `--track-label` 0.06em.

| Token | px at default | Use |
|---|---|---|
| `--fs-display` | 32 | one per page at most |
| `--fs-value` | 24 | big numbers |
| `--fs-h2` | 18 | dialog titles, Settings section titles |
| `--fs-title` | 15 | top-bar title, empty-state titles |
| `--fs-body` | 14 | body, tabs |
| `--fs-sm` | 13 | labels, buttons, table cells |
| `--fs-xs` | 12 | hints, errors, times, small buttons |
| `--fs-micro` | 11 | uppercase section labels |
| `--fs-input` | 13, **16 on phones and touch** | inputs (no iOS zoom, SET-14) |

**Space**: `--s-1` 4, `--s-2` 8, `--s-3` 12, `--s-4` 16, `--s-5` 20, `--s-6`
24, `--s-8` 32, `--s-10` 40 px.

**Sizes**: `--ctl-h` 32 px and `--ctl-h-sm` 28 px (controls); `--ia` 28 px
(icon actions in rows); `--hit` 24 px (minimum target, WCAG 2.2 AA). Under
`(pointer:coarse)` or below 700 px: `--ctl-h` 44, `--ctl-h-sm` 36, `--ia` 36,
`--hit` 44, and small buttons, icon actions and the Settings row name link
get a 44 px hit area through `::after` without changing their look (A11Y-18,
A11Y-19). Shell: `--side-w` 240 px, `--top-h` 52 px.

**Radius**: `--r-xs` 4, `--r-sm` 6 (controls), `--r` 10 (panels, menus,
toasts), `--r-lg` 14 (dialogs, sheets), `--r-pill`.

**Elevation**: `--shadow-1` raised panel, `--shadow-2` menu and toast,
`--shadow-3` dialog. High contrast uses outlines. **Layers**: `--z-top` 20
(top bar), `--z-side` 30 (sidebar and drawer), `--z-pop` 40 (menus without
popover support), `--z-toast` 70. Dialogs and popovers use the top layer.

**Motion tokens**:

| Token | Value | Used for |
|---|---|---|
| `--dur-1` | 90 ms | hover, press, colour changes |
| `--dur-2` | 160 ms | toast, menu, tab underline, field error, page cross-fade |
| `--dur-3` | 240 ms | dialog and sheet open / close, backdrop |
| `--dur-4` | 1600 ms | the highlight that fades from a new or changed row |
| `--ease-out` | cubic-bezier(.2,.7,.3,1) | everything that enters |
| `--ease-in` | cubic-bezier(.5,0,.75,0) | things that leave |
| `--ease-io` | cubic-bezier(.45,0,.25,1) | things that move from A to B |

`@media (prefers-reduced-motion:reduce)` sets the four durations to 0 ms and
keeps the global rule that stops animations, transitions and smooth
scrolling; script-driven motion checks the same query (checked). View
Transitions (`@view-transition{navigation:auto}`) only under
`no-preference`; sidebar and top bar get their own `view-transition-name`.
**Fallbacks**: without View Transitions the page simply navigates; without
`@starting-style` an element appears without its entry motion; without
`transition-behavior: allow-discrete` (`overlay`, `display`) the exit
transition is dropped and the dialog or menu disappears at once.

### 11.4 Buttons

| Kind | Class | Use |
|---|---|---|
| Primary | `.btn.primary` | the one main action of a view or dialog |
| Secondary | `.btn` | everything else (Cancel, Fetch now, Save and send test) |
| Ghost | `.btn.ghost` | quiet actions in headers ("Edit" on a container) |
| Danger | `.btn.danger` | a destructive secondary ("Discard", "Remove overrides…") |
| Danger primary | `.btn.primary.danger` | only the confirm button of the confirm dialog |
| Icon | `.btn.icon`, `.ia` | icon-only; always an `aria-label`, a `data-tip` tooltip on hover devices |
| Small | `.btn.sm` | row actions ("Logs") |

States: hover tints, press moves 1 px down, one focus style (2 px `--focus`
outline, 2 px offset; 1 px on inputs; `.focus-in` puts it inside a clipping
parent, A11Y-04, A11Y-06), disabled 50 %. **Busy**: the button that was used
gets `aria-busy=true` and `aria-disabled=true` (not `disabled`, so it keeps
focus), a spinner and a verb ("Saving…", "Sending test…"); every other action
of the form is `disabled` (checked).

### 11.5 Form field

Anatomy, top to bottom: **label** (with a muted `*` and `required` when
required, SET-20), **control** (`.in`: input, select, textarea; a checkbox
drawn by CSS with a `--ctl-border` outline; a switch `.sw`; or a choice
`.choice`, a radio group drawn as a segmented control for two to four
options), **error** (`.fld-err`, only when there is one: a "!" mark plus words
in `--bad-fg`), **hint**. `aria-describedby` names error and hint; an invalid
control has `aria-invalid=true` and a 2 px `--bad` border.

Fields that belong to one choice carry `data-for="a b"` (the `type` choice)
or `data-for="name:a b"` (another choice). Hidden ones are also `disabled`,
so they are neither sent nor validated; when a chosen option disappears (a
"Plain URL" source when the type becomes Slack) the first remaining option is
chosen (checked).

Validation, in the browser first and on the server always:

- On submit every field that is not disabled is checked, **including fields
  inside a closed `<details>` or a hidden tab panel**; every problem shows at
  once on its field; the first one is revealed (its `<details>` opened, its
  tab selected) and focused; the footer says "2 fields need attention."
  (SET-05, SET-70; checked).
- A field the user changed is checked when it is left; an error clears as
  soon as the value is fixed.
- Rules from attributes: `required`, `maxlength`, `type=number` with `min` /
  `max` / `step` (SET-07), `data-v="http"` (SET-24, SET-35, SET-40),
  `data-v="duration"` (SET-06, SET-61), `data-v="env"`, `data-v="id"`,
  `pattern` with `data-msg`. Values are trimmed, **except passwords**.
- A server 422 names the field (`field`, the form's own field name); the
  message goes on that field in plain words using the label on screen, never
  a config-file line or a deserialiser message (SET-06, SET-07, SET-55).
  Only errors that belong to no field go to the dialog note.
- Names other config refers to (widget id, channel name) are read-only when
  editing, with a hint saying why (SET-49; see §11.13).

### 11.6 Dialog (every add / edit)

One component, `<dialog class="dlg">` holding one `<form method="post"
data-hb novalidate>`. Anatomy: header (title, × close), body (`.dlg-b`,
scrolls), an optional note (`.dlg-note`, `tabindex=-1`) at the top of the
body, footer (status message left, Cancel, secondary actions, the primary
right), and the guard (`.dlg-guard`: "Discard your changes?" · Keep editing ·
Discard) that replaces the footer buttons. Sizes: 560 px; `.wide` 760 px.
Under 700 px a bottom sheet (full width, up to 94 % of the height) whose
footer stays in view above the safe area (SET-13). `max-height` is given in
`vh` first and `dvh` second, so browsers without `dvh` still get a limit.

**The form's only `type=submit` button is the primary.** Other actions that
save ("Save and send test") are `type=button data-submit="test"`; the script
submits the form with that action. So Enter (implicit submission) always
runs the primary (checked).

| Rule | Detail |
|---|---|
| The URL is the dialog | Triggers are links (`[data-dlg=<id>]`, `href="?add=bookmark"`, `?edit=bookmark:3`; `data-item` holds the row's values, `data-key` its identity). Opening pushes the current page's URL with that `add=` / `edit=` parameter. A page loaded with the parameter opens the dialog on load (a reload, a shared link, and **Ctrl/⌘/middle click, which opens a new tab whose page opens the same dialog**; checked). The React router opens the same dialog at that URL; JavaScript is required. Closing removes the parameter: `history.back()` for an entry we pushed, `replaceState` for one we landed on. |
| Opening | Scales in from 97 % with a slight offset toward the trigger, over `--dur-3`, with `@starting-style`. Focus goes to the first editable field, or the field named by `data-focus`, or the first focusable item of the body. Tab stays inside (native modal). |
| One at a time | Opening another dialog while one has unsaved input shows the guard instead and opens nothing; from a clean dialog it switches and reuses the history entry with `replaceState` (checked). |
| Close | Esc, ×, Cancel, a backdrop click (press and release outside the box), **a native close request (`cancel` event, e.g. `requestClose()` or the Android back gesture)** and Back all go through one path: with unsaved input they show the guard, focused on "Keep editing", and Back re-pushes the dialog's URL. A `close` listener clears the state, the history entry and returns focus whatever closed it. "Unsaved" means the values differ from what was loaded (SET-04, SET-58). Esc is ignored by the form dialog while another modal (the confirm) is on top (checked). |
| Enter | Enter in a single-line field submits with the primary; Ctrl/⌘ Enter submits from a textarea or a list (SET-31). |
| Save | Validate (§11.5); then busy (§11.4), the body `inert`, Cancel and × disabled (SET-09). Where `inert` is missing the body stays interactive, and a second submit is ignored while busy. |
| Success | The dialog closes, a toast says what happened ("Added “Wiki” · revision #67"), the list behind updates in place (the server re-renders the list fragment, §11.14) and the new or changed row flashes once (SET-10). Focus returns to the trigger; when the trigger was re-rendered, to the element with the same `data-key` (the edited row's link; checked, SET-08). Saves that change the chrome (title, theme) update it (SET-25). |
| Server errors | 422: on the field. 409 stale file: a note with "Reload and keep my input" (fetch the new hash, keep every value) and "Discard my input" (SET-15, SET-16, SET-65). 401 session gone: a note with "Sign in", which opens `/app/login` in a new tab; when that tab is done the user presses Save again and the script first **asks `GET /api/v1/session` for the new token**, then retries. 403 locked: a note with the lock sentence. Network error: a note, input kept. **When the answer is a note only, focus moves to the note** (checked). |
| Stays open | Actions that save and then show a result keep the dialog open in edit mode and put the result in the dialog note, never a toast: "Save and send test" (channel), "Fetch now" (widget). |

**Fields per dialog.** No field of today's forms disappears by accident. The
lists come from `src/ui/settings/*.rs` and QA.md's form inventory; anything
not listed here is in §11.13.

| Dialog | Fields (in order) |
|---|---|
| Bookmark | Name*, Group (datalist), URL* (http/https), Icon, Description |
| Widget (wide, tabs Request / Fields / Display) | Request: Id* (read-only on edit), Title, URL*, Method (GET/POST), Link, Icon, Refresh, Timeout, Enabled; read-only "Secret headers set in the file" box when present. Fields: rows of name + path (one empty row to start), "Add field". Display: One line (Template) or Rows (label + value rows, "Add row"); Status field, OK values. "Fetch now" |
| Channel | Name* (read-only on edit), Type (Webhook / Slack / Telegram), Secret comes from (Environment variable / File in `data/secrets/` / Plain URL, the last for Webhook only); Slack: `webhook_url_env` or `webhook_url_file`; Telegram: `token_env` or `token_file`, Chat id*; Webhook: `url_env`, `url_file` or `url` (plain text, with the warning); More options: Timeout; read-only box for values bound in the file. "Save and send test" |
| Alert rule (wide) | Name*, Type (Container state, HTTP check, Host metric, Widget value, Log match), Enabled; Condition by type: Containers, Except, Fire on (stopped, exit non-zero, unhealthy, restarting), Restarts, Window, Pattern, Stack, Lines, URL, Expected status, Body contains, Failures in a row, Timeout, Metric, Disk, Check every, Widget, Field, Equals, Not equals, Fire when the widget fails, Above, Below; Notify: Channels (all ticked for a new rule), For, Cooldown, Message, Recovery message (the last four under "More options") |
| Container override ("Edit") | Display name, Group, Icon, URL, Description, Hide (Default / Hide / Show), Order, Card size (Default / Small / Normal / Wide); "Remove overrides…" (confirm) |
| Group | Name*, Title, Icon, Collapsed, Hidden (Columns and Layout go: agreed) |
| Hidden containers | Pattern* with a live "matches N containers" line |
| Reorder (bookmarks, widgets, groups) | the list with ↑ / ↓ per row and Alt + ↑ / ↓; Save order |
| Revision view / diff | read-only text, "Open in the editor", Restore… |

Page sections that stay on the page use the same fields, buttons,
validation and notes: General (Title, Greeting, Search URL, Show stopped
containers, Show live CPU and memory), Recording (Enabled, Sample interval),
Appearance (Default theme: a single choice that saves on change; custom.css
with Save), Config file (raw editor), Security (Current, New, Repeat
password; Sign out other sessions), Clear history (typed confirm).

Platform: `<dialog>.showModal()`. Without `HTMLDialogElement` the script does
not intercept the link and the page form is used.

### 11.7 Toast

`HB.toast(text, {kind, action, sticky})`. One region (`role=status`,
`aria-live=polite`), bottom right; on phones full width above the bottom bar
(SET-12). Kinds: ok, info, bad (`role=alert`, stays until closed). At most
three; the oldest goes. Ok and info leave after 4.5 s (8 s with an action),
paused while hovered or focused; each has a close button and at most one
action ("Undo"). **No toast is shown while a dialog is open**: a modal makes
the rest of the page inert, so a toast could not be clicked. The message goes
to that dialog's note instead (checked). Toasts are announced for: saved,
deleted, test results outside a dialog, theme changes (A11Y-11), export
started (SET-62).

### 11.8 Menu (popover)

Trigger: `<a href="<plain page>" data-menu="<id>" role="button"
aria-haspopup="menu" aria-expanded>`; menu: `<div class="menu" popover
role="menu">`. **Without script the trigger is just a link to the page that
does the same job** (theme → Settings › Appearance; a row's ⋯ → that row's
edit page, which has Delete); the popover never opens, because nothing uses
`popovertarget`. With script: a click (or Space) opens the popover, places it
under the trigger (flipping above when there is no room), focuses the checked
or first item, and adds Arrow keys, Home, End; Esc and a click outside close
(native light dismiss) and focus returns to the trigger. Until placed, a menu
is centred (`inset:0; margin:auto`), never at 0,0. Without `popover` support
the script toggles a class. Uses: the theme menu, the row ⋯ menu (Edit,
Reorder…, Delete…), the account menu.

### 11.9 Tabs, row actions, empty state, skeleton, confirm

**Tabs.** Page tabs are links with `aria-current=page` and an accent
underline. In-page tabs (inside a dialog) are `role=tablist` / `tab` /
`tabpanel` with Arrow keys, Home, End. A strip wider than the screen fades at
its edges and the current tab is scrolled into view (OV-36, SET-23).

**Row actions** (`.ra`). In tables the whole row opens the thing: the name is
a stretched link (`.row-link::before`) and **the row has class `rl`**
(`position:relative`), so the stretch never escapes the row; actions sit
above it (checked: the sidebar, tabs, header buttons and every row stay
clickable). Container rows: **"Logs"** as a labelled small button, first;
then "Open ↗" only when the service has a link; an empty slot keeps columns
aligned. In compact lists (sidebar tree, Overview attention, top lists and
Recent events, Settings › Containers and groups, Host, History, Alerts): a
28 px logs icon (`.ia.logs`, "Logs of <name>") at the end of every row that
names a container, in `--text-3` at rest and `--accent-fg` on hover and
focus. Settings lists: the name opens the edit dialog (44 px hit area on
touch), a pencil does the same, and ⋯ holds Edit, Reorder… and Delete….
Log lines: copy and permalink are focusable buttons that also show on focus
and tap (LG-20).

**Treemap tile** (Overview, memory): a `div.tm` (`position:relative`) holding
a stretched name link to the container and, as a sibling (never a link inside
a link), an `a.ia.logs` in the top-right corner above it. The icon shows when
the tile is at least 64 × 40 px with a fine pointer, or 88 × 56 px on touch,
where its 44 px hit area takes the corner and never more than half the tile.
Smaller tiles show no name and no icon (OV-09); see §11.11 for their count.

**Reorder** (`.reorder`, in a dialog): rows with ↑ / ↓ buttons and Alt +
↑ / ↓, a draft until "Save order", which writes **one** revision for the whole
arrangement (SET-42). Esc with a changed order asks first (checked).

**Empty state** (`.empty`): icon, a title in plain words, one sentence, at
most one action. Kinds: first use, no match, good news (OV-24), error. Every
list and pane has one (LG-14, LG-15, ROB-01, ROB-02); copy never pastes a
filter name into a sentence (OV-61).

**Skeleton** (`.sk*`): shapes of the coming content on a region with
`aria-busy=true`; they appear after 150 ms and shimmer only without reduced
motion (ROB-14). **Navigation progress**: a 2 px accent line after 150 ms on a
full page load (ROB-12).

**Confirm destructive** (`HB.confirm({title, body, ok, typed})`, one `<dialog
class="dlg confirm">` per page): the title names the thing, the body says
what happens and whether it can be undone, Cancel has focus, the filled
danger button names the action; things that cannot be undone ask for the
name to be typed (SET-19, SET-32, SET-63). A delete that would break something
says so first (SET-53). Esc and the backdrop cancel only the confirm.

### 11.10 Navigation and logs

- **Logs is one click from every place a container is named**, with the
  stated exceptions in §11.11. The container's Logs tab is the log viewer
  (`/containers/<name>/logs` renders the header and tabs above the viewer;
  §11.13). The `/logs` page becomes a filterable list grouped by stack (LG-04);
  the Logs nav item is current on every logs page (LG-01); stack logs only
  for compose projects (LG-09).
- **The live tail rests on the newest line** (LG-12, first in wave 3):
  "following" means the last line is in view after the first load, after
  every batch and after "Jump to latest", measured after layout; lines no
  longer use `content-visibility:auto` with a fixed placeholder (or a
  `ResizeObserver` re-asserts the bottom until the height settles).
- **Container header**: status dot, display name with a state pill, the
  sub-line `name · image · stack`, "Open ↗" (when it has a link) and "Edit"
  (signed in; the override dialog on the page, SET-37). "Why is this red?"
  stays on top with "Logs around 02:14".
- **Probe output stays signed-in only** (as today, NAV-11). Signed in: the
  Overview attention row reads "unhealthy for 41 min · <first probe line>";
  the detail page shows the probe output in "Why is this red?" and a Health
  check panel with the last five probes. **Signed out**: the row reads
  "unhealthy for 41 min" (state and duration from the container list); the
  detail page says "Health check failing since 02:14, 6 in a row. Sign in to
  see the check's output." and has no Health check panel.
- **Overview**: attention rows are whole-row links to the cause with the
  reason inline and a logs icon (OV-01, OV-04); signed in, Bookmarks and
  Widgets have "Add" in their header (OV-12).
- **Alerts page**: incidents and log rows link to their subject, show the
  rule name and a clock time, carry "Logs at 02:14"; signed in, "Add
  channel", "Add rule", per-channel "Send test" and per-rule switches
  (LG-55, LG-56, LG-57, SET-54; §11.13).
- **History**: the timeline follows the chart range and zoom, pages back,
  shows clock times, rows link to their subject and logs at that moment;
  a chart marker highlights its row (LG-43, LG-44, LG-47, LG-48).
- **Settings**: `/settings` without the index (SET-22); signed out, only the
  lock panel (SET-21).
- **Command palette**: "Logs: X" ranks directly under X; actions for the top
  tasks (Add bookmark, Add widget, Add channel, Add alert rule, Edit X) and one
  per theme (NAV-51, NAV-52, NAV-55). What Enter on a container does is in
  §11.13 (NAV-50).
- **Phone**: a closed drawer is `inert`; an open one makes the page `inert`
  and Back closes it (A11Y-01, A11Y-03, NAV-33). "More" is a link to a plain
  page listing the drawer's contents, enhanced into the drawer by the script
  (ROB-16). The installed app's Logs shortcut opens the new `/logs` list
  (NAV-72). Signed out, the account control is a "Sign in" link (NAV-43).
  Theme: §11.13.

### 11.11 Click counts: old and new

Old counts are copied from `docs/qa/QA.md`; widths 1440 / 390 px; one click
or tap = 1, typing is not counted. **P** = the path is built in the
prototype and was clicked through; **paper** = counted from this design only.

**Logs from each place a container is named (counted from the element):**

| Place | Old | New | New path | Built |
|---|---|---|---|---|
| Overview "Needs attention" container row | 2 / 2 | **1 / 1** | logs icon at the end of the row | P |
| Overview "Needs attention" alert row | no path | **1 / 1** | the row opens its subject; a container subject has the logs icon | P |
| Overview Top CPU / Memory lists | 2 / 2 | **1 / 1** | logs icon at the end of the row | paper |
| Overview Recent events row naming a container | not counted in QA.md | **1 / 1** | logs icon; opens the logs at that moment | paper |
| Overview treemap tile, large enough | 2 / 2 | **1 / 1** | the corner logs icon (§11.9) | paper |
| Overview treemap: a tile too small for a name, or inside "+N others" | 2 / 2 | **2 / 2** (exception) | "+N others" (or the small tile) opens the flat table sorted by memory / the detail; then Logs. These containers are not named on the Overview, so the one-click rule cannot attach to anything | paper |
| Containers table | 1 / 1 | **1 / 1** | "Logs", the labelled first action | P |
| Sidebar stack tree (container) | 2 / 2 (3 from Overview) | **1 / 1** (2 from Overview on a phone: More, then the icon) | logs icon on the item | P (1440) |
| Sidebar stack tree (whole stack) | 1 / 1 | **1 / 1** | stack logs icon, compose projects only | paper |
| Container detail | 1 / 1 (two views) | **1 / 1** | the Logs tab; key `l` | P |
| Settings › Containers and groups row | not counted in QA.md | **1 / 1** | logs icon in the row actions | paper |
| Alerts page | no path | **1 / 1** | logs icon on rows about a container; log rows open the logs at that moment | paper |
| Host "by container" table | 2 / 2 | **1 / 1** | logs icon | paper |
| History "starts per day" and timeline | 2 / 2 | **1 / 1** | logs icon; timeline rows at that moment | paper |
| Command palette (name typed) | 1 (2 from Overview) | **1** (2) | "Logs: name", directly under the container | paper |
| `/logs` page | 2 (3) / 2 (3) | **1** (2) / 1 (2) | the name in the filterable list | paper |

**Top tasks (from Overview, signed in):**

| Task | Old | New | New path | Reason when above 2 | Built |
|---|---|---|---|---|---|
| See why a container is unhealthy | 3 + scroll / 3 + long scroll | **1 / 1** (0 for the one-line reason, signed in) | the attention row shows the reason; (1) the row opens the detail with "Why is this red?" on top. Signed out: the same click shows state and duration, the probe output needs a sign-in | | P (detail header) |
| Add a bookmark | 4 / 5 | **2 / 2** | (1) "Add" in the Overview's Bookmarks header, type, (2) "Add bookmark" or Enter | | P (from Settings: 2 from the tab) |
| Add a widget with one field | 5 to 6 / 6 to 7 | **3 / 3** (2 without the test) | (1) "Add" in the Widgets header; the dialog starts with one empty field row; type id, URL, field name and path; (2) "Fetch now": the extracted value appears next to the field, or "not found"; (3) "Add widget". Each further field: +1 ("Add field") | Fetch now is the test, kept as its own click; like for like with the old 5 to 6, the cut is the Settings detour and the "Add field" click | paper |
| Add a channel and send a test | 5 to 7 / 6 to 8 | **3 / 3** (+1 for Slack or Telegram, +1 for a file or plain URL) | (1) Alerts, (2) "Add channel", type name and variable, (3) "Save and send test": saved, tested, result in the dialog | Channels live on Alerts, one click from anywhere; type and secret source are real choices; the test saves first (§11.13) | P (dialog) |
| Add an alert rule | 5 to 6 / 7 + scroll | **3 / 3**, any number of channels; +1 per channel to untick; **2** from a container | (1) Alerts, (2) "Add rule", type a name, (3) "Add rule". **A new rule notifies every channel that exists** (none: shown on the page only), watches stopped, unhealthy and restarting containers. From a container: (1) "Add rule for this container", (2) "Add rule" | Same as channels | paper |
| Rename a container | 3 / 4 | **3 / 4**, **2** from its page | (1) the container in the sidebar (390: Containers, then the row), (2) "Edit", display name focused, (3) "Save" | Choosing which container is a click; old path left for Settings | P (from its page) |
| Reorder a list item one step | 1 per step (each a save and a revision) | **3** for one step, **n + 2** for n steps, **one revision** | (1) "Reorder" (header, or ⋯ → Reorder…), (2) ↑ per step, (3) "Save order" | One revision and a draft that can be cancelled per arrangement, instead of a write per click (SET-42); §11.13 | P |
| Change the theme (this browser) | 1 to 4 | **2 / 2** | (1) theme button, (2) pick | §11.13 | P |
| Change the default theme | 5 / 6 | **3 / 3** | (1) theme button, (2) "Default for every browser…", (3) pick in Appearance: a single choice, saves on change (§11.1) | It writes the config for every browser | paper |
| Find last night's incident | 1 + scroll; 6 + two typed date-times to the logs | **2 / 2** | (1) Alerts: incidents with clock times and a range picker, (2) "Logs at 02:14". History: same, 390 needs More first (3) | | paper |
| From a firing alert to the cause | no path | **1 / 1** | the firing row opens its subject; the rule name opens its dialog | | P (attention row) |

### 11.12 Removals agreed by the owner

| Removed | Finding |
|---|---|
| Beszel button on container detail (`metrics_url`) | OV-35 |
| "Open in Dozzle" and "Details" row icons in the containers table | OV-20 |
| "Open in Dozzle: X" rows in the command palette (and the docs line) | NAV-53 |
| The Settings index page of cards | SET-22 |
| The Theme select in Settings › General (Appearance keeps "Default theme") | SET-27 |
| Columns and Layout in Settings › General and in groups (and "on cards" wording) | SET-26 |
| The offline page (`/offline`) and its manifest link; no service worker | NAV-70 |

Nothing else is removed without the owner's yes (§11.13).

### 11.13 Changes that need the owner's yes

Each line: the change; why; what not doing it costs. The default for each,
until the owner answers, is **not** to build it; the alternative in brackets
is what gets built instead.

| # | Change | Why | Cost of not doing it |
|---|---|---|---|
| Y1 | Retire the Ember theme (stored `ember` reads as Dark) | Brand Dark is already a warm dark; one fewer theme block to keep at AA | Keep a fourth hex block (≈ 0.9 KB) and its contrast checks; [Ember re-derived from the brand] |
| Y2 | Remove the header "Logs" button and the Logs tab's interstitial; the tab is the viewer | One view of logs, reached one way (OV-33, OV-34) | Two controls named "Logs" that lead to different places; [the tab links to the viewer page] |
| Y3 | Remove the phone hamburger; "More" is the only way into the drawer | Two buttons opened the same drawer (NAV-35) | One redundant 44 px control in the top bar |
| Y4 | Remove "Settings" from the account menu | Already in the sidebar and drawer | One duplicate menu item |
| Y5 | Remove the "Toggle sidebar" palette action | The toggle is always on screen | One palette row |
| Y6 | No staged lists: every add, edit, delete saves on confirm; reorder in a dialog that saves once | One rule for every list (SET-29, SET-30, SET-42) | Hidden containers and Groups keep their separate "Save" and the silent-loss bugs SET-29 / SET-30; [staged lists get a visible "unsaved" bar] |
| Y7 | Lock widget ids and channel names on edit | Rules refer to them; a rename silently breaks rules (SET-49) | Renames stay possible; [the dialog warns and names the rules that refer to it] |
| Y8 | A container result in the palette opens its Hobnail page; "Open service ↗" is its own row | Everywhere else a name opens the detail page (NAV-50) | Enter keeps leaving Hobnail for the service |
| Y9 | Theme button opens a two-click menu instead of a one-click cycle | A cycle of five is blind and up to four clicks (NAV-41) | Keep the cycle (1 to 4 clicks); [show the theme name in a toast after each step] |
| Y10 | Fold treemap tiles too small for a name into "+N others" | Tiny tiles cannot carry a name or a logs icon (OV-09) | Slivers without labels stay; their logs are 2 clicks either way |
| Y11 | Hide the Settings tab strip when signed out (lock panel only) | Nine links that all lead to the same lock panel (SET-21) | Nine dead-end links signed out |
| Y12 | Config changes refetch affected data without document reload | Preserves logs, input, focus and scroll | Implemented in the React live layer. |
| Y13 | Board edit mode uses a private draft, explicit Save and Cancel | One board-document write per editing session (D17) | Legacy config collection arrows still save per click. |
| Y14 | Channel test only after saving ("Save and send test") | No new server route; reuses the existing save and `channels/test` | QA SET-52 asked for a test before saving: that needs a new `channels/test` variant taking a draft, under the same guards and the secret-binding rule of the widget preview |
| Y15 | Channels and rules can be added and edited from the Alerts page as well as Settings | The page that shows alerts is where people act on them (LG-57); 3-click counts depend on it | Channel and rule tasks stay 5 or more clicks via Settings |
| Y16 | Undo after delete restores the revision before the delete (refused like any write if the file changed since) | Deletes are cheap to reverse; revisions exist | No undo; the confirm is the only safety |
| Y17 | `/settings` opens the last used tab (the script rewrites the nav link; the server opens General) | Saves a click on repeat visits | `/settings` always opens General |
| Y18 | Superseded by React: lists and values come from authenticated Settings JSON | No Settings data for Viewers | JavaScript required (D6). |
| Y19 | "Fetch now" shows the raw upstream response so fields can be picked from it (SET-46) | Finding paths without another tool | **Not designed.** The preview keeps returning `html` and `values` only (`src/settings/api.rs`): no response body reaches the browser. Showing it is a widening of what the app reveals and needs its own decision |
| Y20 | Retire the old token names, keeping aliases (§11.2) | Clear names in the contract | Without aliases an existing `custom.css` breaks |
| Y21 | Single-control settings save on change (Default theme, System tiles, Recording on/off, rule on/off) | A switch with its own Save does nothing until Save (SET-50) | Keep a Save per section |

**Owner's answers (2026-11-15). These override the table and anything else in
section 11 that says otherwise.**

- **Yes, build as written:** Y2, Y3, Y4, Y5, Y6 (except reorder, see Y13), Y8,
  Y10, Y11, Y14, Y15, Y16, Y17, Y18, Y20, Y21.
- **Y1 no:** the Ember theme stays, re-derived from the brand and held to the
  same contrast checks as the other themes.
- **Y7 no:** ids and names stay editable. The dialog warns before saving a
  rename and names the rules that refer to the old one.
- **Y9 no:** the theme button keeps its one-click cycle and shows the name of
  the theme it switched to. Change the theme: 1 click per step.
- **Y12 superseded by React:** open views refetch changed data; nothing reloads.
- **Y13 superseded for boards by D17:** draft, Save and Cancel. Legacy config
  collection arrows still save one revision per step.
- **Y19 no:** the preview keeps returning `html` and `values` only. SET-46
  stays open.
- **Budget:** idle RAM is the budget that matters most. The binary and image
  may grow (up to the 20 MB image target in GOALS.md) when that buys function.

### 11.14 Server changes (historical, superseded by the JSON API)

The following table records the retired UI contract, not current routes.
Those pages and fragments were deleted in M4; `/` returns 404 with a link
to `/app`, no redirects or RH_UI switch. The old UI is on `rust-only`.
Every server-rendered addition or new GET variant this design needed. "Session"
means: rendered only for a valid session; without one the page renders as
today for a visitor (lock panel or no control) and **no settings data of any
kind** (widget URLs, environment-variable names, channel or rule settings,
any form value, `data-item`) is in the HTML.

| Change | Access |
|---|---|
| `?add=<kind>` and `?edit=<kind>:<id>` on every Settings tab: the same form rendered filled, as the no-JS fallback and the deep link (Y18) | session |
| Settings lists rendered on the server (Y18) | session |
| `?frag=list` (per section) returning a Settings list fragment for in-place updates. **Unlike the containers-table fragment**, whose handler builds its context with an empty `HeaderMap` (`src/ui/routes.rs:39-45`, i.e. always the signed-out view), Settings fragments must read the session from the request and answer 401 without one | session |
| Dialog markup and per-row `data-item` / `data-key` on Overview (Bookmarks and Widgets "Add", attention widget rows), Alerts (channels, rules, "Add rule"), container pages (override dialog, "Add rule for this container") | session |
| Health check panel and probe output on Overview and detail (§11.10) | session (unchanged rule) |
| Logs tab route rendering the container header and tabs above the viewer (Y2) | as logs today (`[logs] require_login`) |
| `/logs` as a filterable list grouped by stack | as `/logs` today |
| A plain "More" page listing the drawer's contents | public |
| Alerts page: links to subjects, rule names, clock times, "Logs at" | public (what is shown today); editing controls session |
| History timeline: range and zoom parameters, paging with `before=` | public (auth events filtered as today) |
| Overview attention row reason and Recent events logs links | public (state and duration only) |
| Per-field 422 answers: `field` = the form's field name, plain message, no line / column, no deserialiser text (SET-06, SET-07, SET-55) | session (write routes) |
| `/api/palette` action items (Add bookmark, Edit X, …) | session only; visitors get today's list |
| `/settings` opens General without the index (agreed) | public (lock panel) |
| No new write route. "Save and send test" is the existing save then `channels/test`; Undo is the existing restore; reorder is the existing `reorder` action with the full order; the 401 recovery reads the new token from a same-origin GET of the current page | session (unchanged guards) |

### 11.15 Cost

Measured from the prototype's SHIP part after the review fixes:

| Part | Raw | Stripped | gzip |
|---|---|---|---|
| CSS: theme blocks (dark, light, contrast, OS-light copy) | 3.5 KB | 3.5 KB | 1.0 KB |
| CSS: all tokens incl. aliases and media blocks | 6.0 KB | 5.0 KB | 1.7 KB |
| CSS: base and components | 21.6 KB | 20.1 KB | 4.8 KB |
| **CSS total** | **27.6 KB** | **25.1 KB** | **6.3 KB** |
| **JS total** (dialog, validation, toast, confirm, menu, tabs, reorder, list helpers) | **25.6 KB** | **21.6 KB** | **7.2 KB** |

It replaces the theme blocks, buttons, pills, banners, empty and skeleton
styles in `app.css`, the form and toast styles in `settings.css`, and the
editor, note, toast and field-error code in `settings.js`; wave 1 measures the
real delta with `scripts/measure.sh`. Until compression lands (ROB-04, bench
phase) every inlined CSS byte is paid on every page.

**Not in the figures above (estimates):**

- **Dialog markup on pages** (signed in only): bookmark 2.7 KB, channel 6.4
  KB, container override 3.3 KB, reorder 1.2 KB, confirm 0.6 KB (whitespace
  collapsed; 0.3 to 1.6 KB gzip each). Rule and widget dialogs, not built in
  the prototype, are estimated at 8 to 10 KB each. A Settings tab carries one
  or two dialogs; Overview signed in carries bookmark and widget (≈ 12 KB);
  a container page the override dialog (3.3 KB).
- **Per-row `data-item` JSON**: ≈ 200 bytes per bookmark row, 0.3 to 1 KB per
  widget, channel or rule row; signed in only.
- **Historical, superseded in M4:** server-rendered form pages and lists (Y18) went into the binary as maud
  code: estimated 15 to 30 KB of binary for the nine tabs' forms and lists,
  most of which replaces markup that is already there; to be measured in
  wave 2.
- The review fixes added ≈ 6 KB raw (≈ 1.3 KB gzip) of script over the first
  draft (cancel / close handling, the URL-is-the-dialog logic, reorder,
  menu-from-link, reveal-before-focus).

### 11.16 What this design closes, and what it leaves

Of the 267 findings, 128 are closed by the components, rules and flows above
(some only if the owner says yes to the matching §11.13 item, noted below), 4
are left to the bench phase, and 135 are wave 5 work that needs no further
design decision.

- **Components and validation:** SET-01 to SET-20, SET-24, SET-25, SET-28 to
  SET-35, SET-37 to SET-43, SET-45, SET-47 to SET-50, SET-52 (Y14), SET-53,
  SET-54, SET-56, SET-58, SET-61 to SET-65, SET-68, SET-70, SET-71, ROB-11,
  ROB-15, A11Y-22. (SET-29, SET-30: Y6; SET-42: Y13; SET-49: Y7; SET-50:
  Y21.)
- **Navigation, logs, flows:** NAV-05, NAV-07, NAV-11, NAV-15, NAV-33,
  NAV-41 (Y9), NAV-43, NAV-50 (Y8), NAV-51, NAV-52, NAV-55, NAV-72, OV-01,
  OV-04, OV-12, OV-23, OV-33 (Y2), OV-34, OV-36, OV-38, OV-39, LG-01, LG-03
  (Y12), LG-04, LG-09, LG-12, LG-43, LG-44, LG-47, LG-48, LG-55, LG-56, LG-57
  (Y15), A11Y-01, A11Y-03, ROB-12, ROB-16.
- **Removals:** SET-21 (Y11), SET-22, SET-26, SET-27, NAV-35 (Y3), NAV-53,
  NAV-70, OV-20, OV-35, OV-48.
- **Tokens, accessibility, empty and loading states:** A11Y-04, A11Y-06,
  A11Y-08, A11Y-11, A11Y-12, A11Y-13, A11Y-15, A11Y-17, A11Y-18, A11Y-19,
  A11Y-20, SET-23, OV-24, OV-61, LG-14, LG-15, LG-20, ROB-01, ROB-02, ROB-03
  (`.sr` with the clip pattern and `margin:-1px`), ROB-07, ROB-08, ROB-14.
- **Bench phase:** ROB-04, ROB-05, ROB-10, LG-18.
- **Not closed by this design:** SET-46 (needs Y19, not designed). Wave 5,
  majors first: NAV-21, OV-02, OV-19, OV-21, OV-40, LG-06, LG-07, LG-13,
  LG-19, LG-34, LG-35, LG-36, LG-45, LG-46, LG-58, A11Y-02, ROB-13, ROB-18;
  then: SET-36, SET-44, SET-51, SET-55, SET-57, SET-59, SET-66, SET-67,
  SET-69, NAV-22 to NAV-27, NAV-31, NAV-34, NAV-40, NAV-44, NAV-54, NAV-56 to
  NAV-58, NAV-60 to NAV-64, NAV-73, NAV-80 to NAV-82, OV-03, OV-05 to OV-07,
  OV-09 to OV-11, OV-13 to OV-18, OV-22, OV-25 to OV-30, OV-32, OV-37, OV-41
  to OV-47, OV-49 to OV-60, OV-62, LG-05, LG-08, LG-10, LG-11, LG-16, LG-17,
  LG-21 to LG-33, LG-37 to LG-42, LG-50, LG-51, LG-52, LG-54, LG-59 to LG-62,
  A11Y-05, A11Y-09, A11Y-10, A11Y-14, A11Y-16, A11Y-21, ROB-06, ROB-09,
  ROB-17. (OV-31 and LG-53 do not appear: they were merged into A11Y-18 and
  OV-07 in QA.md.)
