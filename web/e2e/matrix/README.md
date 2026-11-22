# Cross-browser matrix

`scripts/e2e-matrix.sh` runs Hobnail's user flows in **Chromium, Firefox and
WebKit** at phone, tablet and desktop sizes, and checks every page state the
flows reach. Nothing runs on the host but Docker; nothing is added to
`web/package.json`.

```sh
DOCKER_BUILDKIT=1 docker build --build-arg RH_GIT_SHA=$(git rev-parse --short HEAD) -t hobnail-m6:$(git rev-parse --short HEAD) .
TMPDIR=/some/scratch scripts/e2e-matrix.sh hobnail-m6:<sha>
# narrower runs:
ENGINES=webkit PLAN=plan.txt FLOWS="logs,boards" scripts/e2e-matrix.sh hobnail-m6:<sha>
```

`PLAN` is a file of `unit profile theme motion` lines (see the default plan
in the script). Output goes to `$OUT` (default: a new directory under
`$TMPDIR`): one JSON file per unit, `<engine>.log`, `summary.txt` (the
engine × profile × flow table, problem counts by kind, and the distinct
problems, the engine messages set aside and why), and JPEG screenshots of
failing states in `shots/`. Exit status is 0 only when `summary.txt` says
`RESULT: PASS` (a summary that crashed or is missing fails), that is, when
every flow passed
with no problem on any state and every unit of the plan (listed in
`expected.txt`: plan lines times engines) wrote a result; an instance or
browser that fails to start writes a fatal result.

A full run (3 engines in parallel, 26 units each: 78 units, 330 flow runs
and about 2 850 checked page states) takes about 70-95 minutes on this host
and writes 1-6 MB (screenshots only of failing states). Each flows unit
takes 5-7 minutes, each sweep 2-3.

## How it works

- **Browsers:** `mcr.microsoft.com/playwright:v1.64.1-noble`, pinned by
  digest `sha256:2f05f8a7…8768f` (Chromium 156, Firefox 157, WebKit 27.2). `playwright-core` 1.64.1 and `axe-core` 4.14.0
  come from `package-lock.json` in this folder, installed with
  `npm ci --ignore-scripts` into the Docker volume `hobnail-m6-pw` (rebuilt
  when the lock file changes, or with `PW_REBUILD=1`). Both were published
  more than 7 days before they were adopted and have no install scripts.
- **Instances:** every unit starts a fresh throwaway instance with
  `web/e2e/fake-instance.sh` (150 fake containers in 13 stacks, the M2
  provider fixtures for Pi-hole and HTTP on port 8081, log fixtures, a
  container stopping and starting every 30 s). Engines get ports 3395, 3396
  and 3397. Port 3200 and the container `hobnail` are refused everywhere
  (`lib.mjs` `assertTarget`, `fake-instance.sh`). No third-party service
  is contacted: every browser context aborts any request to a host other
  than 127.0.0.1 or localhost and reports it as a `network` problem.
- **Units:** `flows` (fresh instance; onboarding through every step with a
  tested Pi-hole connection, then every flow below), `skip` (fresh
  instance; onboarding skipping every step, then "tour stays closed" and
  login and logout), and
  `sweep` (owner and a board created through the API, then every route of
  `../sweep-fixture.mjs` (27 pages: overview, containers, the five container
  tabs, host, history, alerts, four logs views, a board, the 12 settings
  tabs, account) plus the login page).

### Flows (`flows.mjs`)

onboarding (every step / skip, including the board tour, which must
appear on the first board visit and be saved as seen) · tour stays closed
(straight after onboarding: revisit the board, save an edit, leave and come
back in the app, reload; the tour must not open again) · login and logout
(the account button names its visible initials, wrong password, the
rate-limit pause, sign in again) · overview, host, alerts (each stack's
fold toggle says "Fold" or "Show" and its stack; the process table is a
named region) · containers (filter, open a container, all five detail tabs) · logs
(live tail, search filter, download — opening "Filters" on phones — picker,
multi-pane) · history charts (hover or tap read-out, container metrics) ·
boards (edit, add a link, save; edit, cancel) · catalog → add Pi-hole →
test → choose widgets → save to board → tile on the board · settings (all
12 tabs, save and restore a setting, account) · users and invites (create,
accept in a second browser context with its own checks, listed) · command palette (open, type,
Enter, Escape) · keyboard only (skip link, 30 Tab stops all visibly
focused and never back on `<main>`, palette to settings, filter, account menu with Escape returning
focus) · touch (drawer, tap navigation, tap a row, palette; touch profiles)
· SSE reconnect after a network drop · tablet rotation (portrait ↔
landscape on three pages and with the palette open; tablet profiles).

**Network drop.** The browser context goes offline and the runner restarts
the throwaway server (a file under `$OUT` asks the host-side watcher in
`e2e-matrix.sh` to `docker restart hobnail-p0-h-<port>`), which cuts the
open event stream; the flow requires "reconnecting", the "Live updates
stopped" pill, then "live" again and the pill gone after going back online.
Offline emulation alone does not close an already-open `EventSource` in
Chromium (measured: the status stayed "live" for 20 s), hence the restart.

### Checks on every page state (`lib.mjs`)

| Check | Rule |
|---|---|
| console | no `console.error` and no uncaught page error, counting everything since the previous state (navigation, clicks, saves) and what is left after a flow's last state; `console.error(object)` is reported with its arguments. A flow may allow named, expected ones, matched against the message and its URL: the 401 of a wrong password, the 429 pause, and, while the network is down, failures of the event stream (`/api/v1/stream`) only |
| CSP | no `securitypolicyviolation` event and no CSP console report (same accounting as console) |
| network | no request to anything but the throwaway instance |
| sideways scroll | `scrollWidth` of the document and body ≤ viewport + 1 px |
| clipped controls | no visible control cut by a non-scrolling ancestor, outside the viewport, or with its own text cut without an ellipsis (designed truncation: an ellipsis, with the control still at the page's target size wide) |
| targets | `../hit-targets.mjs` (the whole target must be the control's own): 44 px where the design promises it (`pointer:coarse` or under 700 px, `shell.css` A11Y-18), 24 px (WCAG 2.5.8) for a fine pointer; with a dialog, menu, listbox or the tour open, its own controls; toasts as a scope of their own. A measurement that throws, or none on a page with visible controls, is a `harness` problem |
| focus | two key presses (Tab; arrows in a menu or listbox), on the page or inside the open layer (focus goes back where it was): each focused control matches `:focus-visible`, is on screen, stays inside a modal dialog, and shows a ring: its outline (visible, not transparent) or box shadow, or a near ancestor's, differs from its blurred look |
| reduced motion | under `prefers-reduced-motion: reduce`, no running transition or animation |
| axe | `axe.run(document)` with axe-core's default rules (WCAG 2.0-2.2 A/AA and best practice): zero violations |
| tour | once onboarding has finished the board tour, it is not open on any later state (a `?tour=1` replay excepted) |
| heading | an empty state's title is deeper than its section's own heading |

### Profiles and modes

| Profile | Viewport | DPR | Touch | Flows | Sweep |
|---|---|---|---|---|---|
| 360 | 360 × 740 | 3 | yes | | light, dark+RM |
| 390 | 390 × 844 | 3 | yes | dark + reduced motion | light, dark+RM |
| 412 | 412 × 915 | 3 | yes | | light, dark+RM |
| tablet-p | 820 × 1180 | 2 | yes | light | light, dark+RM |
| tablet-l | 1180 × 820 | 2 | yes | dark | light, dark+RM |
| 1280 | 1280 × 800 | 1 | | | light, dark+RM |
| 1440 | 1440 × 900 | 1 | | light | light, dark+RM |
| 1920 | 1920 × 1080 | 1 | | | light, dark+RM |
| 2560 | 2560 × 1440 | 1 | | | light, dark |
| zoom200 | 720 × 450 | 2 | | | light, dark+RM |

"Skip" onboarding runs at 390 (light) and 1440 (dark). RM = reduced motion.

**Reduction, and why.** The spec's full product (3 engines × 10 viewports ×
2 themes × 2 motion settings × 17 flows) is about 2 000 flow runs. The
default plan runs every flow on every engine at the four sizes where the
layout changes shape (phone, tablet portrait with the rail, tablet
landscape, desktop), and the page sweep (all layout, overflow, target,
focus and axe checks on 28 pages) at all ten sizes on all three engines in
both themes. The other sizes differ from these only in width within the
same layout, which the sweep covers page by page; the flows' extra value
there (the same clicks again) did not justify tripling the run time.

**200 % zoom.** None of the engines lets Playwright set browser zoom. A
1440 × 900 screen at 200 % zoom has a 720 × 450 CSS viewport at device
pixel ratio 2, which is what `zoom200` emulates (the WCAG reflow method);
media queries and container queries see exactly what they would at 200 %.

## Engine notes (evidence from the runs)

- **Firefox has no mobile emulation.** Playwright refuses `isMobile` there,
  so Firefox phone and tablet profiles are a desktop Firefox with a touch
  screen (`hasTouch`) at that viewport and DPR; `pointer:coarse` still
  matches, so the touch target rules apply the same way.
- **Firefox reports dropped streams as errors.** A failed `EventSource`
  request logs `Firefox can't establish a connection to the server at
  …/api/v1/stream…` as a JavaScript error, sometimes after the page has
  already reconnected. Expected during the network-drop flow only.
- **Firefox keeps `activeElement` when Tab leaves the page.** Tab from the
  last control moves focus to the browser chrome, but
  `document.activeElement` stays on the control without `:focus-visible`
  (seen on the login page's Sign in button). The focus check notices that
  and starts again from the top of the page.
- **`EventSource` and offline emulation (Chromium).** See "Network drop".
- **Firefox and `content-visibility: auto`.** Off-screen rows of the
  phone containers list are not laid out; Firefox reports their name cells
  at placeholder height (a 44 px link "cut" to 21-34 px). The clipped check
  skips off-screen controls under `content-visibility: auto`.
- **WebKit and lazy charts.** The container metrics charts load when their
  placeholder scrolls into view. At 390 px it starts below the fold and
  moves down again as the page's data arrives; Chromium and Firefox loaded
  the charts anyway, WebKit not within 20 s after a single early scroll
  (runs 2-4). A standalone probe loads all 12 charts in WebKit once the
  placeholder is in view; the flow keeps it in view, as a user scrolling
  would (run 4b: pass).
- **Engine messages set aside.** Each is matched exactly, only in its
  window, and counted per unit (`excluded` in the JSON, by reason in
  `summary.txt`); everything else counts.
  - *WebKit screenshots and CSP.* `page.screenshot()` in WebKit always
    appends `<style>body {}</style>` to sync animations
    (playwright-core 1.64.1, `lib/coreBundle.js`:
    `inPagePrepareForScreenshots(…, syncAnimations)`, with
    `WKPage.shouldToggleStyleSheetToSyncAnimations()` returning true;
    Chromium and Firefox return false), whatever the `caret` and
    `animations` options. The page's `style-src 'self'` refuses it
    ("Refused to apply a stylesheet …", `style-src-elem` `inline`). Set
    aside: that refusal, in WebKit, while a screenshot is being taken
    (and 150 ms after). Screenshots are taken only of failing states.
  - *Loads cancelled by leaving a page.* A navigation cancels the old
    page's loads in flight: WebKit then reports "EventSource cannot load
    … / Fetch API cannot load … due to access control checks." as page
    errors (seen for the event stream, `/api/v1/strips`, `/icon.svg`,
    `/api/settings/general`), and Vite's preload helper (WebKit, Firefox)
    throws "Unable to preload CSS for …", and a lazy chunk fails with
    "error loading dynamically imported module: …" (Firefox) or
    "TypeError: Importing a module script failed." (WebKit); React logs
    them. Set aside: those
    messages while a main-frame navigation is under way (its request
    sent, the new document not yet committed, at most 1.5 s), when only
    the document being left exists. Measured: every one arrived 1-60 ms
    after the navigation request; the failed-request log
    (`requestsFailed` in the JSON) shows the same URLs "Load request
    cancelled" (WebKit) or `NS_BINDING_ABORTED` (Firefox). A standalone
    probe (a plain page with an open `EventSource` and a pending fetch,
    then `goto`) logs nothing in any engine, so these need the app's
    timing (lazy chunks and the stream still connecting), but they never
    reach the page the user lands on.
  - *Chromium and host network changes.* Chromium on Linux aborts its
    requests in flight with `net::ERR_NETWORK_CHANGED` whenever an
    interface of its network namespace changes. The browsers run with
    `--network host`, and the matrix's own throwaway containers (every
    unit's instance and fixtures, the network-drop restart, on three
    engines at once) add and remove veth interfaces on the host. First
    seen in the run of image 59a3fc7 (Chromium 390 overview: two assets;
    the 1920 sweep: four history requests and `/icon.svg`). The runner
    records the host's container start/die and network connect/
    disconnect events (`docker-events.log`); set aside: exactly "Failed
    to load resource: net::ERR_NETWORK_CHANGED", in Chromium, when one of
    those events falls from 3 s before to 1 s after it (the matching
    events are kept in the record).
- **Transient overlays.** Toasts are measured as a scope of their own; the
  board tour, an open dialog, menu or listbox limits the target check and
  the focus check to its own controls; the focused skip link covers the
  brand by design.
- **Firefox and `console.error(object)`.** Firefox prints an Error
  argument as "Error" or "JSHandle@object", and its handle is gone once
  the page has navigated away. The harness writes the arguments out in the
  page as `console.error` is called (an init script and a binding, like
  the CSP listener) and reports that text.
- **The board tour loads lazily.** In run 2 Chromium showed "Board tour ·
  1 of 3" again after a board save. Its onboarding had recorded no tour
  step: the flow had looked for the tour before its chunk rendered, so the
  tour was never finished and was still pending for the account (the tour
  closes while the editor is open and starts again at step 1 after Save).
  Not an app bug: finishing the tour saves it (`POST
  /api/v1/account/board-tour`) and it stays closed after a save, in-app
  navigation and a reload in all three engines. The flow now waits for the
  tour, requires `board_tour_pending: false` from the server afterwards,
  and the `tour` check fails any later state that shows it; with the tour
  left unfinished on purpose, that check reports exactly the run 2 symptom.
- **Table layout.** All three engines leave the width of `display:none`
  columns unallocated in a `table-layout: fixed` table whose flexible
  column is `width: auto`; give that column a percentage (see the
  containers table fix).

## Results

**Final full matrix**, 2026-11-22, image built from 0727d26 (the branch
head for the app; later commits touch only this folder's docs), with the
un-blinded checks of the M6 gate fixes: 78 of 78 units, 330 flow runs
(321 run, 9 n/a), 2 859 page states checked: **every flow passes and
every check is zero**: 0 console errors, 0 page errors, 0 CSP
violations, 0 blocked outside requests, 0 sideways scroll, 0 clipped
controls, 0 target findings, 0 visible-focus failures (264 of the states
with a dialog, menu, listbox or the tour open, checked inside it),
0 reduced-motion failures, 0 axe violations, 0 tour reappearances,
0 heading findings, 0 harness problems. Engine messages set aside
(see "Engine messages set aside"): 28 loads cancelled by leaving a page
(27 WebKit, 1 Firefox, 5 units) and 5 Chromium `ERR_NETWORK_CHANGED`
(1 unit, each 1.6-2.9 s after a container stop and network
disconnect/connect on the host); no screenshot was taken, so none from
WebKit's screenshot style. No toast was on screen at a checked state, so
the toast target scope measured nothing in this run. 75 minutes, 1.9 MB
of output.

| flow | C 390 | C tablet-p | C tablet-l | C 1440 | F 390 | F tablet-p | F tablet-l | F 1440 | W 390 | W tablet-p | W tablet-l | W 1440 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| onboarding: every step | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| tour stays closed | pass (2) | pass | pass | pass (2) | pass (2) | pass | pass | pass (2) | pass (2) | pass | pass | pass (2) |
| login and logout | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| overview | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| containers | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| logs | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| history charts | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| boards | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| catalog to widget | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| settings | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| users and invites | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| command palette | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| keyboard only | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| touch | pass | pass | pass | n/a | pass | pass | pass | n/a | pass | pass | pass | n/a |
| sse reconnect | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| tablet rotation | n/a | pass | pass | n/a | n/a | pass | pass | n/a | n/a | pass | pass | n/a |
| onboarding: skip | pass | - | - | pass | pass | - | - | pass | pass | - | - | pass |

"(2)": the flows unit and the skip unit. "-": not in the plan (skip
onboarding runs at 390 and 1440). n/a: the flow does not apply to the
profile.

| sweep (28 states each) | 360 | 390 | 412 | tablet-p | tablet-l | 1280 | 1440 | 1920 | 2560 | zoom200 |
|---|---|---|---|---|---|---|---|---|---|---|
| chromium light | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| chromium dark | pass RM | pass RM | pass RM | pass RM | pass RM | pass RM | pass RM | pass RM | pass | pass RM |
| firefox light | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| firefox dark | pass RM | pass RM | pass RM | pass RM | pass RM | pass RM | pass RM | pass RM | pass | pass RM |
| webkit light | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| webkit dark | pass RM | pass RM | pass RM | pass RM | pass RM | pass RM | pass RM | pass RM | pass | pass RM |

RM: under reduced motion.

### History

- Baseline (image 544fedf, before any fix): 1 402 axe violations, 7 353
  target and 588 clipped findings in 20 units.
- Run 3 (image a9389bc): zero console, CSP, overflow, focus and axe
  problems across 2 643 states; target and layout fixes followed.
- Run 4 (image 44d2d1c, 78 units, 311 flow runs, 2 708 states): 293 pass,
  18 fail; one axe violation (heading-order on the empty board, Firefox)
  and 212 target findings, traced to four defects, each fixed after it:
  24 px row links in the containers table at 1440 px and 200 % zoom
  ("Truncate long container names inside their link, not the cell"); the
  keyboard shortcuts button unreachable at 200 % zoom ("Let the sidebar
  scroll in short windows"); heading-order on the empty board ("Title
  empty states with h2"); WebKit 390 history charts (harness, see engine
  notes).
- Run 5 (image 7137de6, flows, skip and sweeps at 1440 and 200 % zoom on
  all three engines: 12 units, 63 flow runs, 421 states): all pass.
- Run 6 (image 0c1d0f9): the whole plan, all pass, after the tour check
  was added. That run's checks had blind spots (M6 gate review): errors
  outside a state's own checks were dropped, dialog and menu states
  skipped the focus check, a failed target measurement passed, and a
  unit that wrote no result counted as a pass.
- Run 7 (image 59a3fc7, the review's harness fixes, app unchanged since
  0c1d0f9): 78 units, 330 flow runs, 323 pass, 7 fail, 2 843 states.
  Found: WebKit's keyboard-only walk landing back on `<main>` after the
  skip link (all four flows profiles; fixed in "Skip link: focus main
  directly"), Chromium `ERR_NETWORK_CHANGED` (two units) and a Firefox
  lazy import cancelled by a reload (engine messages, set aside with
  evidence); 43 left-page and 8 WebKit screenshot messages set aside.
  Before it, smaller runs of the work-in-progress app found that the
  account initials named "(AD)" still failed axe 2.5.3 (parentheticals
  are dropped), fixed in the same commit as the initials.
- Re-run (image 7cc876a, WebKit, keyboard-only, login and overview on
  the four flows profiles): 19 of 20 pass; one "Importing a module
  script failed." during a navigation, then added to the left-page
  signatures.
- Final run above (image 0727d26): the whole plan, all pass.
