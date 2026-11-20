> Historical: this file describes work before the React port. Current UI and
> installation contracts are in README.md, docs/DESIGN.md and docs/install.md.

# Progress

**URL:** <http://100.125.149.51:3200> (Tailscale) · <http://192.168.8.200:3200> (LAN)
Stack: `/opt/stacks/rust-homepage/compose.yaml` · legacy live deployment (see README before redeploying)
The footer shows the build SHA, build time and uptime of what you are looking at.

## M0 — Skeleton ✅ (deployed as `51eed1d`, then `11bce8a`)

Works (verified on the deployed stack):
- Static styled page from a `scratch` container on port 3200, published only on
  192.168.8.200 and 100.125.149.51 (`ss -ltn` shows no 0.0.0.0:3200).
  `curl` against both addresses: `/healthz` → `ok`, `/` → page.
- Config: `/opt/stacks/rust-homepage/data/config.toml`, missing file = defaults.
  Hot reload by polling mtime+size every 2 s; a broken edit keeps the last good
  config and shows `file:line:col: message` in a banner. Unit-tested.
- Footer: version, git short SHA, build time, live uptime, config generation.
- Live updates: one SSE connection per open page (`/api/stream`); fragments are
  re-rendered server-side and swapped in. A new build makes open pages reload.
- Docker healthcheck via `rust-homepage healthcheck` (no curl in scratch).
- Build gate: `cargo fmt --check`, `cargo clippy --all-targets -D warnings`,
  `cargo test` all run inside the Docker build stage.
- Host allowlist against DNS rebinding (unknown `Host` → 421).

Measured: idle 0.55 MiB, image 5.16 MB, app cold start ~20 ms, container cold
start ~287 ms (over target, Docker's own networking setup; see
docs/measurements.md).

## M1 — Docker cards ✅ (deployed as `4ff11d0`)

Built by a subagent (docker client, cards) and integrated by me (auto icons).
Verified on the deployed stack with **no config file**:
- `/api/state` vs `docker ps -a`: 36/36 containers, name, state, health and
  compose project all identical.
- 10 groups (one per compose project + "Standalone" for the 6 without one),
  36 cards. Collapsible groups, remembered per browser.
- All 53 unique links on the page answer (`curl`): 10×200, 37×307 (Dozzle
  login redirect), 4×303 (Odoo login redirect), 1×401 (Mailpit with auth).
  Ports on 127.0.0.1+100.x link to the Tailscale IP; 0.0.0.0 ports use the
  host the page was opened on; loopback-only ports show "local"; Postgres/SMTP
  ports are plain badges.
- Logs (Dozzle) and metrics (Beszel) links auto-detected.
- Docker events stream: a throwaway container's start → exit appeared within
  ~0.3 s (tested by the agent with its own container).
- Per-container CPU/memory from cgroup v2 files (no Docker stats API calls);
  memory matched `docker stats`.

Measured: idle 0.96 MiB, 1 viewer 1.0 MiB, image 5.52 MB.

Not done: metrics links all point to the Beszel home page (Beszel has no
per-container URL to deep-link to).

## M2 — System widgets ✅ (deployed as `3041663`)

CPU, memory, disk, network, temperature and uptime tiles, read from host
`/proc` and `/sys` (read-only mounts). Verified on the deployed stack against
the host and against Homepage's own API (GET only):

| | Host | rust-homepage | Homepage |
|---|---|---|---|
| CPU | load 4.90 | 48.5 % | 49.7 % |
| Memory used | `free`: 3,855,159,296 B | identical | 6.27 GB (counts buffers/cache) |
| Disk `/` | `df`: 468.19 GB, 20 % | 87,337,168,896 / 468,191,526,912 B, byte-exact | 468.19 GB, 19.55 % |
| Temperature | coretemp 61 °C | 61 °C | 61 |
| Uptime | 1,822,076 s | identical | identical |
| Network | enp1s0 | enp1s0 rates | Homepage's *own container* eth0 |

Differences from Homepage are deliberate: memory "used" is total − available
(like `free`), and network is the host uplink, not the dashboard container.

## M3 — Custom widgets ✅ (deployed as `3041663`)

Live on the page (verified via `/api/state` and the HTML):
- Beszel `/api/health` → "API is healthy." with a green status dot (3 ms)
- Mailpit `/api/v1/info` → unread/messages, DB size, uptime, version (15 ms)
- Odoo 20 `/web/health` → "PASS" with a green dot (56 ms)
- Command widget (busybox `sh` in the scratch image) → host process count
- A deliberately broken widget (port 9) shows "timed out after 2000ms" in its
  own tile; everything else keeps working.
Command widgets only run with `enabled = true`. Header secrets come from env
vars/files and never appear in errors or logs.

## M4 — Notifications ✅ (deployed as `1c6c8f5`)

Tested end to end on the deployed stack against a local mock receiver
(`rust-homepage-mock`, compose profile `test`) with fake secrets in
`/opt/stacks/rust-homepage/secrets.env`. Test container `rh-alert-test`
(created by me, the only container I stopped):

| Step | Expected | Seen |
|---|---|---|
| `notify-test webhook/slack/telegram` | one message each | ✅ `ok` ×3, Telegram token redacted in the mock |
| `docker stop rh-alert-test`, wait 40 s | 1 firing per channel after `for = 5s`, no repeats | ✅ 3 messages at +7 s, none after |
| `docker start` | 1 recovery per channel | ✅ "✅ rh-alert-test: recovered after 40s" ×3 |
| stop again inside the 2 min cooldown | suppressed | ✅ 0 messages for 25 s |
| …still stopped when cooldown ends | notified once | ✅ 3 messages at 14:09:01 |
| `docker restart rust-homepage` with the incident open | no re-send (state file) | ✅ 0 messages |
| start | 1 recovery per channel | ✅ 3 messages |
| channel returning HTTP 500 | visible as failing, message queued | ✅ panel "1 failed · 1 queued", `webhook-down failing` |

Also: Slack text is escaped (`& < >`) so container names cannot inject
mentions; secrets never appear in logs, `/api/state`, the page or
`data/alerts-state.json`. Idle RAM with alerting active: 1.28 MiB.

Not tested live: real Slack/Telegram (no credentials, by design). Widget-
threshold and host-threshold rules are covered by unit tests only.

## M5 — Theming and polish ✅ (deployed as `221dffe`)

Checked in a real browser (headless Chromium in a throwaway container; the
T3 preview browser is blocked by AppArmor on this host):
- Dark and light follow the OS (`prefers-color-scheme`); the ◐ button cycles
  auto → dark → light and remembers it per browser, applied before first paint.
- `data/custom.css` is loaded last: setting `--accent` changed the computed
  accent to `#2f81f7`. Only linked when the file exists.
- Icons: dashboard-icons fetched once (opt-in `[icons] remote = true`) and
  cached in `data/icons/di/` (8 cached); built-in glyphs; letter avatars; 24
  images on the page, 0 broken. Served SVGs carry a CSP.
- Keyboard quick-launch: `/` focuses the search, typing filters cards
  ("mailpit" → the 2 Mailpit cards), Enter opens the first match, otherwise a
  web search; Esc clears.
- Phone (390 px): 2-column tiles, 1-column cards, no horizontal scroll.
- Two bugs found and fixed by this check: quick-launch hid the whole page
  (attribute clash), and an open SSE stream stalled later navigations ~20 s.

## v2 — how it was built

Owner feedback after M5: the UI looked like Homepage, config only changeable on
the console, no history, docs. Plan: [docs/V2-PLAN.md](docs/V2-PLAN.md), spec:
[docs/DESIGN.md](docs/DESIGN.md). I put a shared contract on `main` first
(central router with one write gate, per-module route stubs, final DB/auth
signatures, all new crates) and then ran four subagents in separate worktrees
(storage+auth, UI, settings engine, settings pages) with disjoint files, merged
them, and fixed what integration and security review found. The two stopped
drafts in `rust-homepage-wt/` were read for reference (the storage draft's
DB/sampler code was reused after rewriting and testing; nothing from the UI
draft's `safe_next`, which was an open redirect) and removed.

## V1 — Storage, history, login ✅ (deployed as `41aa05f`, current `494ad23`)

- SQLite (`data/rust-homepage.db`, `RH_DB`), WAL, 2 MiB cache, forward-only
  migrations; 15 s sampler with its own counters; rollups 1 min / 10 min,
  retention, hourly maintenance; events for docker lifecycle, alerts, config,
  logins, app start; `/api/history/*`, `/api/events`.
- Login: PBKDF2-SHA256 600k, sessions (hash only), CSRF header, Origin check,
  per-IP + global back-off, `set-password` over `docker exec` (TTY without echo
  or stdin), redirect check (`safe_next`) with the security review's cases.
- Verified on a throwaway container by the agent (samples every 15 s for 38
  containers, every metric × range, events for a victim container's
  stop/die/health, restarts keep data, each login/write denial status) and by
  me on the live stack (samples, events, charts drawing from real data).
- Integration fixes by me: anonymous viewers never see `auth.*` events
  (failed-login IPs; security review); login back-off moved into `AppState`;
  ring optimised in dev builds (logins took ~5 s unoptimised).

## V2 — New UI ✅ (deployed as `41aa05f`, current `494ad23`)

Sidebar with stack tree, dense container table (filter, sort, group by stack or
flat), container detail, History, Alerts, command palette, SVG charts, phone
drawer. Checked in headless Chromium at **1440, 900 and 390 px, dark and
light**: the UI agent reviewed every page; I reviewed (opened and looked at)
Overview 1440 dark (twice: fresh and with an hour of data), Overview 390 light,
Containers 1440 dark and 390 dark, container detail 1440 light, History 900
light (twice), Alerts 1440 light, Login 390 dark, 404 900 dark, Settings
signed-out 1440 dark, and the settings agent's signed-in Widgets 1440 dark,
Config file (locked refusal) 1440 dark, revision diff 1440 light,
Notifications 390 dark. Fixed after looking: chart y-axis labels clipped
("KiB/s"), chart times shown in UTC (now the viewer's local time), network tile
showing "0 B/s" on load, container CPU/memory "—" on first paint (now from the
newest DB sample). Contrast (agent's computation): body text ≥ 14.9:1 dark /
≥ 15:1 light, secondary ≥ 6.5:1, tertiary ≥ 4.66:1, controls ≥ 3.19:1.

Missed: CSS+JS 72.6 KB (JS ~30 KB vs ~15 KB in DESIGN); first paint of
`/containers` 164 ms (target < 150 ms). `page.columns` no longer has an effect.

## V3 — Settings in the app ✅ (deployed as `d42efb8`, current `494ad23`)

Nine tabs (General, Containers and groups, Bookmarks, Widgets, Notifications,
Appearance, History and data, Config file, Security) on a JSON API under
`/api/settings/`. Lossless `toml_edit` edits, validation, stale-file refusal,
atomic writes, revisions with diff and restore, validate-as-you-type.
- `command` widgets: refused on every path (form, raw editor, restore); shown
  read-only with a lock. Tested as a matrix; refused live in the browser.
- Secrets: env var names only, set/not-set shown; **destination binding** and
  the `data/secrets/` allowlist added by me after a security review (a stolen
  session could otherwise point `RH_TELEGRAM_TOKEN` at its own server, or read
  `/host/proc/self/environ`).
- `src/http/gate_tests.rs` (mine): for 27 write routes — refused before a
  password exists, 401 without session, 403 without/wrong CSRF, 403 foreign or
  missing Origin, passes only with all three; every settings GET needs a session;
  login refuses a foreign Origin and a TAB-smuggled `next`.
- Live evidence (agent, throwaway container with a copy of the real config):
  a save kept all 26 comment lines byte-identical; a hand edit between load and
  save gave the stale banner and wrote nothing; a raw-editor command widget was
  refused; a custom-api widget was added with Fetch now and appeared on the
  overview; restoring revision #1 made the file byte-identical to the original.
- **Not tested against the owner's live config**: no password is set on the
  live instance (that is the owner's step), so the live settings are locked.
  Backup made anyway: `data/config.toml.bak-20261114T151143Z`.

## V4 — Polish, measure, docs ✅ (`494ad23`)

Measured (docs/measurements.md, v2 section): idle **1.14 MiB**, one viewer
**1.65 MiB**, image **9.61 MB**, sampler + background work **0.13 %** of a core
(~20 ms per sample), DB **2.39 MiB after 1.3 h** (projected 30–35 MiB steady;
not yet observed for 24 h), pages 3–10 ms. **Missed:** container cold start 281
ms (< 200 ms; Docker's start, app ready in ~30 ms), first paint of
`/containers` 164 ms (< 150 ms), JS size. Docs: README, GOALS status,
DESIGN (section 9 lists every correction), docs/features/*.md. 270 tests.

## Wave 1 (v3) — logs, deep detail, host page, design pass ✅ built, deployed as `1491888` — **design awaiting your sign-off**

Plan: [docs/V3-PLAN.md](docs/V3-PLAN.md). Design review first:
[docs/design-review/README.md](docs/design-review/README.md) (the 10 things that
looked "default"), decisions in [DESIGN.md §10](docs/DESIGN.md). Then a shared
contract on `main` (route stubs, migration slots v3–v5, sampler hooks) and four
subagents in worktrees; I merged, wired and reviewed.

1. **Logs in the app** (`/containers/<n>/logs`, `/stacks/<s>/logs`, `/logs?c=a,b`):
   live tail over SSE only while viewed (Docker stream closed 53 ms after the
   viewer leaves), multiplexed/TTY parsing, ANSI, JSON, levels, search/regex
   (5–16 ms over 5,000 lines), time ranges, download, permalinks, stack and
   split views; `log_match` alert rules with bounded background tails (≤ 20
   containers); per-minute level counts (no text stored). `[logs]
   require_login` defaults to **true** (fail closed, after a security review):
   logs are locked until you set a password, or set it to `false` to open them.
   Hidden/excluded containers cannot be tailed.
2. **Container detail** with Overview / Logs / Metrics / Inspect / Processes /
   Events tabs: CPU with throttling, memory/cache/limit/OOM, network, block
   I/O, PIDs, restarts with history; inspect with env **keys only** for
   anonymous viewers (values via a signed-in API), last 5 health-check outputs;
   "why is this red" line. Security review: command lines, health-check
   commands/output, bind-mount sources and process lists now need a session
   too (tested).
3. **/host**: per-core, CPU split, PSI, memory detail and swap activity,
   per-device I/O and %util, file systems, per-interface rates/errors/drops,
   all hwmon sensors, CPU/RAM by container and top processes mapped to
   containers. Not possible without privileges (not added): S.M.A.R.T., GPU
   busy %, systemd failed units, used/free of mounts other than `/`. Found:
   **`enp1s0` negotiates 10 Mb/s** — worth checking the cable/switch.
4. **Design pass**: type scale with calm zeros, panel hierarchy, **24 h health
   strips** (table, sidebar, stacks, Overview), linked crosshair, drag-zoom,
   event markers, yesterday line, avg–max band, legend toggles, memory treemap,
   ranked "Needs attention", "since you last looked", widget tiles, skeletons,
   number tweens, "updated N s ago", first-paint CPU seeded from the DB, keyboard
   map (`g o/c/h/l/a/s`, `j/k`, `/`, `?`, `l`), tab/favicon alarm badge, four
   themes (Dark, Light, Ember, High contrast; contrast test), installable PWA
   (manifest, icons, offline page, no data caching) and a phone bottom bar.

Verified by me on a release build: 345 tests; screenshots of every page at 1440
/ 900 / 390 dark and light reviewed (Overview, Containers, container Metrics,
Logs, /host, phone Overview and /host among them; fixed: container header
"Metrics" button renamed "Beszel"); live: CPU seeded on first paint, first
paint 152 ms over Tailscale, phone scroll test 0 px. Budgets: all met
(docs/measurements.md, "v3 Wave 1"); watch items: page size +35 %, container
extras sampler cost.

**Not done / open:** multi-line tracebacks are not grouped in logs; a stopped
container's log page does not resume on restart without reload; no memory
limit *line* on charts (a "% of limit" chart instead); a stack-level page is
Wave 2; red event markers on host charts include crashes of the agents'
throwaway test containers (real history, ages out).

**For you to judge:** the look (before/after screenshot sets listed in the
report), the Ember theme, whether logs need a login, whether to keep the
Dozzle row button (kept, also "Open in Dozzle" in the palette), the 10 Mb/s link.

## Final state

- URL: <http://100.125.149.51:3200> (Tailscale), <http://192.168.8.200:3200> (LAN).
- Live config unchanged by v2 (5 widgets, 2 page-only alert rules, icons on);
  history database recording since 16:14Z.
- **Read-only until you run** `docker exec -it rust-homepage /rust-homepage set-password`.

## Before the M6 cutover (needs your sign-off; nothing here is done)

1. Set the password (above), sign in, look around Settings.
2. Real notification credentials in `secrets.env`, channels in Settings →
   Notifications, **Send test** each; decide which rules page you.
3. Migrate from Homepage, now possible in Settings: bookmarks (Operations,
   Documentation, References from `bookmarks.yaml`), the non-container cards
   ("Host & Access": SSH, Samba; "Run Dashboard Watchdog") as bookmarks, and
   display-name / group overrides for the hand-made groups you want to keep.
4. Parity walk-through together (header, resources, per-container stats, site
   monitors → `type = "http"` alert rules or custom-api status dots; Homepage's
   `custom.css`/`custom.js` not ported).
5. Check first paint from your phone and laptop.
6. Retire `homepage_dashboard_watch.py` (note: it restarts Homepage whenever it
   rewrites `services.yaml`, which happened after my first deploy) and stop the
   Homepage stack; optionally move rust-homepage to port 3000. **Not done; not
   touched: Homepage, Dockge, Dozzle, Beszel.**
7. Optional cleanup: `docker rmi rh-dev`, `docker volume rm rh-cargo-home
   rh-target-rust-homepage`, `docker builder prune` (the next build then takes
   ~6 min instead of ~2).

## Decisions I made (please review)

| Topic | Choice | Why |
|---|---|---|
| HTTP server | `hyper` directly, no axum/tower | 6 routes; smaller binary, fewer allocations. |
| Runtime | tokio `current_thread`, max 2 short-lived blocking threads | RAM is the headline goal; one thread is plenty. |
| Config reload | poll mtime/size every 2 s instead of `notify` | No extra thread; inotify misses rename-replaced files on bind mounts. |
| Logging | own 40-line JSON-lines logger instead of `tracing` | Size; we log a few lines. |
| Templates | `maud` (compile-time) for pages; tiny own `{{field \| filter}}` engine for user widgets | Zero runtime cost; user templates must not run code. |
| Docker client | own GET-only client over the unix socket (hyper) instead of `bollard` | Only 3 endpoints needed; read-only by construction. |
| Outbound HTTP/TLS | hyper + rustls (ring) + compiled-in webpki roots, built lazily on first https call | No CA bundle in the image, no cost if only http targets. |
| Image | `scratch` + static binary + static busybox (`/bin/sh`, ~1 MB) | The command widget needs an interpreter. |
| Listen | binary defaults to `127.0.0.1:3200`; compose sets `0.0.0.0:3200` *inside the container netns*, host publishes on the two IPs only | Matches the spec "never 0.0.0.0 by default". |
| Container user | `1000:1000` + group 982 (docker) for the read-only socket; `read_only`, `cap_drop: ALL`, `no-new-privileges` | Least privilege. |
| DNS rebinding | Host header must be an IP, `localhost`, `codexdev` or `codexdev.*.ts.net` (`RH_ALLOWED_HOSTS`) | No auth in v1 (D3). |
| Command widget shell | busybox `sh` only; no `curl`/`jq` in the image | Keeps the image at 6 MB. A script needing more tools can be a static binary in `data/`. |
| Alert defaults | container rules `for = 30s`, `cooldown = 15m`; an existing problem fires once when a rule is first added | Flap guard for `compose up` recreations; one message per real incident. |
| Disk tile and disk alert | df-style Use% (used / (used + avail)) | Matches `df`. |
| Icons | dashboard-icons fetched from jsdelivr **only** because the deployed config sets `[icons] remote = true` (your Homepage already uses that CDN) | Delete that line to be fully offline; cached icons keep working. |
| Live updates | the stream closes when a tab is hidden >10 s or left | Background tabs don't keep the server busy. |
| Browser checks | headless Chromium in a throwaway container (image removed afterwards) | The T3 preview browser needs `sudo` to be allowed on this host. |
| v2 UI technology | Server-rendered maud + hand-written CSS + vanilla JS, SVG charts drawn on the server | Decided in the plan; no framework, no build step. |
| History sampler | Always on, every 15 s, own counters | History needs it with nobody watching; measured 0.13 % of a core. Turn off with `[history] enabled = false`. |
| Write gate | One check in the router for every non-GET request, plus an end-to-end test per route | No write route can forget it. |
| Password | Set only on the host (`set-password`); no web first-run form | Nobody on the LAN can claim the instance first. |
| `command` widgets | File-only on every path; deleting from the app allowed | Remote command execution otherwise. |
| Secrets in settings | Names only; new secret files only in `data/secrets/`; secrets bound to their destination | Security review: a session must not be able to exfiltrate a token. Cost: widgets with auth headers are added in the file. |
| Login history | Hidden from anonymous viewers | Failed logins carry client IPs. |
| Global login back-off | Clients that signed in since start skip it | Otherwise one wrong guess a minute locks you out. |
| Chart times | Server labels in UTC, browser rewrites to local time | The server has no time zone (scratch image). |
| Commit trailer | `Claude Opus 5.5` | The plan asked for `Claude Sonnet 5.5`; the model doing the work is Opus 5.5. |
| Logs read model | Login required by default (`[logs] require_login = false` opens them); hidden/excluded containers never tailable | Logs often contain secrets; fail closed (security review). |
| Log-alert matcher | Own linear-time regex subset (no `regex` crate, +0.24 MB instead of ~1 MB) | Image budget; no backreferences/lookaround. |
| Container secrets | Env values, command lines, health output, bind sources, process lists need a session | Security review; same rule as /host command lines. |
| Web font | None; system stack with a strict scale | ≤ 40 KB font budget and offline; platform fonts are good. |
| Process scan on /host | Only while someone views /host (3 s), not in the 15 s sampler | ~30 ms per scan. |
| Tailscale | Plain `http://100.125.149.51:3200`. `tailscale serve` **not** used. | Not needed; avoids touching Tailscale settings. |

## Hobnail branding — 2026-11-14

Approved project name: **Hobnail**. Tagline: “A tiny home for your homelab.”
The package, executable, image, Compose service, CLI help, exports, notification
messages and active documentation now use Hobnail. A shared SVG nail-head mark
appears in the sidebar, favicon, installable app icons and README. Custom site
titles and hostnames remain visible alongside the product name.

Existing `rust-homepage.*` container labels remain supported, and existing
`rust-homepage.db` databases are reused in place. `RH_*` settings and browser
storage keys stay compatible. Prior entries below the original milestones are
historical and retain the names used at the time.

Code and deployment templates only: the live container and stack path have not
been changed. No software license was added. See README for cutover notes.

Validation: formatting, Clippy with warnings denied, 349 passing tests (one
ignored), JavaScript and shell syntax, Compose config, and the release Docker
image build passed. The image is available locally as `hobnail:branding-check`.
