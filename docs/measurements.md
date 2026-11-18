# Measurements

Targets and baselines come from [GOALS.md](../GOALS.md) section 1. Re-run with
`scripts/measure.sh` (it restarts only the `hobnail` container). Host:
Test host: 4 cores, 7.2 GiB RAM, Docker 29.5.2, cgroup v2.

**How each number is taken**

- *Idle RAM*: `docker stats --no-stream` (cgroup memory as Docker reports it),
  20 s after start, nobody connected.
- *RAM with 1 viewer*: same, while one `/api/stream` SSE client has been
  connected for 30 s (the refresh loop is running: system stats every 3 s,
  container stats every 5 s, widgets at their own intervals).
- *Image size*: `docker images` (unpacked size in the containerd store) and
  `docker image inspect .Size` (compressed content).
- *Cold start* is reported two ways, because they differ by 10×:
  1. **App**: release binary, `exec` → first `200` from `/healthz` on
     loopback, polled with `curl` in a loop (each poll costs ~16 ms on this
     host, so this is an upper bound).
  2. **Container**: `docker inspect .State.StartedAt` → first `200` on the
     published LAN address after `docker stop && docker start`. This includes
     Docker creating the network namespace, veth pair and port proxies, which
     the app cannot influence.
- *Page load*: `curl` `time_total` for `/` from this host to the LAN address,
  median of 10. Plus a real-browser first paint where noted.

## Baselines (from GOALS.md, measured 2026-11-14)

| Metric | Homepage | Others on this host |
|---|---|---|
| Idle RAM | 112 MiB | Dozzle 41 MiB, Beszel hub 37 MiB, Beszel agent 14 MiB, Arcane 59 MiB |
| Image size | 376 MB | Dozzle 101 MB |

## M0 — skeleton (build `11bce8a`, 2026-11-14 13:41Z)

Static page, config + hot reload, SSE, no Docker/system/widgets yet. Three
agents were compiling on the host while this was measured.

| Metric | Measured | Target | Stretch | Result |
|---|---|---|---|---|
| Idle RAM | **0.55 MiB** (CPU 0.01%) | ≤ 15 MiB | ≤ 8 MiB | ✅ |
| RAM, 1 viewer | **0.57 MiB** (CPU 0.07%) | ≤ 25 MiB | ≤ 15 MiB | ✅ |
| Image size | **5.16 MB** unpacked / 1.85 MB compressed (binary 2.25 MB + busybox 1 MB) | ≤ 20 MB | ≤ 10 MB | ✅ |
| Idle CPU | 0.01% | ~0% | ~0% | ✅ |
| Cold start, app (exec → first 200) | **15–27 ms** (7 runs, upper bound) | < 200 ms | < 50 ms | ✅ |
| Cold start, container (StartedAt → first 200 on LAN IP) | **206–355 ms**, median 287 ms | < 200 ms | < 50 ms | ❌ missed by ~87 ms (Docker networking setup, see above) |
| Page load `/` (curl, LAN) | **0.4 ms**, 1.7 KB | < 150 ms | < 80 ms | ✅ |

## M1 — Docker cards (build `4ff11d0`)

| Metric | Measured | Target | Result |
|---|---|---|---|
| Idle RAM | **0.96 MiB** | ≤ 15 MiB | ✅ |
| RAM, 1 viewer | **1.0 MiB** | ≤ 25 MiB | ✅ |
| Image size | **5.52 MB** | ≤ 20 MB | ✅ |

## M4 — all of M1–M4 running (build `1c6c8f5`, 2026-11-14 14:12Z)

Config active: 5 widgets (Beszel, Mailpit, Odoo, a command widget, one broken
on purpose), 3 notification channels, 1 container alert rule. Docker events
stream connected. This is the "cost of always-on alert checks" measurement:
the idle number includes the alert task and the Docker events stream.

| Metric | Measured | Target | Stretch | Result |
|---|---|---|---|---|
| Idle RAM (alerts active, no viewers) | **1.28 MiB** (CPU 0.01%) | ≤ 15 MiB | ≤ 8 MiB | ✅ |
| RAM, 1 viewer (36 cards, system tiles every 3 s, 5 widgets) | **1.04 MiB** (CPU 0.08%) | ≤ 25 MiB | ≤ 15 MiB | ✅ |
| Image size | **6.31 MB** unpacked / 2.21 MB compressed | ≤ 20 MB | ≤ 10 MB | ✅ |
| Idle CPU | 0.01% | ~0% | ~0% | ✅ |
| Cold start, app (exec → first 200) | ~20 ms (see M0 method) | < 200 ms | < 50 ms | ✅ |
| Cold start, container (StartedAt → first 200, published LAN IP) | **268 ms** median (250–340) | < 200 ms | < 50 ms | ❌ **missed by ~68 ms** |
| Page load `/` (curl, LAN) | **2.3 ms** total, TTFB 2.1 ms, 50 KB HTML | < 150 ms | < 80 ms | ✅ |

The two RAM numbers are within noise of each other: `docker stats` counts the
cgroup's memory, which also moves with kernel page cache for the container.

**Why the container cold start misses and what would not fix it.** The binary
answers ~20 ms after `exec`. The rest is Docker creating the container's
runtime state between recording `StartedAt` and the process running. A
throwaway test container (not the deployed one) measured:
bridge + 1 published port 175–236 ms; `network_mode: host` 128–302 ms. So
host networking does not reliably help, and it would contradict the "publish
only on two IPs" setup. I have not relaxed the target; it stays missed.

Comparison with the baselines: Homepage idles at 112 MiB with a 376 MB image;
hobnail idles at 1.28 MiB (~87× less) with a 6.3 MB image (~60× smaller).

## Final — M0–M5 (build `221dffe`, 2026-11-14 14:37Z)

Final config: 5 widgets, 2 page-only alert rules, dashboard-icons cache on.
Host load average ~3 on 4 cores during all runs (other workloads; ~7 % idle).

| Metric | Baseline (Homepage) | Measured | Target | Stretch | Result |
|---|---|---|---|---|---|
| Idle RAM | 112 MiB | **1.14 MiB** | ≤ 15 MiB | ≤ 8 MiB | ✅ stretch |
| RAM, 1 viewer | — | **1.15 MiB** | ≤ 25 MiB | ≤ 15 MiB | ✅ stretch |
| Image size | 376 MB | **6.31 MB** unpacked / 2.22 MB compressed | ≤ 20 MB | ≤ 10 MB | ✅ stretch |
| Idle CPU | — | 0.01 % | ~0 % | ~0 % | ✅ |
| Cold start, app (exec → first 200) | — | ~20 ms | < 200 ms | < 50 ms | ✅ stretch |
| Cold start, container (StartedAt → first 200) | — | **267 ms** median (219–453) | < 200 ms | < 50 ms | ❌ **missed by ~67 ms** (Docker's container start; see M4 notes) |
| Page served (curl, LAN, `/`, 55 KB incl. inlined CSS) | — | **1.2 ms** | — | — | — |
| First contentful paint, real browser | — | **174–183 ms** median (best 135 ms) | < 150 ms | < 80 ms | ❌ **missed by ~25–35 ms in this measurement** |

**How first paint was measured.** Headless Chromium (zenika/alpine-chrome,
software rendering, no GPU) in a throwaway container **on this same busy
server**, fresh tab per load, `first-contentful-paint` from the Performance
API, 9 loads per address, first load dropped. The HTML is complete at 6 ms;
a trivial one-line page paints at a median of 49 ms in the same browser, so
~130 ms is Chromium styling and laying out the 37-card page on a CPU that is
~93 % busy with other work. Fixes applied while measuring: inline CSS,
`custom.css` only when present, `defer` on app.js, `content-visibility` on
groups (250 ms → ~175 ms). Not measured from your phone/laptop (no access);
those will likely paint faster than a software renderer on a loaded server,
but I have not verified that, so the target stays marked as missed.

**A real bug found by the browser test:** with SSE open, the 7th navigation
stalled ~20 s (Chrome's 6 connections per host held by pages in the
back/forward cache). Fixed by closing the stream on `pagehide`/hidden tabs;
12 consecutive navigations now take 153–412 ms.

## v2 — new UI, history database, settings (build `494ad23`, 2026-11-14 17:17Z)

Release build, live stack, real config (5 widgets, 2 rules, icons on), SQLite
history with the 15 s sampler running. Host load average ~2.8 on 4 cores (other
workloads) during all runs.

| Metric | v1 (M5) | v2 measured | Target / budget | Result |
|---|---|---|---|---|
| Idle RAM (DB + sampler, no viewer) | 1.14 MiB | **1.14 MiB** | ≤ 15 MiB; v2 budget < 8 MiB | ✅ |
| RAM, 1 viewer (30 s) | 1.15 MiB | **1.65 MiB** | ≤ 25 MiB | ✅ |
| Image | 6.31 MB | **9.61 MB** unpacked / 3.32 MB compressed (SQLite ~3 MB) | ≤ 20 MB | ✅ (stretch ≤ 10 MB ✅ just) |
| App CPU with no viewer (sampler + events + alerts, 120 s) | 0.01 % | **0.13 %** of one core, ~20 ms per 15 s sample | ~0 % | ✅ (the cost of history) |
| History DB | — | **2.39 MiB after 1.27 h**; ~400 KiB/h for the first 6 h, then ~80 KiB/h (1 min rollups) and ~8 KiB/h (10 min); projected steady ~30–35 MiB with 38 containers | < 50 MB | ✅ projected (not yet observed for 24 h) |
| Cold start, container (StartedAt → first 200) | 267 ms | **281 ms** (228–301) | < 200 ms | ❌ **missed by ~81 ms** (Docker's container start; app ready in ~30 ms incl. opening the DB) |
| Page served `/` (curl, LAN) | 1.2 ms / 55 KB | **9.5 ms / 120 KB** | — | — |
| Other pages (curl) | — | /containers 7.5 ms, /history 6.9 ms, /alerts 2.6 ms | — | — |
| First contentful paint, Overview (headless Chromium on this server, 7 loads) | ~175 ms | **148 ms** median (112–208) | < 150 ms | ✅ barely |
| First contentful paint, Containers | — | **164 ms** median (120–228) | < 150 ms | ❌ **missed by ~14 ms** |
| CSS + JS (all pages) | ~18 KB | **72.6 KB** raw (~19 KB gzip): CSS 42.7 KB inlined, JS 29.8 KB | DESIGN: JS ~15 KB | ❌ JS ~2× the DESIGN estimate |
| Settings-only assets | — | settings.js 68 KB, settings.css 20 KB (loaded only on /settings) | — | — |

Notes:
- First paint: a trivial one-line page paints at a median of 28 ms in the same
  browser, the HTML arrives in 7–13 ms, so the rest is styling/layout of the
  dense pages on a busy, GPU-less server. Not measured from a phone or laptop.
- Page weight grew because the CSS (~43 KB) is inlined into every page (one
  render-blocking request fewer) and the palette index is embedded. Responses
  are not compressed; on the LAN/tailnet this costs ~1 ms.
- The DB-size projection comes from the measured first-hours growth and the
  rollup design; it should be re-checked after 24 h and 7 days
  (`scripts/measure.sh` prints it).

## v3 W1 item 1 — logs in the app (branch `worktree-agent-a56be35c711359283`, 2026-11-14 ~22:45Z)

Release build in a throwaway `rh-w1logs` container (127.0.0.1:3296, copy of the live
config), RSS = `VmRSS` of the process from the host `/proc`. Host load average 9 to 12
on 4 cores (three other agents compiling), so CPU and browser timings are pessimistic.
Test containers: `w1`..`w5` (~45 lines/s each: JSON, logfmt, ANSI, stderr), `50mb`
(~15 KB/s, 55 MB/h), `busy` (~1,100 lines/s, ~550 MB/h), a TTY one and an exited one.

| Metric | Measured | Budget | Result |
|---|---|---|---|
| Idle RAM, base `a57850d`, same config | 4.6 MiB | — | baseline |
| Idle RAM, this branch, no log rule / no `[logs] count` | **5.0 MiB** (task opens nothing) | ≤ 8 MiB | ✅ |
| Idle RAM, `log_match` rule + count on 5 chatty containers (~280 lines/s total) | **5.3 MiB** (+0.3 MiB) | ≤ 8 MiB | ✅ |
| CPU of that background watching (120 s) | **~3.0 %** of one core (≈ 0.1 ms per line: framing, ANSI strip, level, regex) | — | documented |
| RAM, one viewer on the 55 MB/h log (60 s) + the 5 tails | **6.0 MiB** | ≤ 20 MiB | ✅ |
| RAM, one viewer on the 550 MB/h log (60 s) + the 5 tails | **6.05 MiB**, peak 6.1 MiB; browser 72 ms behind | ≤ 20 MiB | ✅ |
| Viewer leaves → Docker log connection closed | **53 ms** (integration test with a fake daemon); live: `viewer_streams` back to 0 | ≤ 5 s | ✅ |
| Initial tail, 5,000 lines over SSE | **84 ms** (`docker logs --tail 5000`: 148 ms) | — | — |
| Download, 5,000 lines | 180 ms | — | — |
| Search over 5,000 lines in headless Chromium (`performance.now`, filter pass) | substring 5–16 ms median, regex 4–10 ms, only-matching 7–16 ms, ±3 context 6–11 ms; worst single run 52 ms | < 100 ms | ✅ |
| Same incl. forced style/layout of the 5,000-line DOM | 7–80 ms median; worst 135 ms on the loaded host | — | lines use `content-visibility:auto` |
| Binary / image | **+241,920 B** (5,243,824 → 5,485,744; JS/CSS embedded); no new crate (own regex subset instead of `regex`, ~1 MB) | image ≤ 12 MB | ✅ |
| logs.js / logs.css (logs pages only) | 27.5 KB / 9.7 KB raw, **8.6 KB / 2.9 KB gzip** | page JS ≤ 40 KB gzip (app.js 11.2 KB + logs.js 8.6 KB) | ✅ |
| DB | `log_counts`: one row per counted container per minute with lines (+1 per 10 min), kept 7 d / 90 d; ≤ ~11k + ~13k rows per always-busy container | 150 MB at 90 d | ✅ projected |

## v3 Wave 1 — all four items merged (build `1491888`, 2026-11-14 23:15Z)

Release build. Idle/viewer numbers from a throwaway copy (`rh-w1review`, same
config and a copy of the history DB, 36 containers); first paint and the phone
test on the live stack. Host load average ~4 on 4 cores.

| Metric | v2 | Wave 1 | Budget (V3-PLAN) | Result |
|---|---|---|---|---|
| Idle RAM, no viewer | 3.0–3.3 MiB | **3.5 MiB** | ≤ 8 MiB | ✅ |
| RAM, one Overview viewer | — | **3.6 MiB** | — | ✅ |
| RAM, one viewer on a busy container's logs | — | **3.7 MiB** (6.05 MiB with 5 background tails + a 550 MB/h log, see item 1) | ≤ 20 MiB | ✅ |
| Image | 9.61 MB | **10.7 MB** | ≤ 12 MB | ✅ |
| Background CPU, no viewer (sampler + container extras + host series + events + alerts) | 0.13 % | **0.18 %** of one core (220 ms / 120 s) | — | documented |
| Container-extras sampler | — | 12–30 ms per tick for ~40 containers (mostly first-time `inspect`s while test containers came and went); quiet-host value logged hourly as "container extras sampled" | "keep it small" | ⚠️ re-check on a quiet host |
| Overview first contentful paint, over the Tailscale IP | 148 ms | **152 ms** median (136–316) | < 300 ms | ✅ |
| Server render (curl, preview) | / 9.5 ms | / 40 ms, /containers 15 ms, /host 82 ms, /history 30 ms, detail 15 ms, logs 4 ms | — | /host is the slowest (process scan) |
| Page size (uncompressed) | / 120 KB | / 165 KB, /containers 237 KB, /host 204 KB, /history 288 KB | — | ⚠️ grew ~35 %; gzip of HTML would recover most |
| Page JS, heaviest page (gzip) | — | **34.1 KB** (app.js 14.7 + settings.js 19.4); logs pages 23.3 KB | ≤ 40 KB | ✅ |
| History DB | 2.4 MiB @ 1.3 h | projected: core ~30 MiB + host series ~25 MiB + container extras + log counts ≈ **60–70 MiB at 90 days** | ≤ 150 MB | ✅ projected, not yet observed |
| Phone scroll test | 0 px | **0 px**, 0 swaps while touching | 0 | ✅ |
| Contrast (automated test over app.css) | AA | Dark 5.15, Light 4.81, Ember 5.46, High contrast 11.1 (lowest body text) | AA; HC ≥ 7 | ✅ |

## ux-pass bench phase (2026-11-17)

Builds compared: **baseline** `02c21fb` (before the UI pass), **before** `49a8421`
(after UI waves 1 to 5, start of this phase) and **after** `47f31b0` (the last
optimisation commit). Host: a 2-core / 4-thread 7th-generation mobile Intel CPU,
7.2 GiB RAM, kernel 7.0, Docker 29.5.2, cgroup v2, about 40 other containers,
load average 1.5 to 3.9 during the runs. Every instance was a throwaway
container on 127.0.0.1 with the same seed config, the host's Docker socket
(read-only, 21 to 23 running containers), and a copy of the same database;
A and B ran side by side (idle) or alternated (timings). The phase was stopped
early at the owner's request: see "Not done".

**Databases.** *Busy*: an online backup of the shared test instance's database
(11 MB, 8,558 events, 200 config revisions, ~1 day of history). *Steady*:
`gen_db.py` on top of it: 6 h raw, 7 d of 1-min and 90 d of 10-min rollups for
the host, 40 containers and 33 host keys, 100,000 events over 180 d (131.3 MiB).

**Method** (scripts kept outside the repo; each is a few lines of shell):
- *Idle*: 20 s after start, nobody connected: `docker stats --no-stream`,
  `VmRSS`/`RssAnon` from `/proc/<pid>/status`, CPU = utime+stime from
  `/proc/<pid>/stat` over 120 s (3 rounds) or 60 s (2 rounds). With a binary
  bind-mounted over `/hobnail`, `RssFile` and docker stats include different
  file pages, so **RssAnon** is the number to compare between builds.
- *Server time*: `curl -w %{time_starttransfer}` on loopback, signed in, 15
  requests per page, median / p90, two rounds alternating the builds.
- *Load*: `hey` 0.1.4 (host network), signed in, keep-alive, 2 s warm-up then
  8 s at `-c 1`, `-c 8`, `-c 32` per route (the baseline report's `load.sh`).
- *Syscalls*: `strace -f -y -p 1` from a container in the app's PID namespace,
  46 s, grouped per 15 s tick.
- *Cold start, binary*: exec of the release binary on the host (empty data
  dir, no Docker) to the first `200` from `/healthz`, raw-socket polling every
  ~1 ms, 7 runs. *Container*: `docker stop`, then `docker start` to the first
  `200`, 5 runs.

### Results

| Metric | Baseline `02c21fb` | Before `49a8421` | After `47f31b0` |
|---|---|---|---|
| Idle RAM, busy DB: docker stats | 5.81-5.95 MiB | 5.76-5.81 MiB | **3.77-3.80 MiB** |
| Idle RssAnon / VmRSS, busy DB | 3.31-3.37 / 7.76-7.80 MiB | 3.32-3.38 / 7.73-7.76 MiB | **1.16-1.20** MiB / 5.66-5.69 MiB* |
| Idle RAM, fresh DB: docker stats (RssAnon) | 2.2-3.0 MiB (1.14-1.20) | 2.4-3.0 MiB (1.12-1.16) | not re-measured |
| Idle CPU, busy DB | 0.10-0.14 % (120-170 ms / 120 s) | 0.10-0.14 % | **0.07-0.10 %** (80-120 ms / 120 s) |
| Idle CPU, fresh DB | 0.09-0.11 % | 0.09-0.11 % | not re-measured |
| Syscalls per 15 s tick (cgroup files) | — | 3,335-3,462 (1,812) | **2,837-2,853 (1,334-1,337)** |
| Image (unpacked / compressed) | 10.8 MB / 3.67 MB | 11.6 MB / 3.91 MB | see the dev image rebuilt from the final commit (binary +12 KB) |
| Release binary | 6,039,984 B | 6,621,616 B | 6,633,904 B |
| Cold start, binary (exec → `/healthz` 200) | 4.4 ms median (3.2-155) | — | **3.5 ms** median (2.9-10.4) |
| Cold start, container (`docker start` → 200) | — | — | 460 ms median (428-526); `StartedAt` → 200: 365 ms |
| `/history`, steady DB | 752-768 ms | 782-830 ms | **39 ms** (after the correction below) |
| `/history?range=7d`, steady DB | 845-880 ms | 875-923 ms | **132-137 ms** |
| `/containers/<name>`, steady DB | 109-111 ms | 109-128 ms | **9-13 ms** |
| `/containers/<name>?tab=metrics`, steady DB | — | 231-272 ms | **22-33 ms** |
| `/host`, steady DB | 27-32 ms | 37-44 ms | 22-30 ms |
| `/history`, busy DB (shared instance copy) | 47 ms | 71 ms | 63-71 ms (216-222 ms before the correction below) |
| `/containers` while `/history` renders in a loop | p90 732 ms, p99 975 ms, 8.8 rps (baseline report) | — | **p90 20 ms, p99 28-30 ms, 73-74 rps** |
| `/api/state` p99 / slowest while ticks run (c=1, 45 s) | — | 6.8-7.3 / 20-25 ms | 5.0-5.3 / 16-22 ms |

\* bind-mounted binary: RssFile reads near 0 there, so VmRSS of a normal image
is about 4.4 MB higher.

Load, busy DB, p50 / p99 ms (baseline → before; "after" from a shorter run,
6 s per cell, two rounds, after the migration, sampler and cache commits):

| Route | c=1 baseline | c=1 before | c=1 after | c=8 baseline | c=8 before | c=8 after |
|---|---|---|---|---|---|---|
| `/` | 8.8 / 17.7 | 15.7 / 34.5 | 6.6-8.4 / 11-19 | 58 / 86 | 79 / 125 | 36-51 / 72-103 |
| `/containers` | 5.3 / 9.9 | 6.5 / 11.1 | 5.5-5.6 / 10-11 | 32 / 57 | 38 / 67 | 37-38 / 64-78 |
| `/host` | 32 / 64 | 37 / 64 | 7.1-8.5 / 14-19 † | 232 / 328 | 273 / 407 | 50-56 / 90-98 † |
| `/history` | 49 / 94 | 77 / — | not re-run | 390 / 804 | 608 / 1317 | not re-run |
| `/alerts` | 3.7 / 6.9 | 6.7 / 11.3 | 6.2-7.1 / 16-20 | 19 / 32 | 39 / 79 | 38 / 72-99 |
| `/api/state` | 2.0 / 3.2 | 2.2 / 4.0 | 2.2 / 3.9-4.1 | 12 / 20 | 13 / 24 | 13 / 19-21 |

† `/host` repeats within the 2 s reuse window under load, so it measures the
cached path; the full render is in the server-time table above.

Not measured in this phase: browser metrics (LCP, CLS, INP, long tasks at 4x
and 390 px), the 150-row table's DOM size and render time, log-burst
main-thread time, page bytes after the UI pass, RAM with one viewer. The
baseline values for these are in the baseline report of this pass (ROB-05:
6,035 nodes, a 1,025 ms long task at 4x; LG-18: 48 long tasks of 60-780 ms).

### Schema migration v6

Adds `events_ts (ts)` and `events_kind_ts (kind, ts, subject)`. On the steady
database: file 131.3 → 136.7 MiB (+5.3 MiB, about 4 %); first start 590 ms
(26 ms on later starts); the two `CREATE INDEX` alone 178 ms. **An older build
refuses a v6 database** ("newer than this build understands"): a rollback needs
the copy of `hobnail.db` taken before the upgrade. Restart counts: 96 → 2.1 ms
(covering index); retention count 5.8 → 0.005 ms; a 1 h window 3 days back
6.0 → 0.04 ms; the newest 24 h window 0.54 → 0.98 ms (index plus sort, still
small).

### Ranked list and decisions

| # | Candidate | Measured cost before | Expected gain | Cost / risk | Decision |
|---|---|---|---|---|---|
| 1 | Index on events, used by restart counts, History, Host markers, strips, retention | `/history` 800 ms, detail 120 ms at steady state | 10-20× on those pages | +5.3 MiB DB at 90 d; one-way migration | **done** `8461d1b` |
| 2 | SQLite page cache released when idle | 2.2 MiB of the 3.4 MiB heap at idle | −2.2 MiB idle | 7-day API call 7 → 12 ms | **done** `47f31b0` |
| 3 | Sampler off the reactor; cgroup files once; no inspect per tick | 3.4k syscalls per tick, 1 inspect per tick | −15 % syscalls, no reactor stall | none | **done** `4b893b3` |
| 4 | Second read-only connection | `/containers` p90 732 ms during `/history` (baseline) | after 1-3: p90 20 ms, p99 30 ms | +1 page cache of RAM | **not needed**: the remaining contention is the 2 blocking threads and CPU, which a second connection does not change |
| 5 | Host poll vs 2 s cache | one viewer 310 ms CPU / 60 s (~13 ms per poll) | — | — | **no change**: one viewer is already one scan per 3 s poll; two pollers 1.4 s apart cost 330-350 ms (they share the cache); with no viewer no scan runs |
| 6 | `opt-level = 3` instead of `"s"` | — | faster renders | binary 6.63 → 9.16 MB (+2.5 MB) | **tried, did not help**: `/` c1 p50 6.6-8.4 vs 7.5-8.1 ms, `/containers` 5.5-5.6 vs 5.6-5.8 ms, `/api/state` 2.2 vs 1.8 ms; no route clearly faster beyond noise |
| 7 | gzip for HTML and static assets (ROB-04, ROB-10) | pages 94-530 KB raw | 8-12× fewer bytes | new crate (`miniz_oxide`, not in the tree) | **not done** (phase stopped; needs the owner's agreement for the crate) |
| 8 | 150-row containers table: `content-visibility` on phone-layout stack groups (ROB-05) | 6,035 nodes, 1 s long task at 4x (baseline) | — | — | **not done** |
| 9 | Log bursts: render per frame within a time budget (LG-18) | 48 long tasks 60-780 ms (QA) | — | — | **not done** (written, not measured, reverted) |
| 10 | `/alerts`, `/` server time grew in the UI pass (3.7 → 6.7 ms, 8.8 → 15.7 ms c1) | — | — | — | **not done**, not profiled |
| 11 | Allocator change | — | — | new crate, RSS risk | **not done**: single reactor thread, no contention measured |
| 12 | Dockerfile: build into a per-checkout target dir | stale-artefact risk | — | — | proposal only (see report) |

### Correction: a busy 24 h window was slower after `8461d1b` (fixed in the next commit)

The first version of the index change made `/history` **slower** on a database
whose 24 h window holds most of the table (the shared test instance: 8,716
events, 7,887 in the last 24 h): 57-76 ms before, 0.20-0.26 s at `7a79581`.
Cause, from `EXPLAIN QUERY PLAN` on a copy of that database: the History scan
read the window in batches of 500 ordered by `id` with `ts` bounds, which
SQLite answered as `SEARCH events USING INDEX events_ts (ts>? AND ts<?)` +
`USE TEMP B-TREE FOR ORDER BY`, so each of the 16 batches sorted the whole
remaining window again: 101-126 ms of SQL (Python sqlite3, warm). One query
in index order (`ORDER BY ts DESC, id DESC`, same bounds, same cap of 10,000
rows, same `before=<id>` paging and signed-out `auth.*` filter) has the plan
`SEARCH events USING INDEX events_ts (ts>? AND ts<?)` with no sort: 12-17 ms.
The page-cache release (`47f31b0`) was not the cause: the Python timings above
run without it.

Server time (curl TTFB on loopback, median of 9, two rounds, three binaries in
the same image on copies of the same databases; ms):

| Page | busy DB `49a8421` | busy `7a79581` | busy fix | 90-day `49a8421` | 90-day `7a79581` | 90-day fix |
|---|---|---|---|---|---|---|
| `/history` | 71 | 216-222 | 63-71 | 759-774 | 44-46 | 39 |
| `/history?range=7d` | 59-60 | 233-235 | 50-53 | 860-908 | 132-138 | 123-126 |
| `/history?range=1h` | 37-53 | 16 | 16-20 | 729-748 | 19-21 | 14-17 |
| `/host` | 31-53 | 32-35 | 30 | 43-57 | 37-39 | 31-48 |
| `/containers/<name>` | 9-14 | 9 | 8-12 | 109-119 | 9-13 | 9-10 |

### Proposed wording for GOALS.md section 1 (for the owner; GOALS.md not edited)

> | Metric | Target | Stretch | Measured 2026-11-17 |
> |---|---|---|---|
> | Idle RAM (docker stats), no viewers, with the history database | ≤ 15 MiB | ≤ 8 MiB | 3.8 MiB (RssAnon 1.2 MiB) |
> | Container image | ≤ 20 MB | ≤ 10 MB | ~11.6 MB |
> | Idle CPU (sampler, events, alerts) | ~0 % | ~0 % | 0.07-0.10 % of one core |
> | Cold start, **binary** (exec → first `/healthz` 200, no Docker) | < 200 ms | < 50 ms | 3.5 ms |
> | Cold start, container (`docker start` → first 200) | reported, no target | — | 460 ms on a loaded host; Docker's share, not the app's |
> | Page served (server time) | < 150 ms | < 80 ms | ≤ 71 ms for every page on the test databases (`/history?range=7d` 123-126 ms on 90 days of data) |
>
> The cold-start target means the binary: the container figure is runc and
> Docker networking before `main()` runs, which the app cannot change.

## React port, phases 0 and 1 (2026-11-18)

Full results, method and raw samples: [plans/react-port-phase-0-1.md](plans/react-port-phase-0-1.md)
and [plans/react-port-raw/](plans/react-port-raw/). Not deployed; throwaway
containers on 127.0.0.1, warm cache, the test instance's config, fresh database.

| Metric | main `680a339` | React port phase 1 |
|---|---|---|
| Release binary | 6,633,904 B | 7,780,784 B |
| Image (`docker images`) | 11.6 MB | 13.3 MB (both UIs ship) |
| `memory.current` max at 1 / 10 / 25 clients | 6.01 / 6.55 / 7.51 MiB | 3.41 / 3.90 / 4.55 MiB (JSON API); differences are page-cache attribution, RssAnon equal |
| `memory.peak`, whole run | 8.84 MiB | 6.29 MiB |
| Client first route | 227.5 KB uncompressed | 105.5 KB gzip |
| 150-row table, phone 4×: longest load task | 1,791 to 1,838 ms | 151 to 169 ms (trace), 169 to 220 ms (long-task observer) |
| 150-row table, phone 4×: live update | 159 to 204 ms | 27 to 32 ms |
