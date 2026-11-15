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
