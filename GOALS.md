# Hobnail: goals

A small, fast, hackable home-server dashboard written in Rust. It should replace
the current Homepage (gethomepage) container, which idles at ~112 MiB and needs a
watchdog script to keep `services.yaml` in sync with Docker.

Status: **M0 to M5 and v2 (V1 to V4) are built**: a Dozzle-style UI, a history database and
settings inside the app behind a login. M6 (cutover) waits for the owner. The v2
spec is [docs/DESIGN.md](docs/DESIGN.md); sections 9 to 11 below are its goals. Numbers marked *(baseline)* were measured on this host on 2026-11-14 with
`docker stats --no-stream` (host: 7.2 GiB RAM).

## 1. Resource usage (the headline goal)

| Metric | Baseline | Target | Stretch |
|---|---|---|---|
| Idle RAM (RSS), no viewers | Homepage: 112 MiB *(baseline)* | **≤ 15 MiB** | ≤ 8 MiB |
| RAM with 1 active viewer | n/a | ≤ 25 MiB | ≤ 15 MiB |
| Container image size | Homepage: 376 MB *(baseline)*; Dozzle: 101 MB | ≤ 20 MB | ≤ 10 MB |
| Idle CPU | not measured yet | ~0% | ~0% |
| Cold start to first request | not measured yet | < 200 ms | < 50 ms |
| Page load (LAN, first paint) | not measured yet | < 150 ms | < 80 ms |

Reference points on this host *(baseline)*: Dozzle 41 MiB, Beszel hub 37 MiB,
Beszel agent 14 MiB, Arcane 59 MiB (after a memory cap; 83 MiB without).

How to hold ourselves to it:
- Poll lazily. When nobody has the page open, do (almost) no work. Cache Docker and
  widget results with a TTL and only refresh while a client is connected.
  **Exception: alerting (section 5).** Alerts must fire with nobody watching, so
  only the checks that an alert rule actually uses keep running in the background.
  Prefer the Docker events stream (push, near-zero cost) over polling for these.
- Cap the async runtime (a small fixed worker count). The default of one thread per
  core costs memory for nothing on a dashboard.
- Keep dependencies small. Check `cargo tree` and binary size when adding a crate.
- Measure on every milestone and record the numbers in `docs/measurements.md`.
  A regression past the target blocks the milestone.

## 2. Customizability

- **Config as a TOML file** (decided, see "Decisions made"). Layout, overrides,
  widgets and alert rules live in one place, and it can be put in git.
- **High-level, not exhaustive.** This is the opposite of Homepage's `services.yaml`,
  where every card is written by hand. With an empty config the page must already
  be useful: every container is discovered, grouped by compose project, given an
  icon and a link from its published ports, and shown with its state. The config
  file holds only *overrides and additions*: rename a card, pin one to a group,
  hide one, set an icon, add a bookmark, add a widget. A 10-line config should
  give a good page, and nothing should ever have to be listed just to appear.
- **Hot reload.** Editing the config updates the page without restarting the
  container. A broken edit keeps the last good config and shows a clear error with
  the file and line, instead of a blank page.
- **Layout control:** groups, per-group columns, ordering, collapsible groups,
  sizes for cards, and the choice between card grid and compact list.
- **Theming:** all colors, spacing and fonts come from CSS variables. Ship dark and
  light, follow the OS setting, and allow a user `custom.css` that is loaded last.
- **Icons:** local files, built-in set, and a dashboard-icons name lookup. Fetched
  icons are cached on disk so the page works with no internet.
- **No required rebuild** to change anything above.

## 3. Docker integration (replaces the watchdog script)

- Read container state straight from the Docker socket (read-only). No generated
  YAML, so no `homepage_dashboard_watch.py`.
- Show **running and stopped** containers, health status, uptime, published
  ports, and the compose project each one belongs to.
- Group automatically by compose project. A manual override in the config always
  wins over the automatic result.
- Opt-in and opt-out through container labels. Accept the existing
  `homepage.*` labels where practical, so migration is cheap.
- Show live per-container CPU and memory, and link out to Dozzle for logs and
  Beszel for history instead of re-implementing them.
- Never write to Docker. No start, stop or remove in v1. Read the compose file
  as text for display only, if we add that.

## 4. Custom widgets and pulling data from elsewhere

The page should accept data from anywhere without anyone recompiling it.

Built-in widgets (parity with the current page): greeting, clock/date, search,
CPU, memory, disk, network, temperature, host uptime.

The model follows Glance: a set of built-in widget types, plus a few generic
ones that let anyone build their own without writing Rust.

Widget types, easiest first. We start with 1 and 2, and add the rest over time:
1. **Custom API widget (v1).** URL, optional headers, a path expression to pull
   fields out of the JSON response, and a small template that renders them. This
   covers most self-hosted APIs (Beszel, Mailpit, Odoo health, Tailscale) with
   no code. This is the same idea as Glance's `custom-api` widget.
2. **Command widget (v1).** Run a script and show its JSON output. Disabled by
   default and enabled per widget in the config.
3. **Extension widget (later).** Point at a URL that returns a ready-made HTML
   fragment, so a widget can be a tiny service written in any language. This is
   the same idea as Glance's `extension` widget.
4. **Feed widgets (later).** RSS/Atom, and similar read-only feeds.
5. **Embed widget.** Show another service's page in a frame.
6. **Plugin interface (much later).** Possibly WASM. Only if types 1–5 prove too
   limiting.

Rules for every widget:
- Each widget has its own timeout and refresh interval, set in the config.
- **One failing widget never breaks the page.** It shows an error state and the
  rest of the page keeps working.
- Secrets (API tokens) come from environment variables or files, never from the
  config file, and are never written to logs.
- Outgoing HTTP can be restricted to an allow-list of hosts.

## 5. Notifications and alerts

The dashboard should tell you when something is wrong, not only show it when you
look.

**Channels** (a "channel" is a named destination in the config):
- **Generic webhook.** POST to any URL with a configurable JSON body template.
  This is the escape hatch that covers ntfy, Discord, Mattermost, Home Assistant
  and anything else that accepts a webhook.
- **Slack** (incoming webhook).
- **Telegram** (bot token + chat id).
- Further channels (email, etc.) can be added the same way later.

**Rules** (what triggers a notification), starting with:
- A container stops, exits non-zero, becomes unhealthy, or restarts repeatedly.
- A service check (HTTP up/down) fails or recovers.
- Host thresholds: disk, memory or temperature above a limit for N minutes.
- Any custom-API widget value crossing a threshold, so alerts work for anything
  the widget system can read.

**Behavior:**
- **Dedupe and cooldown.** One alert per incident, not one per poll, and a
  configurable cooldown before the same rule can fire again.
- **Recovery messages.** Say when it came back, not only when it broke.
- **Routing.** Each rule names its channels, so noisy things go to one place
  and serious things to another.
- **Messages are templates** with the container, service, value and time available.
- **Secrets** (bot tokens, webhook URLs) come from environment variables or files,
  never from the config, and are never logged.
- **Test from the CLI:** a command that sends a test message to a channel, so a
  new channel can be verified without breaking something.
- **Survives restarts.** Keep a tiny state file so a restart does not re-send
  every open alert, and so "already notified" is remembered.
- **Failures are visible.** If a channel is down, the dashboard says so instead
  of silently dropping alerts.

## 6. Security and access

- Bind only to the addresses in the config (here: the LAN and Tailscale IPs, as
  Dozzle does), never `0.0.0.0` by default.
- Notification channels send data out, so each one is explicit in the config and
  nothing is sent anywhere by default.
- Optional authentication: a single shared token or basic auth to start with.
- The Docker socket is mounted read-only, and the docs recommend a socket proxy.
- Custom command widgets and cross-host fetches are off unless explicitly enabled.
- No telemetry, and no calls to the internet at runtime beyond what the user
  configures (icons and fonts are local).

## 7. Experience

- Server-rendered HTML with a small amount of JavaScript for live updates (SSE or
  htmx-style partial updates). No build step for the frontend, and no Node.
- Works well on a phone.
- Keyboard quick-launch: press `/`, type a service name, press Enter.
- Honest status: a card is green only when a check says it is healthy, and a
  stopped container reads as stopped, not as an error.

## 8. Operations

- One static binary, one small image, one `compose.yaml` under `/opt/stacks`.
- `/healthz` endpoint and a container healthcheck.
- Structured logs, and a clean shutdown on SIGTERM.
- Tests for config parsing, label parsing, the widget engine and alert rule
  evaluation (including dedupe and cooldown), run with `cargo test`.

## 9. History (a local database)

The page should remember. A small embedded database (SQLite, capped page cache)
records host and per-container CPU and memory every 15 s, plus an event log of
container starts, stops and deaths, alerts, config changes and logins.
Rollups keep 7 days at 1 minute and 90 days at 10 minutes.

- Answers "what happened last night?" with charts and an incident timeline.
- Budget: database under 50 MB at the defaults, idle RAM with it running under
  8 MiB (the overall target stays 15 MiB).
- Sampling runs with or without a viewer (history needs it). Its cost is measured.
- Details: [docs/DESIGN.md](docs/DESIGN.md) section 4.

## 10. Settings inside the app

Everything safe to change should be changeable from the browser, not only from the
console: page options, container and group overrides, bookmarks, custom-API
widgets, notification channels and rules, appearance, retention.

- Edits are lossless (comments and ordering in `config.toml` survive) and are
  validated before they are written. An invalid edit is never saved.
- Every save is a revision in the database: list, diff, restore.
- A raw TOML editor stays available for power users.
- **Writes require a login.** Viewing stays open. The password is set from the
  console (`set-password`), so nobody can claim an unprotected instance.
- `command` widgets are never editable from the app.
- Secrets are never shown or stored by the app; forms hold env var names.
- Details: [docs/DESIGN.md](docs/DESIGN.md) sections 5 and 6.

## 11. UI design

The first UI looked like Homepage: pale cards in a grid, one big tile per
container, no hierarchy. v2 follows Dozzle's layout language instead.

- Sidebar with a stack tree, a command palette (`Ctrl/⌘ K`), dense sortable
  tables, dark first with a light theme, tabular numbers, charts as inline SVG.
- Pages: Overview, Containers, Container detail, History, Alerts, Settings.
- Works from 390 px to wide desktop; keyboard reachable; WCAG AA contrast.
- Server-rendered, no JS framework, no WASM, no chart library.
- Details and tokens: [docs/DESIGN.md](docs/DESIGN.md) sections 1 to 3.

## Non-goals (for now)

- Editing `command` widgets from the web UI. They run shell commands, so they stay
  file-only (section 10).
- A full monitoring and alert manager. Notifications cover "something broke"
  and "it recovered". Long-term metrics, graphs and complex alert logic stay in
  Beszel.
- Starting, stopping or deploying containers. That is what Dockge and Arcane
  did, and we don't want the socket write access.
- Log viewing and long-term metrics. Dozzle and Beszel own those.
- Multi-user accounts, roles and OIDC.
- Multi-host Docker aggregation in v1.

## Milestones

Each milestone ends with the measurements from section 1 recorded.

| # | Milestone | Done when |
|---|---|---|
| M0 | Skeleton | Binary serves a styled static page from a container, config loads and hot-reloads. |
| M1 | Docker cards | Containers appear grouped by compose project, running/stopped state is correct. |
| M2 | System widgets | CPU, memory, disk, network, temperature, uptime match what Homepage shows today. |
| M3 | Custom widgets | The custom API widget shows live data from at least 3 services (e.g. Beszel, Mailpit, Odoo), and the command widget works. |
| M4 | Notifications | A stopped container and a recovered one each send a message to a webhook, Slack and Telegram, once, with dedupe and cooldown. |
| M5 | Theming and polish | Light/dark, custom CSS, icon cache, keyboard quick-launch, phone layout. |
| M6 | Cutover | Parity checklist passes, then it replaces Homepage. The watchdog script is retired. |

v2 (see [docs/DESIGN.md](docs/DESIGN.md)):

| # | Milestone | Done when |
|---|---|---|
| V1 | Storage | History database records host and container samples and events, rollups and retention work, `/api/history/*` and `/api/events` answer, login and sessions work (`set-password`). |
| V2 | New UI | Shell, Overview, Containers, Container detail, History, Alerts and Login in the new design, checked in a browser at 1440, 900 and 390 px, dark and light. |
| V3 | Settings | Every tab in DESIGN.md section 5 works, edits keep comments, each save is a revision, `command` widgets are protected. |
| V4 | Polish and measure | Docs updated, measurements redone (RAM, DB size, sampler cost), M6 checklist refreshed. |

v3: make it the one tool you keep open.

| # | Milestone | Done when |
|---|---|---|
| W1 ✅ built (look awaits owner sign-off) | The "10x" wave | Logs in the app, deep container detail, Beszel-level host page, and a real design pass (health strips, linked charts, themes, keyboard, PWA); owner signs off the look. |
| W2 | Breadth | Endpoint and uptime monitors, alert rule builder with silences, images/volumes/networks/disk pages, read-only compose view with drift. |
| W3 | Reach and control | Owner decides on container actions; arrangeable Overview and pages; Glance-style widgets; Prometheus `/metrics`; multi-host design note. |

Budgets for v3 are in the plan (idle 8 MiB or less, image 12 MB or less).

Until M6, it runs **alongside** Homepage on a different port. Homepage stays
untouched. Idle RAM is measured at every milestone, and M4 specifically checks the
cost of always-on alert checks.

## Decisions made

- **D1. Config format: TOML**, with automatic discovery and sensible defaults, so
  the file only holds overrides and additions (section 2). We do not reuse
  Homepage's YAML.
- **D2. Widget model: start with the custom API and command widgets**, and grow
  towards Glance's set (extension, feed, embed widgets) over time. No plugin
  system from day one.

## Decisions we still have to make

- ~~D3. Auth.~~ **Decided for v2:** viewing stays open, writes need a login
  (DESIGN.md section 6).
- **D4. Label compatibility.** Reuse Homepage's `homepage.*` labels for easy
  migration, or define our own, cleaner set?
- ~~D5. Name.~~ **Decided:** Hobnail. Binary, image and container: `hobnail`.
- ~~D6. Alert state.~~ The state file stays for now. The database now holds the
  alert *log*; moving open-incident state into it is optional later.

## Starting technology (to be confirmed by measurement)

Not a commitment. Each choice must prove itself against the numbers in section 1.

- `axum` + `tokio` (small fixed worker count) for the server.
- `bollard` for the Docker API.
- `serde` with `toml` for config, `notify` for hot reload.
- A small HTTP client for the custom API widget and for sending notifications. Compare
  `reqwest` against a lighter option such as `ureq` or `hyper` directly, because
  TLS and the client are likely the largest RAM and size cost.
- `maud` or `askama` for HTML templates.
- Read `/proc` and `/sys` directly for host stats rather than pull in a large crate.
- Watch the allocator: a musl static build is small but its default allocator can
  use more memory than expected, so compare against `mimalloc`.
- Build with a multi-stage Dockerfile into a `scratch` or distroless image.
