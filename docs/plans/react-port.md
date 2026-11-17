# React port: research and plan

Date: 2026-11-17. Status: **plan for the owner's review. Nothing is built.**
Written against `main` at `5394304`. Binding input:
[docs/decisions/react-client-and-dashboard.md](../decisions/react-client-and-dashboard.md)
("the decision"). This document changes no other file.

How to read the marks:

| Mark | Meaning |
|---|---|
| **[code]** | verified in this repository's code or documents, with `file:line` |
| **[exp]** | measured or observed in an experiment run for this plan (section 4.11 says what ran) |
| **[src n]** | from a cited source, listed in section 8, read 2026-11-17 |
| **[assume]** | an assumption or estimate; not verified |

MiB is 2^20 bytes, MB is 10^6 bytes, KB is 10^3 bytes. Asset bytes and
runtime RAM are never mixed in one figure.

## 1. Summary

**The premise needs one correction first.** The server is not axum. It is
hyper 1.x with one hand-written `match` router, a single-threaded tokio
runtime and no tower layers [code `src/http/server.rs:1-2,182-249`,
`Cargo.toml:8-24`, `src/main.rs:42-47`]. There is no page CSP, no response
compression and, on purpose, no service worker [code `src/ui/icons.rs:218-230`,
`src/ui/pwa.rs:4-8`]. Everything below is designed for that server, with no
new runtime crate.

**Recommended approach.**

1. **Same binary, client embedded.** Vite builds hashed files in a Node build
   stage; a `build.rs` table embeds each file and a pre-gzipped copy with
   `include_bytes!`; the router serves them with no runtime compression.
   Measured on a copy of the real server with a real React/Vite build
   (907 KB raw): binary +1.18 MB, container RAM unchanged within run-to-run
   noise through 25 clients, zero errors [exp]. "One file, same binary in
   both installs" still holds.
2. **Build side by side, switch once.** The React client grows under `/app`
   while every existing page keeps working at its URL. Pages are ported and
   verified one at a time; users are switched in one step when the parity
   checklist passes, with an environment switch back to the old UI for one
   release. Then `static/` and the maud pages are deleted.
3. **JSON API first.** All nine Settings tabs already have JSON
   [code `src/settings/api.rs:68`]. Twelve areas exist only as rendered HTML
   (section 4.1). They get read endpoints under `/api/v1`, a JSON event
   stream replacing HTML fragments, and a session endpoint. The write gate,
   `X-RH-CSRF`, the same-origin check and the Host guard do not change.
4. **First vertical slice:** shell, sign-in, the Containers table, and a
   container's Overview tab with its charts and the "Edit" dialog. It
   exercises every new mechanism once: build, embedding, CSP, session, the
   view-model API, the JSON stream, Recharts, UI SDK, and a guarded write
   with a 409.
5. **Port to parity first (no database migration, no config change), then
   the five product outcomes.** Until parity, rollback is swapping the
   image or binary. The product features need security decisions the
   current model does not answer (credentials entered in the browser,
   destination binding, stable IDs); those are in section 2 and nothing in
   that part is built before they are answered.

**What the research found that the owner should know before anything else.**

- **UI SDK exists but is five days old.** `@ui-sdk/react` and `@ui-sdk/core`
  were first published 2026-11-12; seven betas in five days; `latest` is
  `0.1.0-beta.6` [src 1, 2]. The decision's install instructions match the
  shipped package [src 3, exp]. Under any release-age gate of one day or
  more, npm 11 refuses every version today [exp]. Rule 6 of the decision
  ("stop that installation and ask the user") therefore triggers now. A
  pinned beta passes a 7-day gate with no exception once it is 7 days old.
- **Recharts costs 378 KB raw / 106 KB gzip** as a lazy chunk; the same
  two-series chart hand-written in SVG was 2 KB [exp].
- **A strict CSP works** (`script-src 'self'; style-src 'self'`) with one
  documented UI SDK switch in `index.html` [exp].
- **Browsers on plain HTTP ask only for gzip**, so embedded brotli would
  never be used without a TLS proxy [exp].
- **The decision and the current security model disagree in three places**
  that only the owner can settle: secrets "never stored by the app" versus
  "enter URL/credentials"; destination binding versus adding a credentialed
  integration from the browser; "every page works with JavaScript blocked"
  versus a client-rendered app (sections 2 and 3).
- **`/api/state` is open and unfiltered today**: it returns the raw
  container snapshot, including containers the owner hid, to anyone who can
  reach the port [code `src/render.rs:22-42`]. The port should not copy that
  into the new API (decision D8).

## 2. Decisions the owner must make

Nothing is built until the "before Part A" decisions are answered. Part B
decisions can wait until the port reaches parity.

### Before Part A (the port)

| # | Decision | Options | Recommendation | Cost of the recommendation |
|---|---|---|---|---|
| D1 | Order of work | (a) port to parity, switch, delete the old UI, then product features; (b) slice, then onboarding and integrations, parity later | **(a)** | Onboarding arrives later than the decision's step order (§8.4) suggests. In return: no migration or config change until parity, one UI to maintain during the feature work, and a clean before/after measurement |
| D2 | UI SDK now, later, or an alternative | (a) use it now: pin one beta, wait until it is 7 days old, wrap every component in `web/src/ui/`; (b) wait for a stable release; (c) Base UI `@base-ui/react` 1.9.0 (MIT, 1.0 since 2025-12) [src 4] | **(a)**, with a go/no-go check at the end of phase 1 | A beta with no track record and no independent accessibility audit [src 3]; expect API changes. The wrapper keeps a later swap to (c) inside one folder. Measured cost: about 165 KB raw / 70 KB gzip eager for Dialog, Select, Tabs, Switch, plus 82.5 KB raw / 10.7 KB gzip for `web.css` [exp] |
| D3 | Release-age policy | (a) 7 days for every package, no exception; (b) 7 days with an exception for UI SDK; (c) none | **(a)** `min-release-age=7` and `ignore-scripts=true` in `web/.npmrc` | First install waits until the pinned UI SDK beta is 7 days old (about a week after pinning). Security fixes younger than 7 days need a deliberate, recorded exception |
| D4 | Charts | (a) Recharts for full charts, loaded only on routes that have one; sparklines, health strips, treemap and meters drawn as plain SVG in React; (b) Recharts for everything; (c) port `src/ui/chart.rs` to TypeScript and use no chart library | **(a)**, which follows the decision ("Recharts for charts in the initial implementation") | +106 KB gzip on chart routes [exp]. Whether Recharts can do the linked crosshair, drag-to-zoom, gaps, bands and event markers the current charts have is **not verified**; phase 1 proves it on one chart or returns to the owner. (b) is unverified at 150 sparklines; (c) contradicts the decision |
| D5 | How the client gets the CSRF token after a reload | (a) new `GET /api/v1/session`, `no-store`, returns the token only to a valid session; (b) the server templates `index.html` per request with the meta tag | **(a)** | Overrides "No new token route" [code `docs/DESIGN.md:606`]. Same exposure as today's meta tag: same-origin readers only; the cookie is `SameSite=Strict` [code `src/auth.rs:520-522`] |
| D6 | JavaScript blocked | (a) accept that the React app needs JavaScript; `<noscript>` explains it; `/healthz`, the API and log download still answer; (b) keep the old pages as a no-script fallback forever | **(a)** | Overrides "Every page works with JavaScript blocked" [code `docs/DESIGN.md:341`]. (b) means two UIs permanently and the old code never leaves the binary |
| D7 | Where the client lives | (a) embedded in the binary, raw + gzip; (b) raw + gzip + brotli; (c) files on disk beside the binary | **(a)** | Binary +1,179,648 B [exp]. Any client change relinks the Rust binary: 166 s on the test host [exp]. (b) adds 225,824 B that plain-HTTP browsers never request [exp]. (c) breaks "one file" and the native install spec |
| D8 | Read access in the new API | (a) same public/session split as the pages have today, enforced by one helper, and `/api/state` becomes session-only; (b) copy today's behaviour exactly, `/api/state` stays open; (c) all reads need a login | **(a)** | Anything that scripts `/api/state` without a session stops working. No change to what a visitor sees on pages |
| D9 | Compressing JSON and the event stream (rest of ROB-04) | (a) measure JSON sizes after phase 2, then decide; (b) add `miniz_oxide` now | **(a)** | Static files are solved by pre-compression with no crate. Dynamic JSON stays uncompressed until measured |
| D10 | Image size during the side-by-side period | (a) accept about 13.3 MB by `docker images` until the old UI is deleted; (b) hold 12 MB | **(a)** | Exceeds the v3 figure "image 12 MB or less" [code `GOALS.md:268`]; stays under the 20 MB target [code `GOALS.md:23`, `docs/DESIGN.md:864-865`]. 13.3 MB is an estimate from the measured binary delta |
| D11 | Rust to TypeScript types | (a) `ts-rs` as a dev-dependency (`cfg_attr(test, derive(TS))`), generated files committed, a test fails the build on drift; (b) `typeshare` CLI; (c) hand-written types plus shared JSON fixtures | **(a)** | One dev-dependency crate (MIT) [src 5]. That it adds nothing to the release binary is an assumption to prove in phase 0 by comparing binary bytes |

### Before Part B (product features)

| # | Decision | Options | Recommendation | Cost of the recommendation |
|---|---|---|---|---|
| D12 | First password | (a) console only, as today; (b) one-time code in the container log; (c) web claim from loopback | **(a)** | Onboarding starts after the first sign-in; before that the page shows the command. (b) and (c) let someone else claim the instance in more cases |
| D13 | Credentials entered in the browser | (a) names only, as today; (b) write-only secret files: the value goes to `data/secrets/<generated>` (0600), only the `*_file` path is stored, no endpoint returns it; (c) the same, encrypted with a key | **(b)** | Changes "Secrets are never shown or stored by the app" [code `GOALS.md:209`] to "never shown, never in the config or revisions". Plaintext at rest on the data volume, like the operator's own secret files today. (a) cannot deliver "enter URL/credentials, test, add" because an env var needs a restart [code `docs/features/notifications.md:78-79`]. (c) helps only if the key lives elsewhere |
| D14 | Destination binding for browser-entered credentials | (a) keep the rule; credentialed integrations are file-only; (b) bind at creation: a browser-entered secret is stored with the origin it was entered for and only ever sent there; changing the origin means typing the secret again; hand-made references keep today's rule | **(b)** | New guard code and tests in `src/settings/guard.rs`. Correctness depends on comparing the origin at send time, not only at save time |
| D15 | Where boards, layout and integration instances are stored | (a) `config.toml`; (b) a separate versioned file in the data folder, same lossless editor, its own hash; (c) SQLite | **(b)**; setup drafts in SQLite | A second file to back up and document; no revisions for it unless built. In return an older binary ignores it, so rollback stays possible, and layout saves do not consume the 200 config revisions [code `src/db/mod.rs:251`]. (a) makes an older binary reject the whole config [code `src/config.rs:17,300-314`]. (c) loses boards whenever the database fails to open [code `src/db/mod.rs:403-420`] |
| D16 | Empty-config behaviour | (a) with no board file, an automatic "All containers" board exactly as today; curated boards are additive; (b) curated only | **(a)** | Keeps "nothing should ever have to be listed just to appear" [code `GOALS.md:52-58`] and still meets "do not dump every discovered container" once the owner has curated a board |
| D17 | Saving in edit mode (revisits Y13) | (a) explicit Save and Cancel, one write per editing session; the one-click arrows in Settings lists stay until edit mode replaces them; (b) autosave per move | **(a)** | Y13 said one revision per arrow click [code `docs/DESIGN.md:860`]; edit mode needs a draft. (b) reloads other clients and writes on every drag |
| D18 | Who can see data fetched with a credential | (a) open like the rest; (b) per integration, sign-in required by default when it has a credential (the `[logs] require_login` pattern); (c) all viewing needs a login | **(b)** | Two rendering paths per integration tile; every read must apply the flag |
| D19 | First providers | the owner names three to five | none from me: today the catalog has **zero** real providers [code `src/widgets/mod.rs:311-356`] | Each provider is a Rust module with a schema, a test and widgets. The custom-API widget stays as "Custom" |
| D20 | Service icons in the catalog | (a) the existing opt-in dashboard-icons fetch and cache; (b) bundle a brand icon set | **(a)** | Letter avatars until `[icons] remote = true`. Lucide 1.x removed brand icons [src 6]. (b) needs a licence review and bundle bytes |
| D21 | Services with self-signed certificates | (a) public roots only at first, documented; (b) per-integration pinned fingerprint; (c) "skip verification" | **(a)**, then (b) | Connection tests against self-signed services fail with a TLS message. (c) sends credentials to an unauthenticated peer |

Two items are not decisions but need the owner's awareness: the decision's
instruction to import `@ui-sdk/core/web.css` costs 82.5 KB raw / 10.7 KB
gzip, 96 % of the spike's CSS [exp]; and the native-install decisions D1
(releases) and D10 (version numbers) in the local native-install spec decide
how upgrade and rollback are named (section 6).

## 3. Conflicts with existing documents

Every line the decision overrides, with proposed wording. **None of these
files is changed by this plan.** Wording goes in when the phase named in the
last column is signed off. Lines that only need a path or number refreshed
are grouped at the end.

### GOALS.md

| Lines | Today | Proposed wording | When |
|---|---|---|---|
| 12-14 | "The agreed next direction, a React client served by the same Rust server, is recorded in … and not started." | "The React client served by the same Rust server (decision: …; plan: docs/plans/react-port.md) is in progress: phase N." | phase 0 |
| 52-58 | "…the page must already be useful: every container is discovered … nothing should ever have to be listed just to appear." | Keep, and add: "Once a board has been arranged in the app, that board shows what was chosen; everything discovered stays available as suggestions and on the automatic 'All containers' board." | D16, Part B |
| 61-62 | "Layout control: groups, per-group columns, ordering, collapsible groups, sizes for cards, and the choice between card grid and compact list." | "Layout control: boards, sections, order and widget sizes, arranged in the app's edit mode and stored on the server (file: …)." | D15, Part B |
| 114-115 | "Secrets (API tokens) come from environment variables or files, never from the config file, and are never written to logs." | Unchanged in meaning. Add: "The app may write a secret file under `data/secrets/` when a credential is entered in the browser; it never returns one." | D13 |
| 168-169 | "Server-rendered HTML with a small amount of JavaScript for live updates (SSE or htmx-style partial updates). No build step for the frontend, and no Node." | "A React client built with Vite and served by the Rust server from the same binary. Node runs only at build time; there is no Node process, SSR service or second container in production. Live updates are JSON over SSE." | phase 5 |
| 177 | "One static binary, one small image, one `compose.yaml`…" | Unchanged. It still holds [exp]. | none |
| 180-181 | "Tests for config parsing … run with `cargo test`." | Add: "Client unit tests and the browser checks run in the image build and in `scripts/`." | phase 0 |
| 206-207 | "The password is set from the console (`set-password`)…" | Unchanged (D12 a). | none |
| 209 | "Secrets are never shown or stored by the app; forms hold env var names." | "Secrets are never shown by the app and never stored in the config or its revisions. Forms hold an env var name, a secret-file path, or accept a credential that is written to a secret file and bound to its destination." | D13, D14 |
| 217-218 | "…tabular numbers, charts as inline SVG." | "…tabular numbers, charts drawn in the browser (Recharts; small graphics as SVG)." | phase 5 |
| 221 | "Server-rendered, no JS framework, no WASM, no chart library." | "Rendered in the browser by React; the Rust server serves the client and a JSON API. No WASM, no server-side JavaScript." | phase 5 |
| 268 | "Budgets for v3 are in the plan (idle 8 MiB or less, image 12 MB or less)." | "…image 12 MB or less; 15 MB while the old and new UI ship together (plan section 4.9)." | D10 |
| 303 | "`maud` or `askama` for HTML templates." | Remove when the last maud page is deleted. | phase 6 |

### docs/DESIGN.md

| Lines | Today | Proposed wording | When |
|---|---|---|---|
| 24-26 | "The hard constraints … still apply: tiny RAM, tiny image, one static binary, no Node, no frontend build step…" | "…tiny RAM, tiny image, one static binary, no Node at run time…" | phase 5 |
| 32 | UI technology: "Server-rendered `maud` + hand-written CSS + ~15 KB of vanilla JS. No Leptos/Dioxus/Yew, no Tailwind, no chart library." | "React 19, TypeScript and Vite; UI SDK for interaction behaviour, Lucide for icons, Recharts for charts; hand-written CSS on the §11 tokens. No Tailwind, no WASM." | phase 5 |
| 33 | Charts: "Inline SVG, drawn on the server from database series." | "Drawn in the browser from the JSON series API." | phase 5 |
| 34 | Icons: "One inline SVG sprite of ~30 hand-picked line icons." | "Lucide React, imported per icon." | phase 5 |
| 92-93 | "Server-rendered full pages; navigation is plain links (fast enough that a SPA buys nothing). The existing SSE channel keeps swapping fragments for live data." | "One client-rendered application with real URLs for every page and dialog. The SSE channel carries JSON events." | phase 5 |
| 158 | "Read API (JSON, no auth, same open model as the page)" | "Read API: each route is declared public, session-only or logs-gated in one table, and a test walks the table (plan section 4.2)." | phase 0 |
| 193 | "Secrets are never shown or stored by the app. Forms hold an env var name." | As GOALS.md 209. | D13 |
| 214 | Secrets (bound): "…cannot get a new destination or new secret references from the app unless that exact pair is already in the file…" | Add: "A credential entered in the browser is bound to the origin it was entered for at creation; moving it needs the credential again." | D14 |
| 224-234 | Module contract (`src/ui/pages/*.rs`, `static/*.css|js`) | Replace with the layout in plan section 5 (`web/`, `src/api/`, `src/http/assets.rs`). | phase 6 |
| 245 | "`cargo fmt --check`, `clippy -D warnings`, `cargo test` gate every build (as now)." | Add: "and the client's type check, unit tests and bundle-size check." | phase 0 |
| 338-340 | "No framework, library, build step, web font or CDN. Platform first…" | "No web font or CDN. Platform features first where they exist (`<dialog>`, `popover`); React, UI SDK, Lucide and Recharts are the only client libraries, and adding one needs a measured bundle cost." | phase 5 |
| 341-350 | "Every page works with JavaScript blocked. …" | "The application needs JavaScript. With it blocked, the page says so and names what still works (`/healthz`, the JSON API, log download)." | D6 |
| 599 | "The URL is the dialog" (`?add=`, `?edit=`) | Unchanged: the React router keeps these URLs. | none |
| 606 | "…re-fetches the current page with a same-origin GET and reads the new `<meta name="rh-csrf">` … No new token route (SET-17)." | "…asks `GET /api/v1/session` for the new token, then retries." | D5 |
| 842, 851 | Y18: Settings lists and form values rendered by the server | "Superseded by the React client: lists and values come from the session-only settings API." | phase 4 |
| 858 | Y12: a config change reloads other open pages, except logs pages and unsaved input | "A config change makes open pages refetch what changed; nothing reloads." (Stricter than Y12 asked for.) | phase 5 |
| 860 | Y13: one-click reorder arrows, one revision per step | Unchanged until edit mode exists; then D17. | D17 |
| 869-891 | §11.14 server changes (`?frag=list`, server-rendered dialogs, `/more`) | Mark the section "superseded by the JSON API" when the fragment routes are deleted. | phase 6 |

### Feature and install pages

| File and lines | Today | Proposed wording | When |
|---|---|---|---|
| `docs/features/ui.md` 3-5 | "Server-rendered pages (maud), hand-written CSS and a little vanilla JS. No framework, no build step, no web fonts, no CDN." | "A React client (TypeScript, Vite) served by the Rust server. No web fonts, no CDN." | phase 5 |
| `ui.md` 17-21 | "More" is a real link to `/more` "so everything is reachable with JavaScript blocked" | "More" opens the drawer. | phase 5 |
| `ui.md` 69-74 | A config change elsewhere reloads the open page | As DESIGN.md 858. | phase 5 |
| `ui.md` 75-78 | Pages rendered with a session are `no-store` | "JSON that carries configuration, session data or probe output is `no-store`; `index.html` is `no-cache`; hashed assets are immutable." | phase 0 |
| `ui.md` 132-140 | "one SSE stream per visible page … Charts: inline SVG drawn on the server" | "one SSE stream per tab, JSON events, topics follow the open page … Charts drawn in the browser." | phase 5 |
| `ui.md` 158-163 | Components are `static/components.css`, `static/ui.js`, `window.HB` | "Components live in `web/src/ui/`." | phase 6 |
| `ui.md` 172-174, 179-196 | No View Transition; "every navigation is a full page load, painted once" | "Navigation is client-side; the shell stays mounted." | phase 5 |
| `ui.md` 197-203 | Loading states are switched on by the script; plain words without it | Remove the no-script half. | phase 5 |
| `docs/features/settings.md` 52-56 | Lists and form values rendered by the server; `GET /settings/<tab>?frag=list` | "Lists and values come from `GET /api/settings/*` (session only)." | phase 4 |
| `settings.md` 95-102 | "Without JavaScript every page still shows its values…" | Remove. | D6 |
| `docs/features/login.md` 44-50 | Forms post; token picked up "by reloading itself in the background (a same-origin GET of the page)" | "…by asking `GET /api/v1/session`." | D5 |
| `docs/features/logs.md` 3-4, 96-98 | Code in `static/logs.js`; "a test fails if `logs.js` contains `innerHTML`" | New path; "a test fails if the log viewer uses `dangerouslySetInnerHTML`." | phase 3 |
| `docs/features/history.md` 53-55 | Toolbar links "are plain links, so the window moves … without JavaScript" | "…are links with real URLs, so the window can be shared and moved by keyboard." | phase 2 |
| `docs/install.md` 33 | "No Rust toolchain is needed on the host; everything builds in Docker." | "No Rust or Node toolchain is needed on the host; everything builds in Docker. The build needs network access to crates.io and the npm registry." | phase 0 |
| `install.md` 45-46 | "Open pages notice the new build and reload themselves." | "Open pages notice the new build and offer to reload." | phase 5 |
| `docs/configuration.md` 3-6 | "Two ways, same file…" | Unchanged for `config.toml`. Add a paragraph for the board file if D15 is (b). | D15 |

Refresh only (no change of meaning): `README.md:9-10` (image size and idle
RAM after re-measuring), `docs/configuration.md:113` and
`docs/BRANDING.md:78-90` (paths under `static/`), `docs/measurements.md:7-25`
(a method section for the decision's reporting rules), `docs/qa/QA.md`
(file references become historical; the 14 deferred findings get the
outcomes in section 4.12).

## 4. Findings

### 4.1 Inventory of today's app, and the API gap list

Counts [code `src/http/server.rs:202-248`, `src/ui/routes.rs:25-128`,
`src/settings/api.rs:68`]: 21 POST routes (login, logout, 19 under
`/api/settings/`), 2 SSE streams, 27 JSON GET routes, 1 streamed text
download, 13 static-asset routes, 12 full-page routes plus 9 Settings tabs,
3 HTML-fragment routes, `/healthz`. Presentation code is 20,033 of 50,340
Rust lines (39.8 %) [code, `wc -l` over `src/ui/**`, `src/logs/pages.rs`,
`src/host/view.rs`, `src/render.rs`]; the browser side is 4,880 lines of
JavaScript and 1,911 of CSS in `static/`.

**Pages**

| Page | Route | JSON for its data today? |
|---|---|---|
| Overview | `/` | **No.** Counts, attention rows, treemap, top lists, widgets, bookmarks are HTML only [code `src/ui/pages/overview.rs:108-186`] |
| Containers | `/containers` | **No.** `GroupView`/`CardView` are not `Serialize` [code `src/ui/view/mod.rs`]; `/api/state` is a raw dump, not this model |
| Container detail (Overview, Metrics, Inspect, Processes, Events) | `/containers/<name>` | Partly: CPU and memory series, events, env, health, top. **No** for "why is this red", the extended metric series, inspect, matching rules [code `src/ui/pages/container.rs:1595-1667`] |
| Logs: list, one container, stack, side by side | `/logs`, `/containers/<n>/logs`, `/stacks/<n>/logs` | Stream, download, levels, status: **yes**, already JSON [code `src/logs/tail.rs:158-248`]. The picker list: **no** |
| Host | `/host` | **No.** Everything is HTML, with a 3 s HTML poll [code `src/host/view.rs:34`] |
| History | `/history` | Preset ranges and events: yes. **No** for `from`/`to` windows, the yesterday line, starts per day, events by window |
| Alerts | `/alerts` | Signed in: yes (settings API). **No** for the visitor's read-only rules, channels, firing list and log |
| Settings, 9 tabs | `/settings/<tab>` | **Yes**, all nine [code `src/settings/api.rs:408-768`] |
| Login | `/login` | Login itself: yes. **No** way for a visitor to learn "is a password set" |
| More (phone) | `/more` | Not needed in a client-rendered shell |

**Live and machine endpoints**

| Endpoint | Payload | Note |
|---|---|---|
| `GET /api/stream` (SSE) | `hello` (build SHA), `frag` (**an id line, then rendered HTML**), `reload`, ping every 25 s | Rendered per connection and per `Host`, always as a visitor; 16-message queue per connection, 64-id broadcast; a lagging client gets `reload`; the refresh loop parks with no viewer [code `src/http/server.rs:343-382`, `src/live.rs:33-37`] |
| `GET /api/logs/stream` (SSE) | JSON: `hello`, `meta`, `l` (line batches), `err`, `end`, `done` | At most 4 containers (24 for a stack); one Docker connection per container per viewer, closed on disconnect [code `src/logs/tail.rs:34-39,107-124`] |
| `/api/state` | raw in-memory state | open, unfiltered (D8) [code `src/render.rs:22-42`] |
| `/api/palette`, `/alarm.json`, `/manifest.webmanifest` | JSON | kept |
| `/api/history/system`, `/api/history/container/<name>`, `/api/events` | JSON | kept; preset ranges only |
| `/api/logs/levels`, `/api/logs/status`, `/api/logs/download` | JSON, JSON, text | kept |
| `/api/containers/<name>/{env,health,top}` | JSON, session only | kept |
| `/api/settings/*` (15 GET, 19 POST) | JSON | kept unchanged; the React client uses it as it is |
| `/containers?frag=table`, `/host?frag=live`, `/settings/<tab>?frag=list` | HTML fragments | deleted with the old UI |

**Client behaviour** [code `static/app.js`, `static/ui.js`, `static/logs.js`,
`static/settings.js`]: 57 key bindings (16 global, 9 palette, 5 tree and
account menu, 14 tabs/dialog/menu/confirm, 10 logs, 3 Settings editors);
44 network calls, 7 of which consume HTML; 10 `localStorage` and 4
`sessionStorage` keys; no cookie written by script. Today every navigation
is a full page load, and theme, rail, folded stacks and tree scroll are set
by inline scripts before first paint [code `src/ui/shell.rs:25,37,174-180`].

**The API gap list** (data with no JSON source today):

1. Session and bootstrap: user, CSRF token for an existing session, whether
   a password is set, logs login rule, title, greeting, default theme,
   search URL, feature switches, build.
2. Container view model: groups and stacks with titles and order, display
   name, icon, resolved link, status word and reason, port bindings,
   hide/exclude filtering.
3. 24 h health strips per container and stack.
4. Overview aggregates: counts, needs-attention rows, treemap and top-CPU
   inputs.
5. Widgets for viewers: configuration and a structured body (today only
   server-built HTML plus `values`).
6. Bookmarks for viewers.
7. Read-only alert rules, channels, firing list and alert log for visitors.
8. History: arbitrary windows, yesterday comparison, starts per day, events
   by window and level.
9. Container detail: "why is this red", extended series, inspect (redacted
   for visitors), image digests, matching rules.
10. The whole Host page (host facts, process report, host series).
11. A JSON live stream.
12. The logs picker (tailable containers by stack).

Formatting and status rules that exist only in Rust and need a TypeScript
twin or a ready field: `human_bytes`, `human_duration`, `ago`, the status
vocabulary, event wording, exit-code meaning, port-link choice, the logs
hue [code `src/util.rs:134-152`, `src/ui/fmt.rs:12-50`,
`src/ui/view/mod.rs:417-475`, `src/ui/view/ports.rs:66`].

### 4.2 API contract

**Namespace and versioning.** New routes live under `/api/v1/`. Existing
`/api/*` routes keep their shape and paths; the old client uses them until
it is deleted, and `/api/settings/*` is already what the new client needs.
The client and server ship in one binary, so version skew only happens in a
tab left open across an upgrade; the stream's `hello` event already carries
the build SHA [code `src/http/server.rs:349`], and the client offers a
reload when it changes. A breaking change to a `/api/v1` shape goes to
`/api/v2`; additions do not. [assume: no third party depends on the current
routes.]

**Shapes.** JSON objects, `snake_case` keys, unix seconds for times (the
logs stream keeps its milliseconds), raw numbers with units in the field
name (`mem_bytes`, `cpu_pct`). The server sends raw facts and the reason
codes it already computes; the client formats. Port links are the
exception that moves: today the server renders links for the `Host` the
page was opened on, which is why fragments are rendered once per viewer
[code `src/http/server.rs:6-8`]; the API sends the bindings and the client
picks, so one serialised event can be shared by every viewer. The baseline
measured 2.0 ms and 3,630 allocations per viewer per 5 s for that render
[src 7: local baseline report, summarised in `docs/measurements.md`].

**Errors.** `/api/v1` handlers answer
`{"error":{"code":"…","message":"…","field":"…"}}` with `no-store`. The
existing settings errors keep their shape (`{error, field, line, col,
stale, locked}`) [code `src/settings/store.rs:36-80`]. **Gate failures stay
plain text** exactly as today (`sign in required`, `missing or bad X-RH-CSRF
header`, `cross-origin write refused`) because `gate_tests.rs` asserts the
bodies [code `src/auth.rs:113-126`, `src/http/gate_tests.rs:384-443`]; the
client maps them by status code.

**Login, session, CSRF, same origin: unchanged.** The cookie
(`rh_session`, `HttpOnly; SameSite=Strict; Path=/`), the token
(SHA-256 of `"csrf:" + session token`), the header `X-RH-CSRF`, the check
order (password set, session, CSRF, origin) and the Host guard stay as they
are [code `src/auth.rs:138-154,504-545`, `src/http/server.rs:154-206`]. Two
additions:

- `GET /api/v1/session` → `{user, csrf, password_set, logs_require_login,
  build, api}`; `csrf` and `user` are null without a valid cookie;
  `no-store` (D5). JSON login already returns the token
  [code `src/auth.rs:779-782`].
- Every new write route is added to the `WRITES` table in
  `gate_tests.rs`, which already proves the gate answers before any handler
  [code `src/http/gate_tests.rs:15-46,160-278`].

**Reads are declared, not open by default.** Today `GET` is open at the
router and each handler decides [code `src/http/server.rs:194`]. `/api/v1`
gets one table: route → `Public`, `Session` or `LogsGate`, checked before
the handler runs, and a test that walks the table as a visitor. The privacy
rules that are enforced at render time today move into the serialisers and
get tests: visitors never receive env values, command lines, probe output,
bind-mount paths, process lists, `auth.*` events or settings data, and
hidden or excluded containers appear nowhere
[code `src/ui/pages/container.rs:986-1030`, `src/db/mod.rs:196-201`,
`src/logs/mod.rs:127-142`].

**Live events.** One `GET /api/v1/stream?topics=…` per tab. Events:
`hello {sha, api}`, `containers` (list and state), `stats` (CPU and memory
per container), `system`, `alerts`, `widget {id, …}`, `config {hash}`
(the client refetches what depends on it instead of reloading), ping. The
stream is public and never carries session-only data, as today
[code `src/ui/shell.rs:93-104`]. Bounds stay: 16-message queue per
connection, `reload`-style resync when a client lags, viewer guard on the
body, refresh loop parked at zero viewers. New: topics let the server skip
work nobody is looking at (the Host page's process scan, a widget that is
not on screen), which is the decision's "suspend irrelevant updates".
A browser allows about six HTTP/1.1 connections per origin and a logs page
already uses one stream per pane [code `src/logs/mod.rs:270`]; the app
keeps one live stream per tab and closes it on `pagehide` and after 10 s
hidden, as now.

**Types in step** (D11). Rust response structs derive `TS` under
`cfg(test)`; `cargo test` writes the TypeScript and a test fails if the
committed files differ. The Node stage type-checks against the committed
files, so the build order stays Node then Rust. Formatting twins are held
together by fixtures: a Rust test writes inputs and expected outputs for
`human_bytes` and friends to JSON, and a client test asserts the same
outputs.

### 4.3 Serving the client from Rust

| Question | Finding |
|---|---|
| Embed or disk | **Embed.** A `build.rs` walks `web/dist`, writes a table of `(path, content type, raw, gzip, hashed)` with `include_bytes!`; the router serves from it. No runtime crate, no runtime compression. Built and run on a copy of the server: correct `Content-Encoding` and byte counts for gzip, brotli and identity, deep links served, unknown assets 404, existing pages unaffected [exp]. Crates such as rust-embed, memory-serve or tower-http's `ServeDir` assume axum/tower or hold copies on the heap [src 8, 9, 10] |
| Binary size | Base 6,633,904 B. Raw + gzip: 7,813,552 B (+1,179,648). Raw + gzip + brotli: 8,038,832 B (+1,404,928). `.rodata` grows by the embedded bytes; `.text` by 2,176 B [exp] |
| RAM | No measurable change in `memory.current`, working set, anon, file or `memory.peak` in any phase (section 4.9). Embedded bytes are file-backed pages of the executable, read only when served: `RssFile` rose by about 0.5 MiB (gzip only) to 0.8 MiB (gzip and brotli) after 25 clients downloaded everything, `RssAnon` did not move [exp]. cgroup v2 counts page cache in `memory.current` [src 11], so a cold cache can charge up to the binary delta, +1.13 MiB, of reclaimable file pages to the first container that reads it. **Cold cache was not measured** |
| Hashed names, cache headers | Vite emits `assets/[name]-[hash].ext` [src 12]. Those get `Cache-Control: public, max-age=31536000, immutable`; `index.html` gets `no-cache`, as MDN recommends [src 13]. Today's `?v=<hash>` scheme does the same job for six fixed files [code `src/http/server.rs:53-91,292-309`] |
| Pre-compressed assets (ROB-04) | Closed for static files with no new crate. Chromium on a plain-HTTP, non-localhost origin sent `Accept-Encoding: gzip, deflate` on all 158 requests: no `br`, no `zstd` [exp]. So gzip is the copy that matters; brotli only helps behind a TLS proxy |
| Client-side routing, deep links | A `GET` that is not under `/api/`, `/assets/`, `/static/`, `/icons/` or a known file returns `index.html`. Unknown `/api/*` stays 404 JSON; unknown `/assets/*` stays 404 so a tab holding an old build fails loudly and offers a reload, as Vite's guide recommends [src 14]. The hook is where unknown paths fall through today [code `src/ui/routes.rs:35-37`] |
| CSP | None today on pages; they use inline scripts and styles [code `src/ui/shell.rs:25,37`]. For the React document: `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`. Verified in headless Chromium 158 over nine flows (three routes, dialog and select opened, chart hover): **zero violations**, provided `index.html` carries `<meta property="csp-nonce" />`, the documented switch that makes UI SDK use constructed style sheets [src 3, exp]. Without it UI SDK injects two `<style>` elements and violates `style-src-elem` [src 31]. Vite's `index.html` has no inline script or style; no chunk contains `eval(` or `new Function(` [exp]. CSP is per document, so the old pages are unaffected while both exist. Firefox and Safari were not tested |
| Host-header guard | Unchanged, and it runs before asset serving [code `src/http/server.rs:183-193`] |
| JavaScript blocked | `index.html` holds a `<noscript>` block: what Hobnail is, that this page needs JavaScript, and that `/healthz` and the JSON API still answer (D6) |
| Service worker | None, as today. The stale-`index.html` problem a worker would create does not arise; the `hello` SHA covers upgrades |
| Manifest and icons | Stay generated and embedded by Rust [code `src/ui/pwa.rs:20-97`] |

### 4.4 Build

**Dockerfile** (proposed shape; not written):

```dockerfile
FROM --platform=$BUILDPLATFORM node:26-alpine@sha256:<pinned> AS web
WORKDIR /web
COPY web/package.json web/package-lock.json web/.npmrc ./
RUN --mount=type=cache,id=hobnail-npm,target=/root/.npm npm ci --ignore-scripts
COPY web/ ./
RUN npm run check && npm run test && npm run build && npm run size

FROM rust:1.100-alpine@sha256:<pinned> AS build
# as today, plus:
COPY --from=web /web/dist ./web/dist
COPY build.rs ./
# fmt, clippy, test, release build: unchanged gates
```

- **Gates.** `cargo fmt --check`, clippy `-D warnings`, `cargo test` and
  the release build stay in one `RUN` [code `Dockerfile:14-19`]. The Node
  stage adds the type check, client unit tests and a bundle-size check; a
  failure in either stage fails the image.
- **Pinned versions.** Node 26 is the active LTS (v26.13.1, npm 11.20.0)
  [src 15]. Both base images pinned by digest; today the Rust image is
  pinned by tag only [code `Dockerfile:5`]. `package.json` uses exact
  versions; `package-lock.json` is committed; `npm ci` installs only from
  the lock [src 16].
- **Versions verified for the first lock** [src 1, 2, 17-21, exp]:

  | Package | Version | Licence | Published | Note |
  |---|---|---|---|---|
  | `react`, `react-dom` | 19.3.1 | MIT | 2026-10-14 | no React 20 exists |
  | `@ui-sdk/react`, `@ui-sdk/core` | 0.1.0-beta.6 (`latest`); beta.7 on `beta` | Apache-2.0 | 2026-11-16 | first publish 2026-11-12; peers `react >=19` |
  | `lucide-react` | 1.70.0 (newest ≥ 7 days old; latest 1.74.0) | ISC | 2026-11-06 | a release every 1 to 4 days |
  | `recharts` | 3.10.1 | MIT | 2026-07-25 | no stable release in four months |
  | `vite` | 8.3.7 (8.3.8 was 6.5 days old) | MIT | 2026-11 | bundler is Rolldown; Node `^20.19 || >=22.12` |
  | `@vitejs/plugin-react` | 6.3.0 | MIT | 2026-11-09 | Oxc transform, no Babel |
  | `typescript` | 7.0.2 | Apache-2.0 | 2026-07-08 | `tsc --noEmit` on the spike: 2.4 s, no errors |

- **Release-age policy** (D3). npm has `min-release-age` (days) since
  11.10.0 and `min-release-age-exclude` since 11.17.0 [src 22, exp]. With
  `min-release-age=7`, 3 or 1, `npm install @ui-sdk/react @ui-sdk/core`
  fails (`ENOVERSIONS`, `ETARGET … with a date before …`); for the other
  packages npm silently resolves the newest version that is old enough
  (lucide 1.71.0, vite 8.3.7) [exp]. Whether `npm ci` re-checks ages for a
  locked version was not tested.
- **Build time** on the test host (2 cores / 4 threads) [exp]:
  `npm install` cold 56 s, `npm ci` warm 11 s, `vite build` 1.9 s median of
  3, Rust release build cold 281 s, **any change to one Rust file or one
  embedded asset 164 to 168 s** (fat LTO, one codegen unit
  [code `Cargo.toml:27-32`]). `node_modules` is 176 MB in the build stage
  only.
- **Stale binary.** The claim is real, in a narrower form than "across
  checkouts" [exp]:

  | Test | Cargo said | Result |
  |---|---|---|
  | Docker `COPY` of a file dated 2001 | n/a | mtime preserved (BuildKit and the legacy builder) |
  | Two checkouts that differ by a `build.rs`, one target dir, second one's files older than the last build | `Fresh` | correct binary (different compilation unit) |
  | Two checkouts of the same shape, one source string changed, files older than the last build | `Fresh` in 0.15 s | **stale binary**, byte-identical to the other checkout's |
  | An embedded asset rewritten with its old mtime restored | `Fresh` in 0.17 s | **stale binary** |

  Cause: the cache mount id `rh-target` is a constant, every checkout
  builds at `/src`, and cargo decides freshness by mtime
  [code `Dockerfile:14-15`; src 23, 24]. `scripts/deploy.sh` mostly hides it
  because the build-time argument changes every minute
  [code `scripts/deploy.sh:9-14`]; a plain `docker build` does not. The
  client makes it worse: a `dist/` copied from a cached Node layer keeps
  old mtimes. **Proposed fix**, to be proven in phase 0 by repeating the
  two failing rows:
  1. Compute a content hash of `src/`, `static/`, `web/dist`, `build.rs`,
     `Cargo.toml` and `Cargo.lock` in the build stage and pass it to cargo
     as an environment variable that `build.rs` declares with
     `rerun-if-env-changed` and compiles into the binary [src 24, assume:
     this dirties the crate exactly when content changes].
  2. After the build, run `hobnail version` and fail the image unless it
     prints that hash and the git SHA. This turns every stale case into a
     failed build.
  3. `sharing=locked` on the target cache mount, so two builds cannot
     interleave between `cargo build` and `cp` [src 23].
  4. `scripts/cargo.sh` keys its volume on a hash of the checkout path, not
     the folder name [code `scripts/cargo.sh:9`].
- **Dev workflow.** `vite` on `localhost:5173` with
  `server.proxy['/api'] = { target: 'http://127.0.0.1:<port>',
  changeOrigin: false }` (and the same for `/icons`, `/custom.css`,
  `/manifest.webmanifest`). Measured against a stub backend: with
  `changeOrigin: false` the backend sees `Host: localhost:5173` and
  `Origin: http://localhost:5173`; the string shorthand and
  `changeOrigin: true` rewrite `Host` and would fail the same-origin check;
  `Origin`, `Referer` and `Cookie` pass through in every mode; SSE events
  arrived with 0 to 2 ms lag, unbuffered [exp; options: src 30]. `localhost` passes the Host
  guard and `Origin == Host` passes `same_origin`
  [code `src/http/server.rs:157`, `src/auth.rs:536-545`], so **no server
  change and no dev-only allowance is needed**. This pairing was derived
  from the code and the stub measurement; it was not run against a real
  Hobnail with a login. The dev server's RAM is never reported as product
  memory. Vite and Node run in a container (`scripts/web.sh`, new), as
  cargo does today.

### 4.5 Install

| Method | What it needs after the port | Changes from today |
|---|---|---|
| Container image | Docker with Compose; network access to crates.io **and the npm registry** at build time | One more build stage. Same image layout: scratch, one binary, busybox. Same compose file |
| Plain binary under systemd (specified in the local native-install spec; **not built**) | The spec's tarball: binary, unit, env example, `install.sh` | **Nothing.** The spec's rule is "one server binary … the same file, byte for byte"; the client is inside that file, so there is no assets directory to install, own or upgrade |
| Upgrade | Copy `hobnail.db` and `config.toml` first, then replace the image or binary | During Part A: no migration and no new config key, so the copy is a precaution. Part B adds a migration (drafts) and new files; the version's minor number is bumped on any migration (spec decision D10) |
| Rollback | Previous image tag or `hobnail.prev`, plus the copies if the schema moved | An older binary refuses a newer database and runs without history and without sign-in [code `src/db/schema.rs:122-130`, `src/db/mod.rs:403-420`]. With D15 (b) an older binary ignores the board file. If new keys were ever written to `config.toml`, an older binary would fall back to the default config at start [code `src/config.rs:300-314`]; this plan writes none |

**Does "one file, same binary in both installs" still hold?**

- **x86_64: yes.** A scratch image containing only the binary served the
  whole client, deep links included [exp].
- **aarch64: yes by construction, not verified.** The Vite output is
  architecture-independent and is built once on the build platform
  (`FROM --platform=$BUILDPLATFORM`) [src 25]; the same bytes are embedded
  in both cross-compiled binaries. No aarch64 binary exists yet (the native
  spec is not built), so this is an assumption until its smoke test runs.

### 4.6 Dependencies named in the decision

| Dependency | Exists and usable | Stability | Bundle cost [exp unless marked] | Accessibility | Do the decision's instructions still match? |
|---|---|---|---|---|---|
| UI SDK `@ui-sdk/react`, `@ui-sdk/core` | Yes: installed, docs read, Dialog, Select, Tabs and Switch rendered and driven in a browser | **Beta, 5 days old**, 7 releases; repository `vercel-labs/ui` created 2026-10-26; published by CI with provenance [src 1, 2, 3] | 202.6 KB raw / 70.1 KB gzip for the four components (about 165 KB eager; a 36.9 KB sheet chunk loads on demand); `web.css` 82.5 KB / 10.7 KB | Docs cite the WAI-ARIA patterns per component and list axe checks; **no independent audit, no track record** [src 3] | **Yes.** `docs/llms.txt`, `docs/core.md`, `docs/rendering.md` and `web.css` are in the package; per-component entries (`@ui-sdk/react/dialog`), no root import; `@layer ui-sdk;` and the Tailwind line are verbatim in its installation page; the skill command is in the README (the skill lives at `packages/core/skills/ui-sdk`). The `skills` CLI is 1.9.0 and needs Node ≥ 22.20 [src 3, 26]. One addition the decision does not mention: the `csp-nonce` meta tag (4.3) |
| Lucide React | Yes | 1.x since 2026-03; frequent releases [src 6, 17] | 9.3 KB raw / 3.7 KB gzip for 23 icons | Icons are `aria-hidden` by default in v1 [src 6]; labels are the app's job | Yes. Brand icons were removed in v1, so service logos cannot come from Lucide (D20) |
| Recharts | Yes | 3.10.1, no stable release since 2026-07; canaries active [src 18] | 377.7 KB raw / 105.7 KB gzip, of which d3 73 KB, Redux Toolkit and friends 30 KB (raw) | `accessibilityLayer` on by default in v3 [src 27] | Yes. Needs `react-is` as a peer |
| React | Yes, 19.3.1 | stable | 218.8 KB raw / 67.4 KB gzip with `react-dom` | n/a | Yes ("React 19 or newer") |
| Vite | Yes, 8.3.x | stable; Rolldown bundler | build tool only | n/a | Yes. Manual chunking is `build.rolldownOptions.output.codeSplitting.groups`; `manualChunks` is deprecated [exp] |

Closest alternatives, **not substituted**: for UI SDK, Base UI
(`@base-ui/react` 1.9.0, MIT; by its own published figures dialog 27.9,
select 50.1, tabs 17.6, switch 9.7 KB gzip) [src 4]; Radix, React Aria
Components, Ark UI and Headless UI exist but their tree-shaken sizes could
not be fetched. For Recharts: uPlot 21.9 KB gzip (no release in 20 months),
hand-written SVG [src 28].

**Recharts against the server-drawn charts** [code `src/ui/chart.rs`]:

| | Today (`chart.rs`, 1,124 lines of Rust) | Recharts |
|---|---|---|
| Bytes to the browser | a few hundred bytes of SVG per chart, in the page | 106 KB gzip once per build, cached immutably, on chart routes only [exp] |
| Server cost | every chart rendered on every page load | none: the series API already exists |
| Gaps, min/max band, yesterday line | built [code `chart.rs:552-668`] | gaps and areas are standard; the band and compare line are extra series [assume] |
| Shared crosshair across charts, drag to zoom into the URL, clustered event markers | built, about 100 lines of script [code `static/app.js:320-383`] | `syncId` exists; drag-zoom into the URL and marker clustering need custom code. **Not verified** |
| 150 sparklines in one table | one 100 × 24 path each [code `chart.rs:691-744`] | each chart carries a store; **not measured, not recommended** (D4 a) |
| Live point per second | n/a | worked in the spike with 360 points, two series [exp] |
| Works with JavaScript blocked | yes | no |

### 4.7 New product features: what the backend needs

Today's model in one paragraph: identity is names and positions
(containers by name, bookmarks by array index, widgets by a renameable
`id`, channels by table key, rules by name; no generated IDs anywhere)
[code `src/settings/ops.rs:1324-1358`, `src/widgets/mod.rs:273-293`]; every
config table rejects unknown keys [code `src/config.rs:16-217`]; there is
no config schema version; every save is a full-file revision, an event and
a reload broadcast [code `src/settings/store.rs:201-251`]; secrets are
referenced by env-var name or file path and resolved at use
[code `src/widgets/mod.rs:561-575`, `src/alerts/channels.rs:254-273`];
"provider integrations" do not exist: there are two widget types
(`custom-api`, `command`), three channel kinds and Docker discovery.

| Feature | Needs | Exists | Missing |
|---|---|---|---|
| Guided onboarding | a resumable draft; discovery as suggestions; skip everywhere; one commit at the end; no duplicates on retry | Docker discovery; settings forms | a draft store (D15: SQLite table, one migration); "suggestion" state; a commit that writes board, sections and integrations in one guarded save; a session, since writes need one (D12) |
| Integration catalog | provider metadata, config schema, connection test and widgets defined together in Rust; credential storage; multiple instances | the fetch client with `allow_hosts`, a 2 MiB cap and timeouts [code `src/http/client.rs:25,85-103`]; header scrubbing | a provider trait and registry; an `integrations` entity with stable IDs; write-only secret storage (D13); a test endpoint that returns a verdict, never the upstream body; error classes (the client returns strings) |
| Board organisation | boards, sections, membership, order, custom names and icons that survive discovery refreshes; dedupe of discovered and manual entries | `[[groups]]` order, per-container overrides | a board store with generated IDs (D15); a match rule for "the same service": compose `project/service` when present, else the container name [assume]; a visible "missing" state instead of silently dropping an item whose container was renamed by hand |
| Edit mode | positions and sizes per breakpoint; draft, Save, Cancel; a version token; keyboard move and resize | `size`, `columns`, `order` keys that the current pages ignore [code `docs/configuration.md:37-38,66`] | a layout model that is responsive by construction (grid spans, not pixels); one write per session (D17); its own hash so a layout save does not collide with a settings save |
| UI customisation | colours, icons, names, density through forms; preview | theme names; `custom.css` (unrevisioned, applies to every viewer) [code `src/settings/api.rs:684-717`] | typed, validated appearance keys in the board file; an icon picker; preview marked as sample data |

**Every place the decision and the guards pull in different directions:**

| # | The decision says | The current model says | Resolution proposed |
|---|---|---|---|
| T1 | Onboarding is the first-use experience (§3) | "There is no web 'set the first password' form, so nobody can claim the instance before you." [code `docs/features/login.md:17-18`] | D12 (a): console password first, onboarding after sign-in |
| T2 | "Keep unfinished choices in a setup draft" (§3) | every confirmed change is written at once and is a revision; the config rejects unknown keys | Drafts in SQLite, never in the config; a draft holds references to stored secrets, never values |
| T3 | "Enter URL/credentials"; "protected storage, masked fields" (§4) | "Secrets are never shown or stored by the app; forms hold env var names." [code `GOALS.md:209`, `docs/DESIGN.md:193,214`]. The "existing protected storage" the decision refers to does not exist | D13 (b) |
| T4 | a credentialed integration is added from the browser (§4) | "an entry that sends a secret can only get a new destination, or new secret references, in the config file on the host" [code `src/settings/guard.rs:216-299`] | D14 (b): bind at creation |
| T5 | providers define their own widgets (§4, §6) | any widget whose `type` is not exactly `custom-api` counts as command-capable and cannot be created from the app (fail closed) [code `src/settings/guard.rs:63-111`] | Integrations are a separate entity with their own validator; the widget guard is not relaxed; provider code cannot run a program |
| T6 | "Test connections on the backend"; "Preserve a draft when testing fails" (§4) | the preview refuses secret headers to an unsaved destination; a channel test needs a saved channel [code `src/settings/api.rs:153-180,626-632`] | On "Test" the credential is stored bound to its origin and referenced by the draft; abandoned drafts and their secret files are removed after 7 days. The test endpoint is a request-anywhere tool for a session holder, as the preview already is; it obeys `allow_hosts` and returns only a verdict |
| T7 | credentials stay protected (§4) | "Viewing is open" [code `docs/DESIGN.md:206`]: data fetched with a credential would be public | D18 (b) |
| T8 | "stable IDs", discovery as suggestions, "do not dump every discovered container" (§1, §3, §5) | names and positions; "nothing should ever have to be listed just to appear" [code `GOALS.md:52-58`] | D15 (b) and D16 (a) |
| T9 | edit mode with move and resize; "do not silently overwrite another session's changes" (§6) | one revision, one event and one reload per save; Y13 kept per-click saves [code `docs/DESIGN.md:860`] | D17 (a), with the board file's own hash |
| T10 | colours and icons through forms; recognisable icons (§4, §6) | no colour keys; remote icons are opt-in; "no calls to the internet at runtime beyond what the user configures" [code `GOALS.md:163-164`] | Typed appearance keys; D20 (a) |
| T11 | "versioned schema/migration path"; "Preserve supported file-based configuration" (§1) | no version key; a hand-edited file is never rewritten without a signed-in save [code `docs/features/settings.md:20-22`] | `schema = 1` in the new board file only; `config.toml` gains no key; any migration runs only as part of a signed-in save and keeps the previous text |
| T12 | "command widgets file-only" is kept | same | No conflict. The React Settings shows them read-only, as now; the guard and its tests are untouched [code `src/settings/tests.rs:633,789,827,1551`] |

### 4.8 Migration strategy

| | One cut-over | Page by page, side by side |
|---|---|---|
| What ships between start and finish | nothing; a long-lived branch | every phase merges to `main`; the old UI keeps working at its URLs |
| Risk found late | all of it at the end | per page |
| Measuring | one before/after | each phase measured against the last |
| Tests | the HTML-asserting tests break all at once | JSON twins are added phase by phase; HTML tests are deleted with their pages |
| Cost | none during; a large review at the end | both UIs in the binary for a while (+1.18 MB), two test sets |
| Rollback | revert a very large merge | per phase; before the switch nothing users see has changed |

**Recommendation: side by side, with one switch for users.** A mixed
experience (some pages old, some new, each with its own sidebar and a full
reload between them) is worse than either UI, so pages are not swapped at
their real URLs one at a time. Instead:

1. The React app is served under `/app` from phase 0. Assets are at
   `/assets/`. The old pages are untouched.
2. Each phase ports pages and their API, and is verified at
   `/app/<page>` against the old page next to it.
3. **Switch** (phase 5): the page routes serve the React app; `/app/*`
   redirects to the same path without the prefix. `RH_UI=classic` in the
   environment brings the old pages back for one release.
4. **Removal** (phase 6): after the owner has run the switched build and
   signs off, `static/`, `src/ui/pages/`, `src/ui/settings/`,
   `src/ui/shell.rs`, `src/ui/chart.rs`, `src/ui/sprite.rs`,
   `src/logs/pages.rs`, `src/host/view.rs`, `src/render.rs`, the three
   fragment routes, `/more` and the `maud` dependency are deleted. The
   `rust-only` branch stays as the frozen record.

**First vertical slice** (phase 1): shell (sidebar with the stack tree,
top bar with vitals, theme, live indicator), sign-in, the Containers table,
and one container's Overview tab with CPU and memory charts and the "Edit"
override dialog. Why this one: it needs gap items 1, 2, 3 and 11, which
everything else depends on; it is the page with the worst measured browser
cost today (150 rows: 6,035 nodes and a 1,025 ms long task at 4× CPU
[code `docs/qa/QA.md:619`]); and the Edit dialog exercises the whole write
path against an API that already exists.

**Parity checklist** (condensed from the client inventory; each line is
checked in a browser at 1440 and 390 px before the switch):

*Shell.* Correct theme on first paint; theme cycle with its toast; theme
per browser over the config default; skip link; sidebar rail at each
breakpoint, remembered; phone bottom bar and drawer (inert page behind,
Esc, Back, focus in and out); alert count badge; stack tree with counts,
dots, folded state, filter kept across pages; logs icon on every container
and compose stack; vitals chips with sparklines; live indicator states and
the "stopped at HH:MM" pill; reconnect with backoff; stream closed while
hidden; new-build notice; config change picked up without losing input;
updates never move what is being read and wait while the user scrolls;
tab title and favicon problem count; relative times refreshed; Docker
unreachable and config-error banners; account menu and sign out;
installable manifest; version in the sidebar foot.

*Keyboard.* All 57 bindings, off while typing; the shortcuts sheet; ⌘ on a
Mac.

*Command palette.* Opens and closes as today; pages, settings sections,
bookmarks, containers, actions; ranking; "Logs: X", "Open service",
"Edit X"; signed-in add actions; one action per theme; web search row;
no selection when nothing matches; focus returns.

*Overview.* Greeting; "since you last looked"; health strip, vitals with
1 h charts, widgets, top consumers, recent events, bookmarks, all live;
needs-attention rows with reason and logs icon, eight at most; probe line
for a session only; treemap with "+N others"; signed-in "Add" on Bookmarks
and Widgets.

*Containers.* Filter by text, state and stack with counts; sort by six
columns with `aria-sort`; group by stack or flat, remembered; the URL
always holds the view; group headers with strip and collapse; "Showing N";
empty states; live numbers in place; rows follow containers appearing and
going; nothing re-sorts under focus; phone stacked rows with a sort
control; "Logs" on every row, "Open" when there is a link.

*Container detail.* Six tabs that keep the range and the scroll position;
live header; "Why is this red?" on every tab; Edit dialog; reveal env
values on demand; signed-out "Sign in to view" links.

*Charts.* Shared crosshair and tooltip; "no data" for gaps; drag to zoom
into `?from=&to=` with mouse, touch and pen; legend toggles remembered;
local-time axes; event markers that open their timeline row.

*Host.* Live every 3 s while visible; open disclosures and focus survive.

*History and Alerts.* Window, zoom and paging in the URL; rows link to
their subject and to logs at that moment; firing list live; signed-in
channels and rules on Alerts.

*Login.* Password sign-in and return to the page asked for, same-origin
only; wrong password; rate-limit countdown; "no password set" with the
command.

*Logs list.* Filter with count; Enter opens the first match; two to four
ticks for side by side; state in the URL.

*Log viewer.* Starts on and follows the newest line; any scroll up stops
it; "Jump to latest" with a count; the reader's line stays put through
wrap, time, filters and resize; ring cap; pause; search (plain, regex,
case) with counter and stepping, kept in the URL; "only matching" with
context; level chips; ranges including custom; wrap and time toggles;
download; ANSI; JSON folding; stack view with name chips; four panes;
"Load older lines"; `#t=` permalinks; copy line and link; reconnect from
the last line; every pane state; never reloaded by a config change; text
reaches the DOM only as text.

*Settings.* Lock panel when signed out; add and edit dialogs named by the
URL; unsaved-input guard on every way of closing; all validation problems
at once on their fields; busy state; success toast with the revision, list
updated in place, focus back on the row; 422 on the field, 409 with
"Reload and keep my input", 401 with "Sign in (new tab)" and a retry, 403
locked; an edit sends only changed fields; delete with confirm and Undo;
refusal to delete what rules use; rename that follows rules; arrows;
switches that save on change and revert on refusal; toasts (three at most,
never over a dialog); every tab's own controls: General, Containers and
groups (patterns, bulk bar), Bookmarks, Widgets (Fetch now, locked command
widgets, bound fields), Notifications (secret source choices, Send test),
Appearance (default theme, `custom.css` editor with warnings), History and
data (export, typed-phrase clear), Config file (raw editor, validation,
revisions, diff, restore), Security.

*Accessibility and motion.* Roles and politeness of every status region;
one visible focus ring; reduced motion makes everything instant; 44 px
targets and 16 px inputs on touch; scales with the browser font size; no
sideways scroll at 390 px; status never by colour alone; a user's
`custom.css` and the retired token names keep working.

*Deliberately not carried over* (D6): the `/more` page, plain-form
fallbacks, "Saving needs JavaScript" notes, the no-script log download
note.

### 4.9 Budgets and measurement

**Method** (the decision's §2 rules). One application container, no memory
limit. Sampled from the host's cgroup files: raw `memory.current`; working
set = `memory.current − inactive_file` (the formula `docker stats` uses);
`memory.peak`; `anon`, `file`, `kernel`, `sock`, `file_mapped` from
`memory.stat`; `cpu.stat` per phase; errors and latency from the load
clients; `RssAnon`/`RssFile` of the process at phase ends. Phases: start-up
10 s at 100 ms; idle 90 s at 500 ms; 1, 10 and 25 clients, 60 s each at
500 ms; after load 60 s. A client holds one event stream, reads state every
2 s and history every 5 s, and downloads every client asset at the start
and every 20 s. Samples are appended to a CSV as they are taken. Each
figure is reported as median and maximum with its sample count; cold-cache
and warm-cache runs are labelled; browser, Docker daemon and build-tool
memory are never included. The spike's sampler and client are the starting
point for `scripts/bench-mem.sh` (new).

**Measured for this plan** [exp]: the unmodified server ("base") against
the same server with the React build embedded as raw + gzip ("embedded"),
empty config, empty database, the host's containers through a read-only
socket, 2-core / 4-thread x86_64 host with about 7.2 GiB, warm cache.
Base ran twice; values are MiB.

| Phase | Samples | Base `memory.current` median / max | Embedded median / max |
|---|---|---|---|
| Idle | 180 | 1.61, 1.66 / 2.10, 2.23 | 1.66 / 2.12 |
| 1 client | 120 | 2.21, 2.15 / 2.62, 2.64 | 2.25 / 2.75 |
| 10 clients | 120 | 3.08, 3.14 / 3.77, 3.82 | 3.16 / 3.70 |
| 25 clients | 120 | 4.28, 4.42 / 4.98, 5.32 | 4.36 / 5.05 |
| After load | 120 | 2.48, 2.40 / 3.07, 3.01 | 2.35 / 2.77 |
| `memory.peak`, whole run | kernel | 6.34, 6.21 | 6.37 |

Zero errors in every phase; 850 to 900 stream events with 25 clients; in
the 25-client phase, 775 to 800 state reads, 325 history reads and, for the
embedded build, 100 `index.html` and 500 asset downloads. Start to first
`/healthz` 200: 361 and 437 ms base, 412 ms embedded.

Limits of these numbers, stated plainly: an **empty** config and database
(the long-running test instance with a real config read 6.1 MiB in one
`docker stats` reading [exp]); warm cache only; the data folder was on
tmpfs, so the database was charged as shared memory and working set
equalled `memory.current`; the container healthcheck was misconfigured in
the spike and never ran the binary; the load used today's HTML-era
endpoints, not the JSON API that does not exist yet. They show what
embedding costs. They are not the product's numbers.

**Proposed budgets** (for the decision's representative workload on the
real backend with a real config; every one is a proposal):

| Metric | Budget | Basis |
|---|---|---|
| Container RAM, idle, median of `memory.current` | ≤ 8 MiB | today 3.8 MiB by `docker stats` on a busy database [code `docs/measurements.md:238`]; target "under 10" |
| Container RAM, 1 / 10 / 25 clients, maximum sample | ≤ 10 MiB | decision §2 preferred target; spike maximum 5.1 MiB on an empty instance [exp] |
| `memory.peak` over the whole run | reported; must stay ≤ 20 MiB | decision §2 limit |
| Regression rule | idle median may not rise by more than 0.5 MiB from one phase to the next without a written cause | assumption |
| Image, `docker images` | ≤ 15 MB while both UIs ship; ≤ 13 MB after removal | 11.6 MB today [code `GOALS.md:23`]; +1.18 MB binary measured; the removal saving is an estimate |
| Binary cold start, exec to first 200 | ≤ 50 ms (unchanged stretch) | 3.5 ms today; embedded assets are not read at start [exp: idle `RssFile` equal] |
| Client, first route: `index.html` + entry JS + CSS | ≤ 180 KB gzip | spike 143.2 KB gzip / 490.1 KB raw [exp]; today's first view is 63.3 KB gzip-equivalent but sent uncompressed as 227.5 KB [code, sizes of `static/`] |
| Client, any lazy route chunk | ≤ 60 KB gzip; charts vendor chunk ≤ 110 KB gzip | spike: Recharts 105.7 KB gzip [exp] |
| Client, whole `dist` | ≤ 1.3 MB raw / 400 KB gzip | spike 907 KB / 264 KB [exp]; decision's experiment 982 KB / 286 KB. Raw + gzip is what the binary grows by |
| LCP, cold HTTP cache, loopback | desktop ≤ 400 ms; phone profile at 4× CPU ≤ 1,200 ms | today 120 to 284 ms desktop and 240 to 676 ms phone 4× with server-rendered pages [src 7]; a client-rendered first load is expected to be slower [assume] |
| Route change inside the app | ≤ 200 ms desktop; ≤ 500 ms phone 4× | today a tab change is a full load: 218 ms and 524 ms [src 7] |
| INP | ≤ 200 ms on every scripted interaction | web.dev "good" threshold [src 29]; today 48 to 240 ms [src 7] |
| Containers with 150 rows, phone 4× | no task over 200 ms on load; live update ≤ 50 ms | today 1,025 ms and 90 to 155 ms (ROB-05) [code `docs/qa/QA.md:619`] |
| Log burst of 1,500 lines | no task over 100 ms | today 48 tasks of 60 to 780 ms (LG-18) [code `docs/qa/QA.md:498`] |
| Update frequency | system 3 s, container stats 5 s, as today; at most one render per animation frame | [code `src/live.rs:12-13`] |
| CLS | ≤ 0.1 | web.dev [src 29] |

The bundle budgets are enforced by `npm run size` in the Node stage; the
others are measured at the end of each phase and recorded in
`docs/measurements.md`. A miss blocks the phase, as GOALS.md already
requires [code `GOALS.md:45-46`].

### 4.10 Tests

| | Carries over | Changes | New |
|---|---|---|---|
| Rust unit tests | config, widgets, alerts, database, auth, logs framing and matcher, settings ops and guards (7,143 lines in the dedicated test files, plus the tests inside modules) | view-model tests in `src/ui/view/tests.rs` stay as serialiser tests; 2,806 lines of tests that assert HTML (`src/ui/pages/tests.rs`, `src/ui/settings/tests.rs`, parts of `src/ui/view/tests.rs`) and `src/http/server.rs:439-572` are deleted with their pages | serialiser privacy tests (visitor vs session, hidden containers); asset table tests (encoding choice, cache headers, 404, deep link); type-drift test; formatter fixtures |
| Gate tests (`src/http/gate_tests.rs`, 10 tests) | 1-4 and 6 are protocol tests and stay byte for byte: nothing writable before a password; every write needs session, CSRF and same origin; settings reads need a session; login refuses a foreign origin and bad redirects; a plain form post is refused with the exact body | 5, 7, 8, 9, 10 assert server-rendered HTML (`?frag=list`, `rh-csrf` meta, `data-hash`, `no-store` on pages). Their invariants get JSON twins **when the matching API lands**; the HTML versions are deleted only in phase 6 | `/api/v1` read table walked as a visitor; `/api/v1/session` returns no token without a cookie; every new write route added to `WRITES`; `index.html` and assets carry the expected headers and CSP |
| Browser checks (`scripts/*.mjs`: raw Chrome DevTools Protocol, host Node, a cached headless shell; no `package.json`) | `shots.sh`; the server-side third of `settings-check`; `a11y-check`'s generic rules | `nav-check`, `settings-check`, `logs-check` (depends on `window.__rhLogs`), `motion-check`: selectors and hooks rewritten per phase. `smooth-check` (assumes a document load per click), `ui-check` and `phone-scroll-test` are replaced | the same checks run from a container so no host Node is needed; a CSP check that fails on any violation report; a memory benchmark script; a bundle-size check |
| Client | n/a | n/a | `tsc --noEmit`; unit tests for formatters, the router, the stream reducer and the log ring; component tests for dialog guard, validation and the 409/401 flows |

**Must pass before a phase ends:** the image builds with every gate; all
Rust tests; the phase's browser checks at 1440 and 390 px, dark and light,
with screenshots looked at by a person (decision §8.6: a passing build or
an HTTP 200 is not a visual check); the CSP check with zero violations; the
phase's measurements inside budget; no entry in the parity checklist for
the ported pages left open.

### 4.11 What was run for this plan

All in throwaway containers on a private Docker network or on 127.0.0.1
ports 3395 to 3399; nothing touched the live or the test instance beyond
`GET` requests and one `docker stats` reading of the test instance;
nothing was installed on the host. Everything created was removed and the
removal was checked (containers, volumes, networks and images, including
the Node, Rust and headless-Chromium images that were pulled).

1. **Release-age test**: `npm install` with `min-release-age` 7, 3 and 1
   for each named package (Node 26.13.1, npm 11.20.0).
2. **Client spike**: a 411-line React 19 / Vite 8 / TypeScript 7 app with
   UI SDK (Tabs, Switch, Select, Dialog), 23 Lucide icons, a 150-row
   table, a Recharts chart and the same chart in hand-written SVG; default
   and per-vendor builds; sizes raw, gzip -9 and brotli 11; headless
   Chromium 158 driven over DevTools for the CSP matrix and the
   `Accept-Encoding` capture; a stub backend for the three Vite proxy
   modes and SSE.
3. **Server spike**: a copy of the repository at `HEAD`, patched outside
   the repository with a `build.rs` asset table and three router arms (no
   new crate; fmt, clippy and tests not run); base and two embed variants
   built; five measured runs of the six-phase workload; mtime and
   shared-target tests.

Two deviations to know about. The client spike installed UI SDK when it was
under one day old, with a release-age exception for those two packages
only, scripts disabled, in a container with nothing but the spike folder
mounted; the product must not do that (D3). And the server spike's
healthcheck and tmpfs data folder differ from a real deployment (4.9).

### 4.12 The 14 deferred QA findings

| Finding | What the port does |
|---|---|
| SET-42 reorder arrows save on every click | Unchanged in Part A (Y13). Closed by edit mode's draft if D17 is (a) |
| SET-46 "Fetch now" hides the raw response | Stays deferred (Y19). Provider integrations reduce the need |
| SET-69 raw editor on a phone does not wrap | Fixed in phase 4: the editor is rebuilt anyway |
| NAV-81 four different sign-in prompts | Closed by construction: one prompt component |
| OV-27 sparklines start empty, each on its own scale | Addressed in phase 1 with one batched history query when the page opens, measured; it costs nothing while nobody views. If the query misses its budget, it stays deferred |
| OV-29, OV-32 toolbar sizes; native `<select>` | Closed by construction: one control size, the UI SDK select |
| LG-18 a log burst blocks the page | Acceptance check of phase 3 (4.9) |
| A11Y-19 targets under 24 px on desktop | Closed by a minimum hit-size token in the component layer; `a11y-check` enforces it |
| ROB-04 no response compression | Static files: closed in phase 0 by pre-compressed embedding, no crate. JSON and the stream: D9 |
| ROB-05 150-row table, 1 s long task | Acceptance check of phase 1: only visible rows are rendered |
| ROB-10 blank first load on a slow line | Changes shape. First ever load needs about 143 KB gzip before anything paints, about 2.9 s at 400 kbit/s [assume, from the spike's size]; today first paint is 1.8 to 2.7 s and the full 150-row page 9.5 s. Later loads come from the immutable cache. Measured at the end of phase 1; reported, not assumed fixed |
| ROB-14 widget "loading…" and raw errors | The client keeps a widget's last value and its age while the tab is open, with no server state. A fresh load still starts empty |
| ROB-17 dead controls without JavaScript | Superseded by D6. The bare-JSON wrong-password page disappears with the form post |

## 5. The plan

Part A needs D1 to D11. Part B needs D12 to D21. Phase lengths are not
estimated; each phase ends on its acceptance checks, not a date.

### Part A: port to parity

No database migration, no new key in `config.toml`, no change to
`src/settings/guard.rs` or `src/auth.rs` beyond the session endpoint.

#### Phase 0: foundations

- **Scope.** `web/` project (exact versions, lock file, `.npmrc`,
  `AGENTS.md` with the SDK reading instructions, the project-local UI SDK
  skill); Node build stage; `build.rs` asset table and `/assets/*`,
  `/app/*` serving with CSP and `noscript`; `/api/v1` skeleton: error
  shape, the read-access table, `GET /api/v1/session`; the type pipeline;
  the stale-binary fix; containerised browser-check runner;
  `scripts/bench-mem.sh`; design tokens ported from `static/app.css` as
  they are (DESIGN.md §11.2, §11.3) and the wrapper folder for UI SDK
  components.
- **Files.** New: `web/**`, `build.rs`, `src/http/assets.rs`, `src/api/`
  (`mod.rs`, `error.rs`, `access.rs`, `session.rs`), `scripts/web.sh`,
  `scripts/e2e.sh`, `scripts/bench-mem.sh`. Changed: `Dockerfile`,
  `.dockerignore`, `Cargo.toml` (`build = "build.rs"`, dev-dependency),
  `src/http/server.rs` (two match arms and the `/api/v1` dispatch),
  `src/http/gate_tests.rs` (additions), `scripts/cargo.sh`,
  `scripts/deploy.sh` (hash argument), `docs/install.md`.
- **Parallel.** Build and serving; API skeleton and types; test runner and
  benchmark script.
- **Acceptance.** The image builds with all gates. `/app` shows an empty
  shell that signs in and out through the real login. The two stale-binary
  cases from 4.4 now fail or rebuild. Gate tests pass with the new routes
  in `WRITES`. CSP check: zero violations. All existing pages and browser
  checks unchanged.
- **Measure.** Binary and image size against `main`; the full memory
  workload on base and on this build with the real seed config (the first
  real numbers); cold start; build times.
- **Rollback.** Revert the merge; no data touched.
- **Owner signs off.** Budgets in 4.9; the pinned dependency set; that
  `/app` may exist on the live instance.

#### Phase 1: first vertical slice

- **Scope.** Shell; sign-in page; Containers; container Overview tab with
  charts; Edit override dialog; the JSON stream with `containers`, `stats`,
  `system` topics. API: `GET /api/v1/bootstrap`, `/containers`,
  `/containers/<name>`, `/strips`, `/stream`.
- **Files.** `src/api/{bootstrap,containers,stream}.rs`;
  `src/ui/view/*` gains `Serialize` and loses nothing; `web/src/{app,
  routes/containers, routes/container, ui, api, live}`.
- **Parallel.** Rust endpoints against agreed types; shell and components;
  the two pages.
- **Acceptance.** Parity checklist items for Shell, Keyboard (global and
  table), Containers, Container detail (Overview), Charts. 150 rows at
  phone 4×: no task over 200 ms. An Edit save, a 409 with "Reload and keep
  my input", a 401 recovery and a foreign-origin refusal all behave as
  today. Go/no-go on UI SDK (D2) and on Recharts for linked charts (D4).
- **Measure.** Bundle sizes; LCP, INP, long tasks at both profiles; memory
  workload with the JSON stream at 1, 10 and 25 clients; CPU of one viewer
  against today's 0.58 % of a core [src 7].
- **Rollback.** `/app` is not linked from the old UI; revert the merge.
- **Owner signs off.** The look and feel of the design system on real
  pages; the two go/no-go results; the measured numbers.

#### Phase 2: read pages

- **Scope.** Overview; History; Alerts (visitor view); Host; container
  Metrics, Inspect, Processes and Events tabs; command palette. API: gap
  items 4 to 10; `from`/`to` windows; `widget` and `alerts` topics; a
  `host` topic that runs the process scan only while subscribed.
- **Parallel.** One Rust endpoint and one page per pair; five pairs.
- **Acceptance.** Checklist items for those pages; visitor privacy tests
  for every new endpoint; JSON twin of gate test 5.
- **Measure.** As phase 1, plus JSON response sizes with 150 containers
  (input to D9) and the Host page's viewer CPU against today's 1.38 %.
- **Rollback.** As phase 1.
- **Owner signs off.** Overview layout; D9.

#### Phase 3: logs

- **Scope.** Logs list, single container, stack, side by side. The stream
  API already exists and does not change; one new endpoint for the picker.
- **Acceptance.** Every Log viewer and Logs list checklist item;
  `logs-check` rewritten and passing at both widths; no task over 100 ms
  in the burst test; text reaches the DOM only as text nodes; viewer
  leaves → Docker connection closed within 5 s (today 53 ms
  [code `docs/measurements.md:169`]).
- **Measure.** Memory with one viewer on a busy log against today's
  6.05 MiB [code `docs/measurements.md:168`]; search time over 5,000
  lines against today's 4 to 16 ms.
- **Rollback.** As phase 1.
- **Owner signs off.** The log viewer, since it is the page kept open
  longest.

#### Phase 4: settings and signed-in editing

- **Scope.** Nine Settings tabs; the signed-in parts of Overview and
  Alerts; palette actions. The settings API is used as it is.
- **Parallel.** By tab, after the dialog, form and list components exist.
- **Acceptance.** Every Settings checklist item; `settings-check` rewritten
  and passing; JSON twins of gate tests 7 to 10; the command-widget,
  secret-path and binding guards refuse exactly what they refuse today
  (their Rust tests are untouched).
- **Measure.** Bundle of the Settings routes; memory during repeated
  config reads and writes (the decision's 10 s cycle).
- **Rollback.** As phase 1.
- **Owner signs off.** Settings as a whole.

#### Phase 5: parity, measure, switch

- **Scope.** The whole parity checklist walked; all browser checks; the
  full memory benchmark from decision §8.8 on the real backend, saved raw;
  the route switch with `RH_UI=classic`; the wording from section 3 put
  into the documents.
- **Acceptance.** Checklist complete; every budget in 4.9 met or the miss
  written up for the owner; visual review at 1440, 900 and 390 px, dark,
  light, ember and high contrast.
- **Rollback.** Set `RH_UI=classic` and restart. No data difference.
- **Owner signs off.** The switch itself, on the test instance first. The
  live instance is deployed by the owner, not by an agent.

#### Phase 6: remove the old UI

- **Scope.** Delete what 4.8 lists; drop `maud`; delete the HTML-asserting
  tests; remove `RH_UI`.
- **Acceptance.** Image builds; binary and image smaller than phase 5;
  gate tests 1-4 and 6 unchanged and passing, twins passing.
- **Measure.** Size and the memory workload again: this is the Part A
  result.
- **Rollback.** The previous image; `rust-only` remains the frozen record.
- **Owner signs off.** Deletion, after running the switched build for as
  long as they choose.

### Part B: the five product outcomes

Order follows decision §8: onboarding and a first board, then providers
incrementally. Each phase adds to the React Settings so setup and later
editing share forms.

#### Phase 7: configuration foundations

- **Scope.** Board file with `schema = 1`, generated IDs, its own hash and
  lossless edits (D15); migration for the draft table; write-only secret
  storage and origin binding with guard tests (D13, D14); the integration
  entity, provider trait and registry with the first one or two providers
  from D19; per-integration sign-in rule (D18).
- **Files.** `src/board/`, `src/integrations/`, `src/settings/guard.rs`
  (additions and tests), `src/db/schema.rs` (one migration),
  `src/api/{boards,integrations}.rs`.
- **Acceptance.** An older binary starts with the new data folder and
  shows its usual pages; a stolen-session test cannot move a stored
  credential to another origin or read it back; command widgets remain
  uncreatable from any new route.
- **Rollback.** Previous binary plus the pre-upgrade database copy (first
  migration since v6).
- **Owner signs off.** The security review of this phase before anything
  is built on it.

#### Phase 8: onboarding and a first board

- **Scope.** Appearance → Discover → Select → Configure connections →
  Review, with a server-side draft, skip and back on every step, resume in
  a new browser session, one commit, no duplicates on retry; the default
  board from the selection; a short tour of real controls.
- **Acceptance.** Decision §3 line by line; missing Docker access does not
  block manual setup; a failed connection test does not block the rest;
  save, reload, container restart and a new browser session all keep the
  board.

#### Phase 9: organisation, edit mode, customisation

- **Scope.** Boards and sections; move, reorder and resize with keyboard
  equivalents; responsive spans; widget configuration forms; appearance
  form; preview with sample data marked as such; two-session conflict
  handling (D17).
- **Acceptance.** Decision §5 and §6 line by line; a layout made at 1440 px
  is usable at 390 px; a second session's save is never silently
  overwritten.

#### Phase 10: integration catalog

- **Scope.** Searchable catalog; choose → URL and credentials → test →
  widgets → add; the remaining providers from D19, one at a time.
- **Acceptance.** Decision §4 line by line; only real providers are listed;
  a saved link is never shown as healthy.

#### Phase 11: acceptance

- **Scope.** The five outcomes checked against decision §8 "Acceptance";
  the full memory benchmark with the chosen integrations; documents
  updated; a written report of what works, which integrations are real and
  what remains.

## 6. Install after the port

### Container image

```sh
git clone https://github.com/<owner>/hobnail.git
cd hobnail
docker build -t hobnail:latest .          # builds the client (Node stage), then the server
docker compose -f deploy/compose.yaml up -d
docker exec -it hobnail /hobnail set-password
```

Open `http://127.0.0.1:3200`. The build needs network access to crates.io
and the npm registry; nothing but Docker is needed on the host. The compose
file, the data folder and the environment variables are unchanged.

### Plain binary under systemd

As the native-install spec describes it; the port adds no step and no
file. (The spec is approved but not built, so these are the steps it
defines, not steps that have been run.)

```sh
docker buildx build --platform linux/amd64,linux/arm64 --target bin \
  --output type=local,dest=dist .
# package dist/<platform>/hobnail with the unit, env example and install.sh
sha256sum -c --ignore-missing SHA256SUMS
tar xzf hobnail-<version>-<arch>-linux-musl.tar.gz && cd hobnail-<version>-<arch>-linux-musl
sudo ./install.sh --start
sudo -u hobnail RH_DATA_DIR=/var/lib/hobnail /usr/local/bin/hobnail set-password
```

### Upgrade and rollback

```sh
# container
docker compose -f deploy/compose.yaml stop hobnail
cp -a data data.pre-<old version>          # config.toml, hobnail.db, board file
git pull && docker build -t hobnail:latest -t hobnail:<new sha> .
docker compose -f deploy/compose.yaml up -d
# rollback: point the compose file at hobnail:<old sha>, restore the copy if
# the release notes say the database schema moved, then `up -d`

# native
sudo ./install.sh        # from the new tarball: keeps hobnail.prev and a database copy
# rollback: stop, move hobnail.prev back, restore the copy if the schema moved, start
```

Open browser tabs notice the new build and offer to reload. The database
migrates forward only; a release that contains a migration says so in its
version number and notes. Part A contains none.

## 7. Risks and what is still unknown

**Risks**

| Risk | Why it matters | Mitigation |
|---|---|---|
| UI SDK is a days-old beta | API churn; accessibility unproven; one publisher | Wrapper folder; pinned version; go/no-go after phase 1; Base UI named as the fallback, not adopted |
| Recharts cannot match the current charts | linked crosshair, zoom into the URL and clustered markers are custom work; the library has had no stable release in four months | Prove on one chart in phase 1 before porting the other pages |
| First load is heavier than today | 143 KB gzip of script before first paint, against server-rendered HTML | Budget and measure in phase 1; immutable caching; route-level chunks |
| The log viewer's follow and anchor logic | the most intricate client code today, tuned over many fixes | Its own phase; `logs-check` rewritten first; real DOM lines with a ring before any virtualisation |
| Two UIs in one binary for a while | size, two test sets, drift | The old UI is frozen from phase 0: fixes only |
| Rebuild cost | any client change relinks the binary: about 165 s | Vite dev server for client work; the image build is the gate, not the inner loop |
| Stored credentials (Part B) | a new class of data at rest; a wrong binding check leaks a token | A separate phase with its own review; guard tests written before the feature |
| Privacy regressions in JSON | rules enforced in HTML templates today must be re-stated in serialisers | One access table, visitor tests per endpoint, hidden containers tested explicitly |
| Six connections per origin | one live stream plus up to four log streams per tab | One multiplexed live stream; closed when hidden |

**Not verified**

- Cold-cache memory. Everything measured was warm. The bound is the binary
  growth, +1.13 MiB of reclaimable file pages.
- Memory and CPU of the real backend with a real config serving the JSON
  API to 1, 10 and 25 clients. The spike used an empty instance and today's
  endpoints.
- Whether `ts-rs` under `cfg(test)` leaves the release binary unchanged.
- The stale-binary fix: designed from cargo's documentation, not run.
- The Vite proxy against a real Hobnail with a signed-in write.
- CSP in Firefox and Safari; visual correctness of UI SDK components with
  constructed style sheets on those browsers.
- Any aarch64 build.
- Whether `npm ci` enforces `min-release-age` for versions already in the
  lock file.
- JSON payload sizes with 150 containers.
- First paint on a slow link.
- How hyper answers `HEAD` for the new routes.
- UI SDK's accessibility under a screen reader; only its documentation was
  read.
- Tree-shaken sizes of Radix, React Aria Components, Ark UI and Headless
  UI (the size service timed out), and a tree-shaken Recharts figure from
  any source but the spike.
- Lint tooling for TypeScript 7: typescript-eslint needs the TypeScript 6
  compatibility package [src 21]; the choice is left to phase 0.

## 8. Sources

All read 2026-11-17 unless noted.

1. npm registry, `@ui-sdk/react`: https://registry.npmjs.org/@ui-sdk%2Freact
2. npm registry, `@ui-sdk/core`: https://registry.npmjs.org/@ui-sdk%2Fcore
3. UI SDK repository, README and documentation shipped in the package:
   https://raw.githubusercontent.com/vercel-labs/ui/main/README.md ,
   https://unpkg.com/@ui-sdk/react@0.1.0-beta.6/docs/llms.txt ,
   https://ui-sdk.dev/
4. Base UI: https://registry.npmjs.org/@base-ui%2Freact (sizes from
   https://deno.bundlejs.com/ for `@base-ui/react@1.9.0/{dialog,select,tabs,switch}`)
5. ts-rs: https://raw.githubusercontent.com/Aleph-Alpha/ts-rs/main/README.md
6. Lucide 1.0 release notes:
   https://github.com/lucide-icons/lucide/releases/tag/1.0.0 ; package
   guide: https://lucide.dev/guide/packages/lucide-react
7. This project's bench-phase baseline (2026-11-15), a local working note;
   its results are summarised in `docs/measurements.md` ("ux-pass bench
   phase") and `docs/qa/QA.md`.
8. rust-embed 8.14.0: https://crates.io/api/v1/crates/rust-embed
9. memory-serve: https://raw.githubusercontent.com/tweedegolf/memory-serve/main/README.md
10. tower-http 0.7.1 `ServeDir`:
    https://raw.githubusercontent.com/tower-rs/tower-http/main/tower-http/CHANGELOG.md
11. Linux kernel, cgroup v2: https://docs.kernel.org/admin-guide/cgroup-v2.html
12. Vite build options: https://vite.dev/config/build-options
13. MDN, `Cache-Control` and HTTP caching:
    https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Cache-Control ,
    https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/Caching
14. Vite, "Building for Production" (load error handling):
    https://vite.dev/guide/build
15. Node.js release schedule:
    https://raw.githubusercontent.com/nodejs/Release/main/schedule.json ;
    https://nodejs.org/dist/index.json
16. `npm ci`: https://docs.npmjs.com/cli/v11/commands/npm-ci
17. npm registry, `lucide-react`: https://registry.npmjs.org/lucide-react
18. npm registry, `recharts`: https://registry.npmjs.org/recharts ;
    migration guide: https://github.com/recharts/recharts/wiki/3.0-migration-guide
19. React 19.3: https://react.dev/blog/2026/09/09/react-19-3 ;
    https://registry.npmjs.org/react
20. Vite 8: https://vite.dev/blog/announcing-vite8 ;
    https://registry.npmjs.org/vite ;
    https://registry.npmjs.org/@vitejs%2Fplugin-react
21. TypeScript 7.0:
    https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/
22. npm configuration (`min-release-age`, `min-release-age-exclude`):
    https://docs.npmjs.com/cli/v11/using-npm/config
23. Dockerfile reference, cache mounts:
    https://docs.docker.com/reference/dockerfile/#run---mounttypecache ;
    build cache invalidation: https://docs.docker.com/build/cache/invalidation/
24. Cargo build scripts (`rerun-if-changed`, `rerun-if-env-changed`):
    https://doc.rust-lang.org/cargo/reference/build-scripts.html ;
    checksum freshness (unstable):
    https://doc.rust-lang.org/cargo/reference/unstable.html#checksum-freshness
25. Docker multi-platform builds:
    https://docs.docker.com/build/building/multi-platform/
26. npm registry, `skills`: https://registry.npmjs.org/skills
27. Recharts and accessibility:
    https://github.com/recharts/recharts/wiki/Recharts-and-accessibility
28. uPlot: https://registry.npmjs.org/uplot
29. web.dev, INP, LCP, CLS thresholds: https://web.dev/articles/inp ,
    https://web.dev/articles/lcp , https://web.dev/articles/cls
30. Vite server options (proxy): https://vite.dev/config/server-options
31. MDN, CSP `style-src`:
    https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Security-Policy/style-src
