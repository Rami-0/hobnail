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
problems), and JPEG screenshots of failing states in `shots/`. Exit status
is 0 only when every flow passed with no problem on any state.

A full run (3 engines in parallel, 26 units each: 78 units, 330 flow runs
and about 2 800 checked page states) takes about 70-95 minutes on this host
and writes 1-6 MB (screenshots only of failing states). Each flows unit
takes 5-7 minutes, each sweep 2-3.

## How it works

- **Browsers:** `mcr.microsoft.com/playwright:v1.64.1-noble` (Chromium 156,
  Firefox 157, WebKit 27.2). `playwright-core` 1.64.1 and `axe-core` 4.14.0
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
  is contacted.
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
(wrong password, the rate-limit pause, sign in again) · overview, host,
alerts · containers (filter, open a container, all five detail tabs) · logs
(live tail, search filter, download — opening "Filters" on phones — picker,
multi-pane) · history charts (hover or tap read-out, container metrics) ·
boards (edit, add a link, save; edit, cancel) · catalog → add Pi-hole →
test → choose widgets → save to board → tile on the board · settings (all
12 tabs, save and restore a setting, account) · users and invites (create,
accept in a second browser context, listed) · command palette (open, type,
Enter, Escape) · keyboard only (skip link, 30 Tab stops all visibly
focused, palette to settings, filter, account menu with Escape returning
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
| console | no `console.error` and no uncaught page error (a flow may allow named, expected ones: the 401 of a wrong password, the 429 pause, failed stream requests while the network is down) |
| CSP | no `securitypolicyviolation` event and no CSP console report |
| sideways scroll | `scrollWidth` of the document and body ≤ viewport + 1 px |
| clipped controls | no visible control cut by a non-scrolling ancestor, outside the viewport, or with its own text cut without an ellipsis |
| targets | `../hit-targets.mjs` (the whole target must be the control's own): 44 px where the design promises it (`pointer:coarse` or under 700 px, `shell.css` A11Y-18), 24 px (WCAG 2.5.8) for a fine pointer; with a dialog, menu or listbox open, its own controls |
| focus | two Tab presses: each focused control matches `:focus-visible`, shows an outline or box shadow, and is on screen |
| reduced motion | under `prefers-reduced-motion: reduce`, no running transition or animation |
| axe | `axe.run(document)` with axe-core's default rules (WCAG 2.0-2.2 A/AA and best practice): zero violations |
| tour | once onboarding has finished the board tour, it is not open on any later state (a `?tour=1` replay excepted) |

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
- **WebKit screenshots and CSP.** `page.screenshot()` in WebKit injects a
  style element (to hide the caret), which the page's `style-src 'self'`
  refuses with a console error. The harness takes screenshots only after a
  state's checks, and none of these reached a result (they would show as
  `csp` problems).
- **Transient overlays.** Toasts are hidden while targets are measured; the
  board tour, an open dialog, menu or listbox limits the target check to
  its own controls; the focused skip link covers the brand by design.
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

**Final full matrix**, 2026-11-22, image built from 0c1d0f9 (the branch
head for the app; later commits touch only this folder's docs): 78 units,
330 flow runs (321 run, 9 n/a), 2 835 page states checked: **every flow
passes and every check is zero**: 0 console errors, 0 CSP violations,
0 sideways scroll, 0 clipped controls, 0 target findings, 0 visible-focus
failures, 0 reduced-motion failures, 0 axe violations, 0 tour
reappearances. 70 minutes, 0.9 MB of output.

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
- Final run above (image 0c1d0f9): the whole plan, all pass, after the
  tour check was added.
