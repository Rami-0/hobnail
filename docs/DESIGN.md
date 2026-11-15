# Design v2: UI, history and in-app settings

Status: **implemented** (V1 to V4, 2026-11-14). Written as the spec after M0 to
M5; where the implementation showed the spec was wrong or incomplete, the text
below was fixed and the change is listed in section 9. It is the contract the
implementation work is built against. When the code and this file disagree, fix
whichever is wrong.

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
binary, no Node, no frontend build step, nothing leaves the machine unless the user
configured it.

## 1. Decisions

| Topic | Decision | Reason |
|---|---|---|
| UI technology | Server-rendered `maud` + hand-written CSS + ~15 KB of vanilla JS. **No** Leptos/Dioxus/Yew, no Tailwind, no chart library. | WASM UI frameworks ship a 0.5 to 2 MB bundle and a heavy toolchain, and they do not fix a design problem. Pages already render in under a millisecond. |
| Charts | Inline SVG, drawn on the server from database series. | Zero JS dependency, works with the existing SSE swap, a few hundred bytes per chart. |
| Icons | One inline SVG sprite of ~30 hand-picked line icons. | No icon font, no CDN. |
| Fonts | System stacks only, `tabular-nums` for every number. | Works offline, no download. |
| Database | SQLite via `rusqlite` (bundled), WAL mode, page cache capped at 2 MiB. | Range queries and rollups are exactly what it is good at. Costs about 1 MB of binary. |
| Config editing | Lossless edits with `toml_edit` (keeps comments, order, unknown keys), validated with the existing `config::parse` before every write. | A form save must never destroy the user's hand-written comments. |
| Auth | **Viewing stays open** (network-level, as today). **Any write needs a login.** | See section 6. An editor that anyone on the LAN can use is a remote-code-execution hole in this container. |
| Command widgets | Can only be created or changed **in the file**, never from the app. | They run shell commands. The app shows them read-only with a lock icon. |

## 2. Visual language

Reference points: Dozzle (layout, density, dark default), Linear and Grafana
(restraint, chart style). Not Homepage.

### Tokens (CSS variables; user `custom.css` can override all of them)

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

Server-rendered full pages; navigation is plain links (fast enough that a SPA
buys nothing). The existing SSE channel keeps swapping fragments for live data.

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

**Read API (JSON, no auth, same open model as the page)**

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
- Secrets are never shown or stored by the app. Forms hold an env var name.
- Restoring a revision creates a new revision; nothing is deleted from history.
- Every change is an event in the timeline ("config saved by admin: widgets").

## 6. Security model

The old model (no login) was acceptable because nothing could be changed from the
browser. Once the app can write config, that stops being true, because a
`command` widget runs shell commands inside this container, which has the Docker
socket. So:

| Rule | Detail |
|---|---|
| Viewing | Open to anyone who can reach the port (LAN and tailnet only; not `0.0.0.0` on the host). Unchanged. |
| Any write | Requires a valid session. All `POST`/`PUT`/`DELETE` routes check it; there is no unauthenticated write route. |
| First-run | No password is set until the owner runs `docker exec -it hobnail /hobnail set-password` (reads from the TTY without echo, or from stdin when piped). Until then `/settings` shows how to do this and nothing is writable. No web "claim" form, so nobody can race the owner. |
| Password storage | PBKDF2-HMAC-SHA256 (`ring`, already compiled in via rustls), at least 600,000 iterations, random salt, constant-time compare. Hash in the `kv` table. Never logged. |
| Sessions | 32 random bytes, only the SHA-256 is stored. Cookie `rh_session`: `HttpOnly; SameSite=Strict; Path=/; Max-Age=7d`, revocable. The server-side expiry slides; the browser cookie keeps its 7-day `Max-Age` unless a page calls `auth::refresh_cookie`. (`Secure` cannot be set over plain HTTP; the tailnet is already encrypted by WireGuard.) |
| CSRF | `SameSite=Strict` **and** a required `X-RH-CSRF` header on every write **and** `Origin`/`Host` check. |
| Brute force | Per-IP and global back-off after failed logins (1 s, 2 s, 4 s ... capped at 60 s), failed logins logged as events. Clients that already signed in successfully since the app started skip the *global* back-off (they keep their per-IP one), so a stranger cannot lock the owner out with one wrong guess a minute. |
| Command widgets | Cannot be created, edited or enabled from the app. The editor refuses a raw-config save that adds or changes a `command` widget, or sets `enabled = true` on one, unless it already existed identically. They are managed in the file. |
| Secrets (bound) | The app uses secrets but can never read them. A new `*_file` path from the app must be inside `{data_dir}/secrets/` (also after resolving symlinks; a denylist missed `/host/proc/self/environ`). A widget or webhook with secret headers, and a Telegram channel with a custom `api_base`, cannot get a new destination or new secret references from the app unless that exact pair is already in the file; the widget preview follows the same rule. |
| Login history | `auth.*` events (failed logins carry the client IP) are returned by `/api/events` and shown on pages only to a signed-in session. |
| `allow_hosts` | Editable (signed in), with a warning that widening it enables outbound requests to more hosts. |
| Output | The app never prints secrets: not in pages, JSON, logs or revisions (revisions store the config, which only ever holds env var names). |
| Enforcement | One gate in `http::server::route`: every non-`GET`/`HEAD` request must pass `auth::require_write` before reaching any module (only `POST /api/login` is exempt and checks `Origin` itself). Tested end to end for every write route in `src/http/gate_tests.rs`. |

## 7. Module contract (so parallel work merges cleanly)

New code lives in new files wherever possible.

```
src/db/mod.rs      Db handle, schema, migrations, record_* / queries, maintain()    (storage work)
src/db/api.rs      JSON read API handlers: /api/history/*, /api/events               (storage work)
src/auth.rs        password hash, sessions, rate limit, set-password CLI, login API  (storage work)
src/ui/shell.rs    page shell: sidebar, top bar, palette, icon sprite                (UI work)
src/ui/pages/*.rs  overview, containers, container detail, history, alerts, login    (UI work)
src/ui/chart.rs    SVG sparkline and line chart from a series                        (UI work)
src/settings/*.rs  toml_edit patching, revisions, settings API                       (settings work)
src/ui/settings/*  settings pages                                                    (settings work)
static/*.css|js    new design system                                                 (UI work)
```

`AppState` gets `pub db: Arc<db::Db>`. `db::Db` is a synchronous handle (internal
mutex around one connection). Async callers wrap read queries in
`tokio::task::spawn_blocking`; the sampler's 15 s batch write is small enough to
run inline. The public API is fixed in `src/db/mod.rs` (signatures only until the
storage work lands). The UI must compile and render against the stub, showing an
empty chart state, and fill in once the real implementation is merged.

## 8. Quality bar and how it is checked

- `cargo fmt --check`, `clippy -D warnings`, `cargo test` gate every build (as now).
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
