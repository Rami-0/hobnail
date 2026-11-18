# React port, phases 0 and 1: results

Date: 2026-11-18. Spec: [react-port.md](react-port.md) (sections 4.9 and 5).
Owner decisions D1 to D11 answered "(a)" for all. Raw outputs:
[react-port-raw/](react-port-raw/). Nothing here was deployed.

Marks as in the plan: **[code]** verified in code, **[exp]** measured or
observed here, **[assume]** not verified. MiB = 2^20 B, MB = 10^6 B, KB =
10^3 B. Asset bytes and runtime RAM are never mixed.

## Builds compared

| Name | What | Binary | Image (`docker images` / content) |
|---|---|---|---|
| main | `680a339`, the old UI only | 6,633,904 B | 11.6 MB / 3,910,610 B |
| phase 0 | `6d7d101`, empty `/app` shell embedded | 6,994,352 B (+360,448) | 12.1 MB / 4,087,157 B |
| phase 1 | this branch's tip before the report commit (`p1rc`) | 7,780,784 B (+1,146,880) | 13.3 MB / 4,448,685 B |

All [exp]. Budget (4.9, D10): image ≤ 15 MB while both UIs ship. Met.

## Phase 0 acceptance

| Item | Result | Evidence |
|---|---|---|
| Image builds with all gates (web: type check, lint, guard, unit tests, build, bundle budgets; Rust: fmt, clippy, tests, release, version check) | pass | builds `6d7d101`, `p1rc` [exp]; 508 Rust tests, 100 client tests |
| `/app` shows an empty shell that signs in and out through the real login | pass | `web/e2e/app-check.mjs` at 1440 and 390 px, dark and light, against the real server: `react-port-raw/app-check-rc.log` |
| The two stale-binary cases of plan 4.4 now rebuild or fail | pass | table below [exp] |
| Gate tests pass with the new routes in `WRITES` | pass | `src/http/gate_tests.rs` [code, exp] |
| CSP check: zero violations | pass | 0 in every pass of app-check and slice-check [exp] |
| Existing pages and browser checks unchanged | pass, with limits | same results on main and phase 0 (below); not re-run on the phase 1 build, whose only change to old-UI code is three `pub(crate)` visibilities in `src/ui/pages/container.rs` [code] |
| ts-rs (dev-dependency) leaves the release binary unchanged (D11) | pass | two release builds of the same tree with and without the dev-dependency: byte-identical, sha256 `ab7a4e92…`, 7,780,784 B [exp] |

**Stale binary** (plan 4.4), real Rust stage, the shared target cache,
throwaway copies of the tree [exp]:

| Run | Change | Cargo | Result |
|---|---|---|---|
| 1 | baseline | Compiling hobnail | content hash `773bc968…` |
| 2 | one source string changed, every file dated 2001 | Compiling hobnail | new hash, binary differs, string present |
| 3 | an embedded CSS file rewritten, old mtime restored | Compiling hobnail | new hash, binary differs, served raw and gzip |
| control A | hash fixed to a constant, version check removed | Compiling | reference binary |
| control B | as A, one string changed, file dated 2001 | Fresh (0.11 s) | **stale**: byte-identical to A; the bug reproduces without the fix |

**Existing browser checks**, run through the new containerised runner
(`scripts/e2e.sh`) against throwaway instances seeded with the test
instance's config [exp]:

| Check | main | phase 0 |
|---|---|---|
| a11y-check | pass | pass |
| ui-check | 2 failures | the same 2 |
| nav-check | 3 failures | the same 3 |
| motion-check | pass | pass |
| smooth-check | pass | pass |
| logs-check | 12 failures | the same 12 |
| settings-check | 6 failures | 5 failures |

The failures are environmental in this setup (the mock receiver's host
name does not resolve from a throwaway instance; different steps time out
between runs) and identical in kind on both builds. `phone-scroll-test.mjs`
and `shots.sh` cannot run in the container unchanged (host Playwright path,
bash) and were not run.

## Phase 1 acceptance

Instance: the phase 1 image behind `web/e2e/fake-docker.mjs` (150
containers in 13 stacks, live cgroup files). `web/e2e/slice-check.mjs`:
158 steps, all passing, at 1440 × 900 and 390 × 844, dark and light
(`react-port-raw/slice-check-rc.log`).

| Item | Result | Evidence |
|---|---|---|
| Edit save | pass | toast with the revision, dialog closes, head shows the new name |
| 409 "Reload and keep my input" | pass | config changed behind the open dialog → note → reload-keep → save keeps the typed value and the other change |
| 401 recovery | pass | cookie deleted → "Sign in (new tab)" + Retry → new session → Retry saves |
| Foreign-origin refusal | pass | valid cookie + token, `Origin: http://evil.example` → 403 `cross-origin write refused`, config hash unchanged |
| CSP zero violations | pass | 0 violations, console errors, exceptions and failed requests in all four passes |
| 150 rows, phone 4×: no task over 200 ms on load | **not cleanly met** | trace max 150.7 and 168.5 ms; long-task observer max 169 and **220** ms (run 2). The two methods disagree; the observer's figure counts more of the first render and layout as one task [assume] |
| 150 rows: live update ≤ 50 ms | pass | phone 23 to 32 ms, desktop 23 to 41 ms |
| Shell, Keyboard, Containers, Container detail (Overview), Charts checklist | mostly | see "Parity" |
| Go/no-go D2 (UI SDK) and D4 (Recharts) | see below | |

**Parity, partly or not done.** Command palette (and its keys); relative
times are refreshed only where pages use `useNow()`; installable manifest is
the existing link only; event markers: a cluster opens the old History page
at that event, a single marker scrolling to its timeline row was not
checked; desktop CLS (below).

## Measured against the budgets (plan 4.9)

**Bundle** [exp] (`npm run size`, the phase 1 build):

| | Raw | Gzip | Budget |
|---|---|---|---|
| First route (index.html, entry, React runtime, CSS) | | 105,473 B | ≤ 180 KB gzip |
| Largest lazy page chunk (container page) | 29,466 B | 10,019 B | ≤ 60 KB gzip |
| Recharts chunk (+ its CSS 1,106 B) | 384,304 B | 109,608 B | ≤ 110 KB gzip (0.4 KB left) |
| Whole `dist` | 807,686 B | 241,398 B | ≤ 1.3 MB / 400 KB |

**Browser** [exp] (`web/e2e/perf-check.mjs`, cold cache, loopback; phone =
390 × 844 with 4× CPU throttling, desktop = 1440 × 900; two runs,
`react-port-raw/perf-rc1.md`, `perf-rc2.md`; "old" = today's page on the
same instance):

| Page | Profile | LCP ms | Longest load task (trace / observer) ms | Live update max ms | INP ms | CLS |
|---|---|---|---|---|---|---|
| `/app/containers` | phone | 920, 1204 | 151, 169 / 169, 220 | 32, 27 | 96, 96 | 0.016 |
| `/app/containers` | desktop | 340, 348 | 58, 46 / 62, 0 | 41, 27 | 152, 200 | **0.134** |
| `/app/containers/<n>` | phone | 1332, 1276 | **369, 368** | 11, 17 | 152, 144 | 0.0002 |
| `/app/containers/<n>` | desktop | 400, 480 | 123, 95 | 12, 9 | 16, 48 | 0.003 |
| old `/containers` | phone | 2440, 2420 | 1838, 1791 | 159, 204 | 936, 1176 | 0 |
| old `/containers` | desktop | 904, 992 | 535, 612 | 49, 58 | 312, 352 | 0 |
| old `/containers/<n>` | phone | 896, 964 | 347, 402 | 177, 174 | 40, 56 | 0.03 |

Budget misses: phone load on `/app/containers` by the observer in one run
(220 > 200); desktop CLS 0.134 > 0.1 (placeholder rows that hold the height
of rows still to render approximate the real height); LCP on the chart
route at phone (1276 to 1332 > 1200 ms) and desktop (400 to 480, marginal
over 400); the chart route's first Recharts mount is one ~370 ms task at
phone 4× (no budget for that route in 4.9, but it would fail the 200 ms
rule applied to it). INP ≤ 200 ms everywhere, 200 exactly once.

**Memory** [exp] (`scripts/bench-mem.sh`, plan 4.9 method: one container,
no limit, warm cache, the test instance's config copied, fresh database,
the host's Docker socket read-only, 500 ms samples; values MiB, max of
`memory.current`; raw CSV in `react-port-raw/bench-*`):

| Phase | main, today's endpoints | phase 1, today's endpoints | phase 1, JSON API + JSON stream + embedded assets |
|---|---|---|---|
| idle (median / max) | 4.22 / 5.00 | 2.00 / 4.76 | 2.03 / 3.04 |
| 1 client | 6.01 | 3.34 | 3.41 |
| 10 clients | 6.55 | 4.10 | 3.90 |
| 25 clients | 7.51 | 5.12 | 4.55 |
| `memory.peak`, whole run | 8.84 | 6.16 | 6.29 |
| RssAnon at 25 clients | 1.28 | 1.29 | 1.25 |
| errors | 0 | 0 | 0 |

Budgets: ≤ 10 MiB at 1/10/25 clients, peak ≤ 20 MiB: met. **The lower
figures of the phase 1 runs are not a saving**: the difference is in
`file` (page cache charged to whichever cgroup first read the shared
pages: 2.8 vs 0.6 MiB at idle), while `anon` and RssAnon are equal across
builds. Cold cache was not measured.

**Viewer CPU** [exp]: one load client (stream + state every 2 s + history
every 5 s + assets every 20 s): main 1.14 %, phase 1 JSON 0.82 % of one
core. Not the same workload as the plan's 0.58 % (one browser on today's
page) [src 7 of the plan]; a browser-viewer measurement was not repeated.

**Start-up** [exp]: binary exec → first `/healthz` 200 with a fresh data
folder each run (includes creating the database), median of 9: main
26.6 ms, phase 1 22.5 ms. Container start → first 200: 419 to 440 ms.

**Build time** [exp], warm caches: web stage `npm ci` 49.6 s, check + test
+ build + size 4 s; Rust stage 196 to 221 s (release link with fat LTO
about 165 s).

## Go/no-go

**D2, UI SDK: not testable.** No version is installable under the 7-day
rule: first publish 2026-11-12, `latest` `0.1.0-beta.6` published
2026-11-16 18:47 UTC, `beta.7` 2026-11-17. The earliest any version passes
is 2026-11-19 (beta.0); beta.6 passes on 2026-11-23. Every primitive the
slice needs (dialog, menu, tabs, select, switch, toast) is in
`web/src/ui/` on platform elements (`<dialog>`, native `<select>`), and
the slice-check passes on them. Recommendation: decide between pinning
beta.6 after 2026-11-23 and swapping it in behind the wrappers in phase 2
(the decision's instruction), or keeping the platform wrappers until a
stable release; the swap stays inside one folder either way.

**D4, Recharts: go, with two risks.** On the container Overview's CPU and
memory charts [exp]: linked crosshair and tooltip across both (slice-check);
drag to zoom into `?from=&to=` with mouse and touch (slice-check); gaps
(the line splits into two paths across a 2.5-minute outage); max band
(drawn from 24 h windows); event markers (9 markers in 1 h, clustered to 1
in 24 h, clickable); local-time axes; legend toggles remembered. Native in
Recharts: lines, axes, tooltip, `syncId`, areas for the band. Custom code:
gap detection, drag-zoom, marker layer and clustering, tick formatting.
Risks: the chunk is at its 110 KB budget, and the first chart mount is a
single ~370 ms task at phone 4× that a transition cannot split.

## Deviations from the plan

1. UI SDK not installed (D3); its project-local skill was not installed
   either. Wrappers on platform elements instead.
2. Files outside the phase lists: `src/main.rs` (`mod api;`, `version`
   prints the content hash) and `src/http/mod.rs` (`pub mod assets;`), both
   needed for the planned code to compile; three `pub(crate)` visibilities
   in `src/ui/pages/container.rs`; one accessor (`link_template`) in
   `src/ui/view/mod.rs`. The view structs did not need `Serialize`: the API
   has its own row structs.
3. `/api/v1/containers/<name>` already serves `from`/`to` windows (plan:
   phase 2) to prove drag-to-zoom (D4). Bad query values are a 400 where the
   old page falls back; an owner-hidden container is a 404 where the old
   page still answers a direct link (plan 4.2 "appear nowhere").
4. The Edit dialog lives on `/app/containers/<name>?edit=override`; the old
   UI opens it on the Settings page, which is not ported yet.
5. Tooling: oxlint instead of typescript-eslint (TypeScript 7); Node's test
   runner with type stripping instead of a test framework; no DOM component
   tests (the browser checks cover the DOM); a hand-written router.
6. `started_at` in the list is derived from Docker's status text (the list
   API has no start time); the detail uses the exact inspect time.
7. The section 3 wording marked "phase 0" for GOALS.md, DESIGN.md and
   `docs/features/ui.md` was not applied (outside this run's files); only
   `docs/install.md` line 33 was.
8. `npm audit`: 5 high findings, all one unpatched ReDoS in `picomatch`
   ≤ 4.0.7 used by Vite at build time only; nothing ships to the browser.

## Not verified

Firefox and Safari; aarch64; cold-cache memory; the Vite dev proxy against
a real Hobnail with a signed-in write; whether `npm ci` re-checks ages of
locked versions (the lock was audited instead: 128 packages, youngest 7.3
days); screen readers; the max band and markers on a long history (the
instances lived minutes); old browser checks on the phase 1 build.
