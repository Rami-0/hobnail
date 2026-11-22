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

A full run (3 engines in parallel, 26 units each: 78 units, about 300 flow
runs and 2 600 checked page states) takes about 95 minutes on this host and
writes about 6 MB. Each flows unit takes 5-7 minutes, each sweep 2-3.

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
  instance; onboarding skipping every step, then login and logout), and
  `sweep` (owner and a board created through the API, then every route of
  `../sweep-fixture.mjs` (27 pages: overview, containers, the five container
  tabs, host, history, alerts, four logs views, a board, the 12 settings
  tabs, account) plus the login page).

### Flows (`flows.mjs`)

onboarding (every step / skip, including the board tour) · login and logout
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
| 1920 | 1920 × 1080 | 1 | | | light, dark |
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
- **Table layout.** All three engines leave the width of `display:none`
  columns unallocated in a `table-layout: fixed` table whose flexible
  column is `width: auto`; give that column a percentage (see the
  containers table fix).

## Results

Full matrix, 2026-11-22, image built from 44d2d1c (run 4: 78 units, 311
flow runs, 2 708 page states checked): **293 flow runs pass, 18 fail**.
Zero console errors, zero CSP violations, zero sideways scroll, zero
clipped controls, zero visible-focus failures, zero reduced-motion
failures; one axe violation (heading-order on the empty board, Firefox)
and 212 target findings. Every failure was traced to one of the four
defects below, each fixed after that image. Verification on the final
image (7137de6, run 5: flows at 1440, skip at 1440, sweeps at 1440 and
200 % zoom, all three engines): 12 units, 63 flow runs, 421 states,
**all pass, zero problems of any kind**.

- 24 px row links in the containers table at 1440 px and 200 % zoom
  (Chromium, WebKit): fixed by "Truncate long container names inside their
  link, not the cell".
- Keyboard shortcuts button unreachable at 200 % zoom (all engines): fixed
  by "Let the sidebar scroll in short windows".
- heading-order on the empty board (Firefox): fixed by "Title empty states
  with h2".
- WebKit 390 history charts: harness (see engine notes), run 4b passes.

The first full run (run 3, image a9389bc, before the later fixes) had the
same zero counts for console, CSP, overflow, focus and axe across 2 643
states. The first baseline (image 544fedf, before any fix) had 1 402 axe
violations, 7 353 target and 588 clipped findings in 20 units.

| flow | C 390 | C tablet-p | C tablet-l | C 1440 | F 390 | F tablet-p | F tablet-l | F 1440 | W 390 | W tablet-p | W tablet-l | W 1440 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| onboarding: every step | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| login and logout | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| overview | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| containers | pass | pass | pass | FAIL | pass | pass | pass | pass | pass | pass | pass | FAIL |
| logs | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| history charts | pass | pass | pass | pass | pass | pass | pass | pass | FAIL | pass | pass | pass |
| boards | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| catalog to widget | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| settings | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| users and invites | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| command palette | pass | pass | pass | FAIL | pass | pass | pass | pass | pass | pass | pass | FAIL |
| keyboard only | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| touch | pass | pass | pass | n/a | pass | pass | pass | n/a | pass | pass | pass | n/a |
| sse reconnect | pass | pass | pass | FAIL | pass | pass | pass | pass | pass | pass | pass | FAIL |
| tablet rotation | n/a | pass | pass | n/a | n/a | pass | pass | n/a | n/a | pass | pass | n/a |
| onboarding: skip | pass | - | - | pass | pass | - | - | FAIL | pass | - | - | pass |

| sweep | 360 | 390 | 412 | tablet-p | tablet-l | 1280 | 1440 | 1920 | 2560 | zoom200 |
|---|---|---|---|---|---|---|---|---|---|---|
| chromium light | pass | pass | pass | pass | pass | pass | FAIL (5) | pass | pass | FAIL (34) |
| chromium dark | pass | pass | pass | pass | pass | pass | FAIL (2) | pass | pass | FAIL (34) |
| firefox light | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL (24) |
| firefox dark | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL (24) |
| webkit light | pass | pass | pass | pass | pass | pass | FAIL (2) | pass | pass | FAIL (30) |
| webkit dark | pass | pass | pass | pass | pass | pass | FAIL (1) | pass | pass | FAIL (30) |

"-": not in the plan (skip onboarding runs at 390 and 1440) or not reached
(none in run 4). n/a: the flow does not apply to the profile.
