# React port, phases 2 to 5: results

Date: 2026-11-19. Spec: [react-port.md](react-port.md), especially §4.9;
[locked decisions](react-port-decisions.md) override its recommendations.
The measurement run used disposable instances. The [deployment handover](react-port-handover.md) records the later preview and verification.

Marks: **[code]** verified in code/history; **[exp]** measured here;
**[assume]** not verified. MiB = 2^20 B, MB = 10^6 B, KB = 10^3 B.
Asset bytes and application runtime RAM are separate figures.

## Build and conditions

**[exp]** Image `hobnail-rp:m5` was built from **b93b92ec9a5b8c259695adb8896ad8b82e71d48d**
with `DOCKER_BUILDKIT=1 docker build --progress=plain --build-arg
RH_GIT_SHA=$(git rev-parse --short HEAD) -t hobnail-rp:m5 .`.
Image ID/manifest: `sha256:14897f701f9943b39f37182cb9eeaf7b9c19beda34e66312de6664b86bac466f`.
Printed content hash: `93da0f7f50edb275af333c569d9641b4981b4aa3e63f06c70fd72a87d34b3196`.
Later commits change documentation and the host benchmark script only; embedded
Rust/client inputs are unchanged. The first build attempt exposed an excluded
API-reference input; `.dockerignore` was fixed before this successful build.

Shared x86_64 host: four logical CPUs, 7.2 GiB RAM, Linux 7.0.0-38-generic,
Docker 29.5.2. Application containers had no memory cap. Image/assets were warm;
no kernel caches were dropped. Browser checks used separate containers and
fresh browser caches. The first performance run overlapped Rust checks, the
memory run and browser parity checks; the second overlapped logs checks and
memory work. These are observations under contention, not isolated latency
baselines. No request went to port 3200, no command addressed the deployed
container, and no production data directory was read or copied.

Web stages were cached in the successful Docker build, so their wall time is
not a fresh-build measurement. Rust stage: **225.4 s**, including clippy, tests
and release; release compilation/link: **2m 44s**. Separately, `scripts/web.sh ci`
with an empty ephemeral npm cache took **50.16 s** including Docker startup
(npm reported 47 s); with the existing named cache **16.67 s** (npm 9 s).
Both installed the same lock with scripts disabled. The web check/test/build/size
were also rerun through `scripts/web.sh`; Vite's own timing is retained in the
raw build output. No new packages or crates were added.
Evidence: [build](react-port-raw/m5/build.log),
[cold npm](react-port-raw/m5/npm-ci-cold.log), [warm npm](react-port-raw/m5/npm-ci-warm.log).

## Sizes

All **[exp]**. Image display size is the `docker images` figure; content is
`docker image inspect .Size` on this containerd store, not unpacked RAM.

| Build | Release binary B | Image display / content B |
|---|---|---|
| main (`680a339`, retained phase 0–1 baseline) | 6,633,904 | 11.6 MB / 3,910,610 |
| phase 1 | 7,780,784 | 13.3 MB / 4,448,685 |
| M5 (`b93b92e`) | **7,694,768** | **13.3 MB / 4,578,941** |
| Finish Hobnail M1 | 7,723,440 | 13.4 MB / 4,579,996 |

M5 exceeded the original **13 MB** budget. The owner accepted the roughly
13.3 MB image on 2026-11-20. M1 enforces the signed-off **13,500,000 B** limit
using the exact Docker image-list size in `scripts/check-image-size.py`,
called before deployment. The local x86-64 M1 release image used
**13,359,582 B**, leaving 140,418 B of headroom. Historical M5 evidence:
[startup and size output](react-port-raw/m5/startup.log).

## Bundle

**[exp]** `scripts/web.sh run size`, production `web/dist`; sums exclude the
extra `.gz` copies from raw size and sum those copies for gzip size.

| Measure | Raw B | Gzip B | Budget / outcome |
|---|---|---|---|
| First route (HTML + direct entry/modulepreload JS and CSS) | — | **133,192** | ≤180,000: pass |
| Largest non-chart lazy chunk (`assets/Select-Cai2KDVa.js`) | 43,297 | 15,309 | ≤60,000: pass |
| `assets/charts-vendor-C6Pdu-IP.js` | 100,385 | 25,497 | ≤110,000 plan / ≤60,000 script per chunk: pass |
| `assets/charts-vendor-BhgI_-Ol.js` | 133,285 | 37,551 | ≤110,000 plan / ≤60,000 script per chunk: pass |
| `assets/RechartsUsage-Cv9abRUt.js` | 150,919 | 51,137 | ≤110,000 plan / ≤60,000 script per chunk: pass |
| `assets/RechartsUsage-CC9LOCIs.css` | 3,008 | 1,050 | ≤110,000 plan / ≤60,000 script per chunk: pass |
| Whole dist | **1,344,189** | **449,533** | Original ≤1,300,000 / 400,000: **both missed** |
| Whole dist, raised script cap | 1,344,189 | 449,533 | ≤1,350,000 / 450,000: pass (467 gzip bytes remain) |

**[code]** The raised whole-dist cap was a deviation; the owner accepted it on
2026-11-20 (see the sign-off in `react-port-decisions.md`). `web/scripts/size.mjs` also uses
60 KB per chart chunk, stricter than the plan's 110 KB per vendor chunk, but
does not enforce a combined chart download cap. The four chart-named files sum
**115,235 B gzip**: this exceeds 110 KB if that allowance is applied to the
aggregate chart payload; each individual chunk passes. Other shared dependencies
may already be in the first route.

Largest ten files by gzip bytes **[exp]** (filename attribution **[code]**;
shared chunks combine app/framework/library code, so these are file sizes,
not exact per-package tree-shaken totals):

| File | Raw B | Gzip B | Contributor |
|---|---|---|---|
| `assets/web-fgg0mksI.js` | 285,476 | 90,060 | UI SDK, React DOM and app shell (mixed) |
| `assets/RechartsUsage-Cv9abRUt.js` | 150,919 | 51,137 | App chart components and shared chart dependencies |
| `assets/charts-vendor-BhgI_-Ol.js` | 133,285 | 37,551 | Recharts implementation |
| `assets/charts-vendor-C6Pdu-IP.js` | 100,385 | 25,497 | Recharts implementation |
| `assets/Select-Cai2KDVa.js` | 43,297 | 15,309 | Select wrapper and UI SDK interaction code |
| `assets/logs-CLb8lUjJ.js` | 43,129 | 14,342 | App route/shared code |
| `assets/sheet-presentation-DyhvGWRA.js` | 37,481 | 13,491 | UI SDK sheet presentation |
| `assets/popup-C1bZqKLS.js` | 33,727 | 12,833 | UI SDK popup runtime |
| `assets/web-DupGL4Wj.css` | 65,569 | 12,460 | Trimmed UI SDK stylesheet |
| `assets/use-primitive-BWYySHcm.js` | 27,655 | 11,057 | UI SDK primitive runtime |

React's separate runtime file is 7,876 raw / 3,011 gzip B; SDK primitive runtime
`use-primitive` is 27,655 / 11,057 B. React DOM also lives in the mixed `web` JS
chunk. SDK popup/interaction chunks are lazy. The SDK stylesheet is trimmed
by the existing Vite PostCSS pass; no package attribution is inferred from a
chunk's name alone. [Full file inventory](react-port-raw/m5/web-size.log).

## Memory and CPU

### Finish-hobnail M5 server memory

2026-11-22, x86-64 Core i5-7200U, four logical CPUs, 7.15 GiB host RAM, Linux
7.0.0-38-generic, Docker 29.5.2, cgroup v2. The release image was built from
runtime commit `e2cd77b` using the repository Dockerfile. Image
`hobnail-m5:history`, content hash
`eb31290f2ad731569cf3fef4343fa0399a12371b2021705c86419dd5f8ae5149`. One
application container on a throwaway loopback port, with Docker's healthcheck,
read-only root, no capabilities and separate data. The fake Docker daemon, fake
upstreams, HTTP drivers, Chromium, build tools and host services were outside the
application cgroup.

The unlimited run starts with an owner-free database. A separate process preloaded
the full 6 h raw, 7 d one-minute and 90 d ten-minute history windows before the
run, plus 100,000 events. The fixture has 150 live Docker entries, with
long-retention series for 40 containers and the host. It includes host, container,
container-extras and log-count tables. The database was 175.30 MB on disk. Startup
was observed for 10 seconds, idle and cleanup for 60 seconds each. The run
includes fresh owner creation, sign-in, guided board setup and installs, followed
by 60 seconds each of 1, 10 and 25 live clients. Each client keeps a board-scoped
SSE stream open, reads its board and the 150-container collection every two
seconds, reads system chart data every five seconds, bootstraps every 30 seconds
and fetches the shell and production asset set initially and every 20 seconds. All
18 installed widgets reached `ready` and refresh against fakes for Pi-hole, Uptime
Kuma, Jellyfin, qBittorrent, Sonarr, Radarr, Transmission, Home Assistant, Proxmox
VE, Generic HTTP, RSS, TLS certificate expiry and reachability.

After the clients disconnected, the workload read 24-hour comparisons and 30-day
history, 24-hour host history, and 30-day container history ten times each. It
then issued four concurrent 5,000-line log reads in each of three rounds, and ten
rounds of board saves, settings writes, integration edits and connection tests.
The populated system charts returned 360 buckets per metric at 24 h and 392 at 30
d. Startup sampling begins after Docker makes the cgroup accessible. Nominal
sample intervals were 500 ms and 20 ms for owner creation/sign-in. Bytes in the
tables are converted using MiB = 1,048,576 B. Memory columns report median /
maximum, except kernel `memory.peak`, which is cumulative through each phase.
`Kernel` and `sock` come from `memory.stat`. The working set subtracts
`inactive_file` from `memory.current`. Stream headers count as HTTP requests.
Named events, including log completion, count separately as SSE events. Latencies
include response reads.

| Phase | n | Current | Working set | Kernel peak | Anon | File | Kernel | Sock | One-core CPU % | Requests / errors / SSE events | p50 / p99 ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| startup | 20 | 1.82 / 4.37 | 1.52 / 4.34 | 4.61 | 1.20 / 3.26 | 0.30 / 0.30 | 0.31 / 0.32 | 0.00 / 0.00 | 0.93 | 0 / 0 / 0 | – / – |
| owner+login | 9 | 5.01 / 9.26 | 4.71 / 8.96 | 9.26 | 3.67 / 8.33 | 0.34 / 0.37 | 0.31 / 0.31 | 0.00 / 0.00 | 53.10 | 2 / 0 / 0 | 57.1 / 75.4 |
| guided-setup | 2 | 2.53 / 2.55 | 2.13 / 2.14 | 9.26 | 1.33 / 1.35 | 0.40 / 0.43 | 0.30 / 0.30 | 0.00 / 0.00 | 15.29 | 10 / 0 / 0 | 1.2 / 3.2 |
| providers | 1 | 3.31 / 3.31 | 2.23 / 2.23 | 9.26 | 1.36 / 1.36 | 1.08 / 1.08 | 0.32 / 0.32 | 0.00 / 0.00 | – | 14 / 0 / 0 | 1.9 / 2.7 |
| poll-warmup | 7 | 5.13 / 5.49 | 4.05 / 4.41 | 9.26 | 3.48 / 3.49 | 1.08 / 1.08 | 0.36 / 0.37 | 0.00 / 0.00 | 1.93 | 6 / 0 / 41 | 4.6 / 15.5 |
| idle | 119 | 3.95 / 5.82 | 1.97 / 4.68 | 9.26 | 1.49 / 3.48 | 2.01 / 2.47 | 0.47 / 0.60 | 0.00 / 0.00 | 0.79 | 0 / 0 / 0 | – / – |
| clients-1 | 118 | 5.20 / 5.86 | 2.72 / 3.38 | 9.26 | 1.81 / 2.02 | 2.48 / 2.48 | 0.54 / 0.55 | 0.00 / 0.02 | 2.39 | 301 / 0 / 125 | 0.7 / 21.9 |
| clients-10 | 120 | 6.10 / 6.55 | 3.60 / 4.06 | 9.26 | 2.41 / 2.70 | 2.50 / 2.50 | 0.62 / 0.63 | 0.00 / 0.07 | 14.20 | 3001 / 0 / 1250 | 1.1 / 24.2 |
| clients-25 | 115 | 7.16 / 7.75 | 4.66 / 5.23 | 9.50 | 3.41 / 3.69 | 2.52 / 2.52 | 0.73 / 0.74 | 0.00 / 0.08 | 27.73 | 7502 / 0 / 3125 | 1.6 / 59.2 |
| full-history | 28 | 7.92 / 12.24 | 5.40 / 9.72 | 12.45 | 4.37 / 8.46 | 2.52 / 2.52 | 0.55 / 0.56 | 0.00 / 0.00 | 55.87 | 40 / 0 / 0 | 218.5 / 562.5 |
| log-buffers | 5 | 7.25 / 8.62 | 4.73 / 6.11 | 12.45 | 4.18 / 5.11 | 2.52 / 2.52 | 0.55 / 0.57 | 0.00 / 0.00 | 18.70 | 12 / 0 / 96 | 118.7 / 222.9 |
| config-writes | 69 | 5.21 / 5.88 | 2.68 / 3.35 | 12.45 | 2.14 / 2.19 | 2.52 / 2.53 | 0.49 / 0.55 | 0.00 / 0.00 | 0.97 | 50 / 0 / 0 | 3.8 / 21.0 |
| cleanup | 117 | 5.12 / 5.70 | 2.58 / 3.16 | 12.45 | 2.08 / 2.20 | 2.54 / 2.54 | 0.48 / 0.56 | 0.00 / 0.00 | 0.31 | 0 / 0 / 0 | – / – |

The uncapped lifetime peak was **13,049,856 B (12.45 MiB)**. Every phase median
was under 10 MiB. Full history crossed the preferred 10 MiB ceiling, and the
lifetime peak stayed below 20 MiB including owner creation and login. The run
recorded 10,938 successful HTTP requests and 4,637 SSE events, with zero errors.

The same workload at `--memory 20m --memory-swap 20m` recorded **12,836,864 B
(12.24 MiB)** lifetime peak, 10,938 successful HTTP requests, 4,637 SSE events,
zero errors and zero OOM kills. Cgroup `memory.events` recorded `max=0`, `oom=0`
and `oom_kill=0`. Sampled anon/file/kernel/sock maxima were 8.02 / 2.54 / 0.84 /
0.22 MiB. Three additional capped runs initialized SQLite from empty data and
completed owner creation, login and guided setup. The hard limit was active before
app startup in these runs and in the full validation. The sub-500 ms owner/login
phase fell between sampler wakeups in the hard-limit run. Its two requests are
present and its memory is included in the kernel lifetime peak. The per-phase
hard-limit summary is stored with its raw samples.

**Cache attribution and other peaks.** File pages are charged to the cgroup that
first brings them into cache. A warm image shared across containers and a database
seeded from the host can therefore produce lower charges than the same workload
reading cold pages. The uncapped baseline with the same large fixture recorded
**75,165,696 B (71.68 MiB)** at minute-one maintenance. The largest sample was
71.20 MiB, including 3.66 MiB anonymous, 66.89 MiB file, 0.52 MiB kernel and less
than 0.01 MiB socket memory. Another warm run of the baseline stayed at 6.15 MiB
sampled through idle, with maxima of 2.96 MiB anonymous and 2.34 MiB file. Its
SQLite trace recorded roughly 174 MiB of reads from the database around the first
maintenance, while the WAL stayed small. These runs show the effect of cache
attribution during history reads and maintenance. The warm results depend on cache
residency.

The same release was also probed after evicting the 175.30 MB SQLite file from the
host page cache with `POSIX_FADV_DONTNEED`. The uncapped probe had already peaked
at **42.56 MiB** by guided setup. The lifetime peak was **187,559,936 B (178.87
MiB)** after minute-one maintenance and history reads. Its highest sample had
178.46 MiB current, 8.87 MiB working set and 7.70 / 169.59 / 0.88 / 0.00 MiB
anon/file/kernel/sock. The uncapped 20 MiB target therefore remains unmet for a
cold, populated database. The capped cold probe reached 20,971,520 B (20.00 MiB),
with 7.84 / 11.24 / 0.45 / 0.00 MiB anon/file/kernel/sock at its highest sample.
Startup, authentication and 40 full-history reads completed. The capped cgroup
recorded `max=1458`, `oom=0` and `oom_kill=0`. Both short probes omitted the live
dashboard phases. Each returned twelve HTTP 400 responses for standalone log
requests, which require visible containers. The full warm runs exercised the log
streams successfully.

The interval may miss short-lived maxima in the sampled breakdown. The lifetime
peak records those maxima without identifying the underlying memory class. The
current image uses musl and SQLite releases its page cache after sampler ticks.
The History API now retains only the requested page and 200 markers while scanning
up to 10,000 borrowed rows from SQLite. In an event-scan comparison with 100,000
health events and 20 ms sampling, 40 history requests per build passed. Sampled
anonymous maximum fell from 5.07 to 4.18 MiB and total sampled maximum from 7.03
to 6.33 MiB. File maximum was 1.04 / 1.20 MiB, kernel 0.36 / 0.37 MiB and socket
0.00 / 0.38 MiB. History p50/p99 was 74.2/108.8 ms before and 63.8/125.2 ms after.
The main workload uses restart events and retained a higher History peak. After
disconnect, anonymous median was 2.08 MiB, down from 3.41 MiB with 25 viewers and
above the initial 1.49 MiB idle median.

**Authentication.** Three fresh release-container trials per profile, each with
one owner creation and seven logins. Samples targeted 5 ms during credentials.
Argon2id v19 and one lane in all cases. The host also ran builds during some
trials. [Equivalent OWASP
profiles](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#argon2id)

| Memory / passes | Kernel peak | Anon / file / kernel / sock maxima (MiB) | Login median / max (ms) |
|---|---:|---:|---:|
| 19,456 KiB / 2, baseline | 22,487,040 B (21.45 MiB) | 19.80 / 0.53 / 0.37 / <0.01 | 56.2 / 120.3 |
| 9,216 KiB / 4 | 17,268,736 B (16.47 MiB) | 9.81 / 5.66 / 0.34 / <0.01 | 105.4 / 147.7 |
| 7,168 KiB / 5, selected | 9,551,872 B (9.11 MiB) | 7.80 / 0.53 / 0.34 / <0.01 | 73.6 / 183.4 |

The 9 MiB trial with the highest charge had 5.66 MiB of file cache. The anonymous
measurements show the effect of each work factor. A first 7 MiB series peaked at
9.30 MiB (7.81 MiB anon, 0.54 MiB file) before a scratch quota interruption. Login
verifies a legacy hash with its original parameters and writes the new salted hash
after creating the session. An uncapped first legacy login peaked at 21,524,480 B
(20.53 MiB), with maxima of 19.70 MiB anon, 0.25 MiB file, 0.35 MiB kernel and
less than 0.01 MiB socket. An existing legacy account's first sign-in under the
hard limit reached 20,971,520 B and was OOM-killed. That remains a migration risk.

**Guided setup storage investigation.** The old 19 MiB build disconnected during
fresh owner creation in all three capped attempts. Docker reported OOM kills after
those failures. One surviving cgroup sample recorded 1 `oom_kill` and a peak of
20,971,520 B. The old phone screenshot showed `dashboard storage unavailable` at
board commit. Three uncapped replays on the baseline and three replays with the
new hash profile under the hard cap completed the same 39 phone checks. The capped
phone peaks were 9,965,568 B, 9,973,760 B and 9,445,376 B. The highest sample
components across those replays were 8.39 MiB anon, 0.54 MiB file, 0.49 MiB kernel
and 0.01 MiB socket.

Filling a disposable SQLite data mount reproduced the exact HTTP 500 text. The
application's `pwrite64` returned `ENOSPC` (`SQLITE_FULL` in the bundled VFS). The
cgroup lifetime peak was 9,711,616 B (9.26 MiB), and at injection it held 0.80 MiB
anon, 0.41 MiB file, 0.30 MiB kernel and less than 0.01 MiB socket. Cgroup memory
events remained zero. The regression check verified that no board was published,
the draft survived, and retry created a complete 128-item board after space was
freed. The original M4 run used the host's quota-limited tmpfs for app data. That
tmpfs returned `EDQUOT` for scratch writes during M5, making storage exhaustion
plausible. The original phone incident has no recorded SQLite code, so its exact
cause remains unverified.

**Artifact sizes.** Production web assets are 1,349,132 B raw / 438,069 B gzip
(decimal bundle units). The three chart-named files total 104,762 B gzip. The
executable is 8,964,656 B, embedded assets included. Docker's unpacked size is
15,147,903 B (15.15 decimal MB) and its content size is 5,127,236 B (5.13 decimal
MB). The unpacked image also missed the 13.5 MB gate on the baseline, at
15,135,612 B. These are artifact bytes and are reported separately from cgroup
RAM.

**Checks.** The web check, 152 web tests, production build and bundle size check
passed. Rust formatting, all-target Clippy with warnings denied, and 500 Rust
tests passed. The repository Dockerfile also completed the release build. The
separate unpacked image size gate reported the miss above.

**Raw evidence.** All directories are below
`/tmp/claude-1000/-home-dev-projects-rust-homepage/c3c8671c-4fd5-4601-90a5-2f27d491fe9d/scratchpad/m5-mem/`.
`disk/` points to disk-backed scratch after the tmpfs quota interruption. The
release runs are `disk/final-unlimited/` and `disk/final-hard/`, with
`samples.jsonl`, `meta.jsonl`, `requests.jsonl`, `sse.jsonl` and `fixture.jsonl`.
The same directory holds the workload driver `bench.py`, `summarize.py`, the
`disk-fault.py` regression and its JSONL output, capped phone checks
(`browser-hard-replay/`), and the legacy-login runs. The alternative hashes and
the production profile are in `auth-baseline/`, `auth-9m/` and
`disk/auth-7m-repeat/`. Baseline large-history runs and maintenance tracing are in
`baseline-complete/`, `maintenance-raw/` and `maintenance-io.jsonl`. The isolated
event scans are in `disk/history-before/` and `disk/history-after/`. The fresh
hard-limit setup runs are in `disk/final-hard-fresh/` and the old failure replays
are in `disk/baseline-hard-confirm/`. The cold database probes are in
`disk/final-cold-db/` and `disk/final-cold-db-hard/`.

From the repository root, use `python3 "$scratch/disk/bench.py" hobnail-m5:history
<new-output-name> --mode full --port 3396`, where `scratch` is the directory
above. Add `--hard` to apply the cap.

### Earlier React port measurements

**[exp]** `PORT=3395 scripts/bench-mem.sh hobnail-rp:m5 <fresh-output-dir>`;
no config argument, a new empty data directory/database, real Docker socket
read-only and host proc/sys read-only. No production configuration was used.
One application cgroup, no limit, warm cache. Load clients run elsewhere.
Startup: nominal 10 s/100 ms (sampling begins after cgroup discovery; the
script rounds the remaining window to whole seconds); idle: 90 s/500 ms; 1, 10, 25 clients: 60 s each/500 ms;
after-load: 60 s/500 ms. Requests use an authenticated session, JSON containers
every 2 s, legacy JSON system history every 5 s, one JSON SSE connection per
client and every embedded client asset at start/every 20 s.

The script originally fetched only index references. A completed diagnostic
run is retained separately and is **not** the full-workload result. The first
asset-graph fix missed the backtick-quoted search worker; that run was stopped.
The corrected final run traverses all **89 dist files** (the index is requested
as `/app`), checked against the built dist inventory. Discovery fetches the
graph before each client phase. Each phase's load process signs in once; the
first onboards, so lifetime peaks include argon2 even when 500 ms steady-state
samples miss it. The script fixes are separate commits and do not change the
embedded runtime or require rebuilding the image.

| Phase | Samples | `memory.current` median / max | Working set median / max | `memory.peak` at end | anon / file / kernel / sock median | CPU % of one core | Requests / errors / events | p50 / p99 ms |
|---|---|---|---|---|---|---|---|---|
| startup | 100 | 1.54 / 1.96 | 1.27 / 1.71 | 4.06 | 0.78 / 0.26 / 0.41 / 0.00 | 0.46 | – | – |
| idle | 180 | 1.64 / 2.28 | 1.28 / 2.02 | 4.98 | 0.83 / 0.32 / 0.39 / 0.00 | 0.32 | – | – |
| clients-1 | 120 | 2.29 / 3.07 | 1.73 / 2.45 | 21.44 | 1.09 / 0.60 / 0.45 / 0.00 | 0.66 | 310 / 0 / 37 | 1.5 / 6.6 |
| clients-10 | 120 | 3.23 / 4.49 | 2.46 / 3.61 | 21.95 | 1.38 / 0.75 / 0.52 / 0.00 | 3.98 | 3100 / 0 / 364 | 1.4 / 10.7 |
| clients-25 | 120 | 3.93 / 4.29 | 2.91 / 3.29 | 22.24 | 1.75 / 1.00 / 0.58 / 0.00 | 7.98 | 7750 / 0 / 831 | 3.1 / 16.4 |
| after | 120 | 2.74 / 3.38 | 1.50 / 2.25 | 22.24 | 1.05 / 1.22 / 0.43 / 0.00 | 0.24 | – | – |

Process RSS at phase ends (MiB, from /proc/<pid>/status):

| Phase | RssAnon | RssFile | RssShmem | VmRSS |
|---|---|---|---|---|
| startup | 0.89 | 4.39 | 0.00 | 5.28 |
| idle | 0.88 | 4.45 | 0.00 | 5.34 |
| clients-1 | 1.08 | 5.83 | 0.00 | 6.91 |
| clients-10 | 1.13 | 5.83 | 0.00 | 6.96 |
| clients-25 | 1.14 | 5.83 | 0.00 | 6.96 |
| after | 1.03 | 5.83 | 0.00 | 6.86 |

Endpoints: stream `/api/v1/stream?topics=containers,stats,system`, state `/api/v1/containers`, history `/api/history/system?metric=cpu&range=1h`, assets 89: `/app`, `/assets/Appearance-KEv8dnBm.js`, `/assets/Bookmarks-BRwpcWsu.js`, `/assets/Button-DwrunF04.js`, `/assets/Collection-BchEO3Dq.js`, `/assets/Config-DF2qPR0g.js`, `/assets/Containers-DUvRjNnR.js`, `/assets/Dialog-B3uVPhtz.js`, `/assets/EditDialog-BARi-kyo.js`, `/assets/Editor-e7wlhjy2.js`, `/assets/Field-7RUjToNA.js`, `/assets/General-BS7yNxbg.js`, `/assets/History-CSQqn_iB.js`, `/assets/MenuImpl-GkuM2KrT.js`, `/assets/Notifications-C6FCuCAV.js`, `/assets/Palette-D4ScbimI.css`, `/assets/Palette-yNr_pyGY.js`, `/assets/RechartsUsage-CC9LOCIs.css`, `/assets/RechartsUsage-Cv9abRUt.js`, `/assets/RelativeTime-C4TN0sAR.js`, `/assets/Security-8lIEN2kZ.js`, `/assets/Select-Cai2KDVa.js`, `/assets/SettingsForm-DILsZIp3.js`, `/assets/Shortcuts-DSdpKul5.js`, `/assets/Switch-BUS2Y8uf.js`, `/assets/TabNav-CRIRui1C.js`, `/assets/Tabs-ExaiD3g4.js`, `/assets/Toaster-B_zV1Cvv.js`, `/assets/Users-BuWt-_Z_.js`, `/assets/WidgetContent-Dzg06snL.js`, `/assets/Widgets-DmWyKGaL.js`, `/assets/account-B5ciHFge.js`, `/assets/alerts-Y0bxH_1U.js`, `/assets/announce-BaDTwOl2.js`, `/assets/app-runtime-DLsVIt_d.js`, `/assets/board-Di-mSvph.css`, `/assets/board-J3Sjea6r.js`, `/assets/bookmarks-HIjuKMXL.js`, `/assets/button-dp3fTQDb.js`, `/assets/charts-vendor-BhgI_-Ol.js`, `/assets/charts-vendor-C6Pdu-IP.js`, `/assets/children-CYU4CyM7.js`, `/assets/container-BG7CEc1s.js`, `/assets/container-gV5Acizf.css`, `/assets/containers-C9ZYqLk9.js`, `/assets/containers-CNkSD3_X.js`, `/assets/containers-DIOiKCIT.css`, `/assets/createLucideIcon-CRbusLLJ.js`, `/assets/env-i8CXn8sH.js`, `/assets/events-B2Bcfi6U.js`, `/assets/events-BCX97W3_.js`, `/assets/format-CF5Z1gM7.js`, `/assets/forms-DU4HvGk8.js`, `/assets/history-1HslwQfM.js`, `/assets/history-BRBLfKut.js`, `/assets/history-Dlkw40-6.css`, `/assets/host-BUbc8QtP.css`, `/assets/host-Odp2i3mJ.js`, `/assets/index-0Y5ftNI3.js`, `/assets/keyboard-HHPkxq8T.js`, `/assets/layers-B7tDtED_.js`, `/assets/live-DqPGC3QO.js`, `/assets/logs-C7iCcirE.css`, `/assets/logs-CLb8lUjJ.js`, `/assets/modal-attribute-BP04ZJUB.js`, `/assets/overview-Bn4RnX6U.css`, `/assets/overview-D7rpB5gY.js`, `/assets/parts-GyezLEtB.js`, `/assets/pointer-BBzP2ZJc.js`, `/assets/popup-C1bZqKLS.js`, `/assets/preload-helper-Dp5Bp1yV.js`, `/assets/react-CdlCbvX4.js`, `/assets/read-DszbDfUj.js`, `/assets/rolldown-runtime-CNOWq4Kh.js`, `/assets/search-worker-B5Paj1wr.js`, `/assets/session-7ttDCFor.js`, `/assets/settings-DDwTshO5.js`, `/assets/settings-DktKE8Cv.css`, `/assets/settings-Dq18O-bI.js`, `/assets/shared-Biha3iLl.js`, `/assets/sheet-presentation-DyhvGWRA.js`, `/assets/shortcut-slot-Cs7WM6Rv.js`, `/assets/shortcuts-B26-QLIQ.js`, `/assets/sides-Ce7I_LlF.js`, `/assets/triangle-exclamation-point-Bxrvyj48.js`, `/assets/typeahead-D5O04iSg.js`, `/assets/use-primitive-BWYySHcm.js`, `/assets/web-DupGL4Wj.css`, `/assets/web-fgg0mksI.js`


Exact cgroup byte figures (to preserve thresholds independently of displayed MiB):

| Phase | Samples | Current median B | Current max B | Peak at end B |
|---|---|---|---|---|
| startup | 100 | 1613824.0 | 2056192 | 4255744 |
| idle | 180 | 1722368.0 | 2392064 | 5222400 |
| clients-1 | 120 | 2400256.0 | 3219456 | 22478848 |
| clients-10 | 120 | 3389440.0 | 4706304 | 23011328 |
| clients-25 | 120 | 4116480.0 | 4493312 | 23322624 |
| after | 120 | 2875392.0 | 3547136 | 23322624 |

Budgets: idle median ≤8 MiB; max at each client level ≤10 MiB. Compare the
figures above directly; the **≤20 MiB lifetime peak budget is missed**: **23,322,624 B =
22.2421875 MiB**. There were **11,160 requests, zero errors and 1,232 SSE
events** across the three load phases. The
peak is not a memory limit. Warm page-cache charges are shared with other
containers; lower charged `file` cannot be claimed as an allocator saving.
The phase 1 idle median was 2.03 MiB under different configuration/cache
conditions; this is not a controlled regression comparison.
[CSV](react-port-raw/m5/memory-samples.csv), [RSS](react-port-raw/m5/memory-rss.csv),
[client results](react-port-raw/m5/memory-clients.json),
[metadata](react-port-raw/m5/memory-meta.txt),
[asset endpoints](react-port-raw/m5/memory-endpoints.json).

**Separate sign-in burst [exp].** Fresh container/data on 3397, no Docker socket
or host mounts; owner onboarding followed by five sequential logins, 100 ms
between calls; cgroup samples every 5 ms, 404 samples overall. All five returned
200 (onboarding 201); durations: 121.94599397480488, 191.46260805428028,
83.46963068470359, 140.73411701247096, 77.46229600161314 ms. Peak before the
burst (including onboarding): **21,823,488 B**. Max sampled current during the
five sign-ins: **21,786,624 B**; lifetime memory.peak after: **21,823,488 B**.
Both exceed 20 MiB (20,971,520 B). Argon2id uses 19,456 KiB = 19 MiB transient
per hash, before ordinary runtime memory. Sequential logins do not model
concurrent requests or retained credentials in a busy instance.
[Raw burst](react-port-raw/m5/login.json), [5 ms samples](react-port-raw/m5/login-samples.csv).

**Viewer CPU [exp].** One load client's CPU is in the table above (percent of
one core). A separate authenticated Chromium Host page with the host topic
subscribed, fresh empty configuration, real read-only socket/proc/sys, consumed
**519,503 µs over 60.00021409476176 s = 0.8658352438201625%** of one core.
This is below the plan's 1.38% reference, with different host load/data and a
short interval, not a controlled speedup. [Raw Host CPU](react-port-raw/m5/cpu.json).

## Startup

**[exp]** Extract the static release binary from the image; execute `serve` with
RH_LISTEN=127.0.0.1:3397 and a fresh empty RH_DATA_DIR each time; absent Docker
socket; time from process launch until the first loopback `/healthz` 200 using
Python monotonic time/HTTP polling without an intentional polling sleep.
Nine runs in ms: 70.83218498155475, 57.937296107411385, 83.30271486192942,
28.533823788166046, 28.48677011206746, 32.61141711845994,
15.253101009875536, 15.962343197315931, 30.991185922175646.
Median **30.991185922175646 ms**, passing ≤50 ms. These are fresh-data/warm-binary
starts, not dropped-cache measurements. Concurrent work explains no particular
outlier without another experiment. Container start → first 200 in the final memory run: **446 ms**, one start
(host polling begins before `docker start`); the earlier diagnostic run was
391 ms. Docker start itself returned after 450 ms in the final run; health
can answer before the Docker CLI returns. [Raw startup](react-port-raw/m5/startup.log).

## JSON sizes and D9

**[exp]** Authenticated Owner; fake Docker with 150 containers/13 stacks,
short-lived database (not a 30-day history), an explicit 40-link board, no
providers; one response per endpoint. Raw response bytes and hypothetical
`gzip -9` bytes (Node zlib, not HTTP compression) were:

| Response | Raw B | Gzip-9 B |
|---|---|---|
| Containers | 63,725 | 6,596 |
| Strips | 33,170 | 611 |
| Overview | 14,216 | 3,596 |
| History, 24h window on this young database | 990 | 321 |
| Board, 40 link items | 12,547 | 1,757 |

Twelve seconds of SSE were captured with containers/stats/system/host/boards
topics. Event bytes include `event:`/`data:` framing and the blank line; each
hypothetical gzip size is a separate gzip member, not a sustained stream:

| Event | Count | Raw range B | Gzip-9 range B |
|---|---|---|---|
| hello | 1 | 123–123 | 124–124 |
| system | 5 | 373–374 | 267–271 |
| host | 5 | 3559–3604 | 1060–1095 |
| stats | 2 | 8920–8937 | 2479–2495 |

No container-membership change occurred during this capture, so it does not
measure a `containers` event. A 40-widget/container board and a mature history
can be larger. [Raw responses/event sizes](react-port-raw/m5/json-sizes.log).

**D9 built in M1.** Buffered API JSON from 1,025 through 262,144 B uses
level-1 gzip when accepted and carries `Vary: Accept-Encoding` for either
representation. An explicit `gzip;q=0` selects identity. Both SSE endpoints,
the log download and static assets bypass runtime gzip. Responses with an
existing encoding keep it.

The 256 KiB ceiling keeps the raw compression input at 1.25% of the
20 MiB container budget and bounds compression work on the sole async
thread. It covers the measured dashboard responses below and sends
larger payloads as identity. The gzip output starts with a capacity of
one quarter of the input plus the 18-byte wrapper. The deflater appends
blocks to that vector, so wrapping them does not allocate a second
compressed body.

**[exp]** Ran the static musl x86-64 M1 release binary on loopback port 3300
on an Intel i5-7200U, with a fresh scratch database and an authenticated
Owner. A fake Docker Unix socket served 150 containers across 13 stacks.
SQLite held 241 synthetic system samples at 15-second intervals for a
fixed one-hour history window. The legacy config contained 40 synthetic
bookmarks. A keep-alive client alternated identity and gzip using the same
binary and data to isolate the encoding cost. Each cell reports the median
of five 2,000-request blocks per encoding. CPU is server user plus system
time from `/proc/<pid>/stat` (100 Hz), divided by the request count. Body
sizes exclude HTTP headers. All 80,000 timed requests succeeded.

| Response | Identity B | Gzip request B | Identity CPU µs/request | Gzip request CPU µs/request |
|---|---:|---:|---:|---:|
| `/api/v1/containers` | 61,203 | 4,291 | 2,990 | 3,345 |
| `/api/v1/history`, one hour | 40,593 | 12,741 | 2,940 | 3,565 |
| `/api/v1/settings`, default settings | 22 | 22 | 165 | 165 |
| `/api/settings/config`, 40 bookmarks | 3,372 | 434 | 570 | 690 |

The client decoded gzip and checked it against identity. It also verified
that `gzip;q=0` suppressed compression even with a wildcard and that eligible
responses varied in both cases. The 22 B settings response stayed below the
threshold. These figures cover local request handling and depend on the
payload and host. They exclude client decoding and LAN transfer time.

**M1 review follow-up.** On the same i5-7200U, a Python standard library
client compared the previous and updated static musl x86-64 release binaries
over loopback. Two boards in a scratch database each had 200 links with
seeded random alphanumeric URL suffixes of 980 or 1,300 characters. Four
keep-alive clients repeatedly fetched one board at a time. The client
verified wire lengths, gzip decoding, and that the updated server sent the
oversized board as identity even when offered gzip.

Each row used a fresh server process with a fake Docker Unix socket and a
session created beforehand. After the readers completed their first GETs,
the client made 120 metadata PATCHes to a separate board, with a 25 ms
pause between probes. Latency runs from sending each PATCH to reading the
matching `boards` event on an open SSE connection, including the PATCH and
loopback delivery. Peak RSS is Linux `VmHWM` from `/proc/<pid>/status` at the
end of the run. These are process peaks during dashboard use. Container
memory and overlapping Argon2 work were outside this measurement.

| Build and response mode | Identity / wire B | GETs | Peak RSS (KiB) | SSE p50 / p95 / max (ms) |
|---|---:|---:|---:|---:|
| Updated, idle SSE baseline | — | 0 | 5,120 | 1.31 / 2.90 / 4.78 |
| Updated, near ceiling, identity | 259,664 / 259,664 | 1,907 | 6,348 | 23.58 / 26.94 / 36.20 |
| Previous, near ceiling, gzip | 259,664 / 163,074 | 1,447 | 6,244 | 51.38 / 57.74 / 79.34 |
| Updated, near ceiling, gzip | 259,664 / 163,146 | 1,452 | 6,248 | 50.03 / 58.85 / 64.45 |
| Previous, above ceiling, gzip | 323,665 / 211,429 | 1,436 | 6,560 | 65.32 / 71.51 / 76.05 |
| Updated, above ceiling, identity | 323,665 / 323,665 | 1,804 | 6,372 | 27.38 / 34.69 / 41.24 |

Near the ceiling, gzip saved 37% of wire bytes and p95 SSE latency was
about 32 ms higher than identity during this run.

## Browser performance

**[exp]** `web/e2e/fake-instance.sh start hobnail-rp:m5 3396 --log-fixtures`,
then twice `HB_PASSWORD=<throwaway-password> OUT=<run-dir> scripts/e2e.sh
web/e2e/perf-check.mjs http://127.0.0.1:3396`. Chromium in the container runner;
phone 390×844, touch and 4× CPU; desktop 1440×900, unthrottled; cold browser
HTTP cache, loopback. Per route/profile: one load and three live-update windows
per run. Tracing RunTask and buffered long-task observer are separate methods;
the observer reports only tasks ≥50 ms, so 0 does not mean no work. INP here
is the worst scripted event duration, not field-user INP. Conditions/overlap
are described above. Both runs exited 1 on budget misses.

Budgets: LCP ≤400 ms desktop / ≤1,200 ms phone; INP ≤200 ms; CLS ≤0.1;
150-container phone load tasks ≤200 ms; live-update tasks ≤50 ms. The script
also applies load/live rules to other route families; those diagnostic misses
are retained. Each cell below lists **run 1 / run 2**; live is the maximum of
three updates. Full per-update values and violation lists:
[run 1](react-port-raw/m5/perf1.md), [run 2](react-port-raw/m5/perf2.md).

| Route | Profile | LCP ms | Load max trace ms | Observer max ms | Live max ms | INP ms | CLS |
|---|---|---|---|---|---|---|---|
| `/app/containers` | phone | 2112 / 1524 | 301.4 / 160.5 | 224 / 164 | 52.8 / 67.7 | 160 / 120 | 0.0132 / 0.0132 |
| `/app/containers` | desktop | 836 / 552 | 97.5 / 167.9 | 114 / 118 | 110.5 / 84.4 | 176 / 192 | 0.0596 / 0.0627 |
| `/app/containers/svc002` | phone | 2896 / 1708 | 576 / 429.7 | 576 / 429 | 29.6 / 31.3 | 240 / 168 | 0.002 / 0.002 |
| `/app/containers/svc002` | desktop | 684 / 840 | 122.5 / 107.6 | 122 / 107 | 7.8 / 29.2 | 32 / 72 | 0.0194 / 0.0225 |
| `/app/overview` | phone | 1760 / 1440 | 160.4 / 127 | 266 / 220 | 60.0 / 65.6 | 104 / 128 | 0.002 / 0.002 |
| `/app/overview` | desktop | 1228 / 812 | 114.9 / 181.6 | 122 / 181 | 40.3 / 16.7 | 64 / 24 | 0.0213 / 0.0247 |
| `/app/history` | phone | 916 / 1540 | 482.4 / 714.5 | 481 / 714 | 261.3 / 110.3 | 128 / 168 | 0.0047 / 0.0047 |
| `/app/history` | desktop | 604 / 516 | 170.5 / 123.2 | 170 / 123 | 96.4 / 43.3 | 72 / 88 | 0.026 / 0.0023 |
| `/app/host` | phone | 1412 / 1496 | 164 / 194.1 | 164 / 194 | 22.0 / 24.8 | 64 / 80 | 0.002 / 0.002 |
| `/app/host` | desktop | 432 / 568 | 122.6 / 119.5 | 122 / 119 | 18.7 / 21.4 | 48 / 48 | 0.0782 / 0.0782 |
| `/app/boards/<id>` | phone | 1680 / 1624 | 128 / 139.3 | 356 / 238 | 43.4 / 27.4 | 136 / 64 | 0.0368 / 0.0388 |
| `/app/boards/<id>` | desktop | 512 / 732 | 53.4 / 49.2 | 69 / 72 | 19.1 / 11.3 | 128 / 80 | 0.0002 / 0.0002 |
| `/app/logs` | phone | 3024 / 2044 | 309.3 / 259.7 | 354 / 258 | 40.8 / 38.7 | 168 / 192 | 0.0032 / 0.0032 |
| `/app/logs` | desktop | 1192 / 896 | 62.1 / 60.1 | 87 / 69 | 18.1 / 18.9 | 104 / 80 | 0.033 / 0.0331 |
| `/app/settings/general` | phone | 1828 / 1740 | 233.6 / 110.7 | 250 / 203 | 58.1 / 30.5 | 128 / 72 | 0.002 / 0.002 |
| `/app/settings/general` | desktop | 828 / 756 | 94 / 34.9 | 94 / 54 | 28.9 / 19.8 | 96 / 48 | 0.0224 / 0.0224 |

LCP misses affect all desktop route/profile cases and most phone cases; see
raw rows for exact exceptions. First chart mount reached **576 / 429.7 ms**
on phone. History phone load reached **482.4 / 714.5 ms**, with live tasks up
to **261.3 / 110.3 ms**. Phone table load was **301.4 / 160.5 ms** by tracing
(observer **224 / 164 ms**). Table live updates exceeded 50 ms in both runs.
INP missed once at **240 ms** (chart phone, run 1); CLS stayed ≤0.1 in these
runs. Route-change latency (≤200/500 ms) was not separately timed.

## Browser checks and logs

**[exp]** Fresh fake instance at 3398 with `--log-fixtures`, same image;
`HB_PASSWORD=<throwaway-password> E2E_ENV=LOG_FIXTURES LOG_FIXTURES=1 OUT=<dir>
scripts/e2e.sh web/e2e/<check>-check.mjs http://127.0.0.1:3398`.
Slice adds `svc001 svc002`. Checks ran sequentially; perf/memory could overlap.
Counts use each harness's own reporting unit, not a fabricated unified count.
Screenshots were retained in scratch only; the phone light Host and desktop
light board-editor captures were visually inspected, not every generated shot.

| Check | Passed / failed | Unit and evidence |
|---|---|---|
| app-check | 69 / 0 | logged assertions; [log](react-port-raw/m5/app-check.log) |
| slice-check | 166 / 0 | logged assertions; [log](react-port-raw/m5/slice-check.log) |
| board-check | 97 / 0 | assertions; [result](react-port-raw/m5/board-result.json) |
| pages-check | 4 / 0 | width/theme profiles, each traverses read routes; [log](react-port-raw/m5/pages-check.log) |
| logs-check | 3 / 1 | width/theme profiles; [result](react-port-raw/m5/logs-results.json) |
| settings-check | 4 / 0 | width/theme profiles; [log](react-port-raw/m5/settings-check.log) |
| a11y-check | 60 / 0 | audits, zero violations; [log](react-port-raw/m5/a11y-check.log) |
| csp-check | 80 / 0 | route visits, zero failed profiles; [log](react-port-raw/m5/csp-check.log) |

Settings, accessibility and CSP harness summaries:

```text
settings:
ok 1440-dark: Settings tabs, Account, Users and signed-in editing
ok 1440-light: Settings tabs, Account, Users and signed-in editing
ok 390-dark: Settings tabs, Account, Users and signed-in editing
ok 390-light: Settings tabs, Account, Users and signed-in editing
e2e: exit 0; output dir /tmp/claude-1000/-home-dev-projects-rust-homepage/b1d6dc1b-01ae-438b-8462-73d11eda49a6/scratchpad/m5/check-settings
a11y:
ok 390 /app/settings/security: 26 controls, 0 violations
ok 390 /app/settings/boards: 28 controls, 0 violations
ok 390 /app/settings/integrations: 22 controls, 0 violations
ok 390 /app/settings/users: 32 controls, 0 violations
ok 390 /app/account: 16 controls, 0 violations
ok 390 /app/login: 3 controls, 0 violations
60 audits; 0 violations
e2e: exit 0; output dir /tmp/claude-1000/-home-dev-projects-rust-homepage/b1d6dc1b-01ae-438b-8462-73d11eda49a6/scratchpad/m5/check-a11y
csp:
ok 390 /app/settings/notifications?add=rule
ok 390 /app/settings/boards
ok 390 /app/settings/integrations
ok 390 /app/settings/users
ok 390 /app/boards/206577df0aeac6c37a45f2ea2b83a213
ok 390 /app/login
80 route visits; 0 failed profiles
e2e: exit 0; output dir /tmp/claude-1000/-home-dev-projects-rust-homepage/b1d6dc1b-01ae-438b-8462-73d11eda49a6/scratchpad/m5/check-csp
```

Logs measurements (unthrottled desktop/phone viewport; not the perf-check 4× profile):

| Profile | Single burst max task ms | Four-pane burst max task ms | Close/zero-stream check ms | 5,000-line input-to-result ms | Matching work ms |
|---|---|---|---|---|---|
| 1440-dark | 0 | 0 | 2719 | 343.2999997138977 | 1.8 |
| 1440-light | 0 | 0 | 2642 | 284.09999990463257 | 6.7 |
| 390-dark | 107 | 81 | 3244 | 756.8999996185303 | 8.8 |
| 390-light | 59 | 56 | 3115 | 591.3000001907349 | 7.5 |

The 390-dark single-pane **107 ms** burst misses ≤100 ms; logs-check therefore
exits 1, despite all four profiles reaching the functional end. An observer
value 0 means no ≥50 ms task was observed, not zero CPU. Close time includes
the harness wait/poll and checking zero viewer streams, not a precise socket
teardown timestamp. Search input-to-result and the pane's matching-work timer
measure different intervals. Every log profile recorded zero console errors
and CSP violations. No performance fixes were made merely to change the report.

## Tests and dependency audit

**[exp]** Required commands all passed: `scripts/cargo.sh fmt --check`,
`scripts/cargo.sh clippy --locked --all-targets -- -D warnings`,
`scripts/cargo.sh test --locked` (**382 tests**, including API-reference drift);
`scripts/web.sh run check`, `run test` (**140 tests**), `run build`, `run size`.
The Docker image build passed the same gates. See
[Rust tests](react-port-raw/m5/rust-test.log), [clippy](react-port-raw/m5/clippy.log),
[client tests](react-port-raw/m5/web-test.log), [client check](react-port-raw/m5/web-check.log).

`scripts/web.sh audit --json`: **0 vulnerabilities** (0 low/moderate/high/critical),
130 locked dependencies counted. The old phase 1 picomatch findings were
build-time-only historical findings; there are none in this audit. This is an
npm audit result, not an application security assessment.
[Raw audit](react-port-raw/m5/audit.json).

`scripts/web.sh node scripts/ages.mjs`: **130 packages; 0 non-exception packages
younger than seven days**. Youngest is immer 11.2.10, published
2026-11-10T14:44:08.531Z, audit-displayed age **8.6 d**. The sole exceptions are
`@ui-sdk/react` and `@ui-sdk/core`, both pinned exactly **0.1.0-beta.10**,
published 2026-11-18 at **09:45:28.252Z / 09:47:47.539Z** respectively
(audit-displayed age 0.8 d). `min-release-age=7`, `ignore-scripts=true` remain.
No crates were added here; the Argon2 addition's age audit is recorded in
commit `1c2e647` (all added crates older than seven days).
[Raw age audit](react-port-raw/m5/ages.log).

## Deviations from the plan and the locked decisions

From code and `git log 1bd6f10..HEAD` **[code]**, with measured consequences **[exp]**:

1. Whole-dist cap raised first to 420 KB and then **450 KB gzip / 1.35 MB raw**
   (`a5c0de5`, `ef54589`); accepted by the owner on 2026-11-20. Original caps
   are missed. Chart chunks are split and individually capped at 60 KB; their
   aggregate named payload is 115,235 B gzip, above a combined 110 KB allowance.
2. The **13.3 MB** M5 image exceeded the original 13 MB after-removal
   budget. The owner accepted it and set a 13.5 MB gate. Lifetime memory
   peaks including login exceed **20 MiB**.
3. Browser LCP, load-task/live-task, one INP and log-burst budgets are missed,
   as recorded above; they are not rounded into passes. Full chart first mount
   remains a long task. CLS passes these two runs.
4. `/` returns **404 with a link to `/app`**, following the owner's no-redirect
   rule. No old-URL compatibility or RH_UI=classic exists; `rust-only` retains
   the old UI. This differs from the earlier plan's redirect/fallback wording.
5. Board widget tiles render **structured widget bodies**, not server HTML;
   existing widget IDs and generic HTTP integration tiles are supported, not
   an unbuilt provider/widget catalog (`98707d5`, board/API code).
6. The scoped accounts model permits container visibility only through granted
   boards; host-wide pages are refused. Scoped Admins edit granted boards and
   integrations but cannot create/delete entities or alter global Settings.
7. Databases with a legacy shared password set an **upgrade lock**; their old
   hash/sessions are discarded and host `set-password [username]` recovery is
   required (`b6686f8`). Fresh installs use browser owner setup, not that CLI.
8. M1 adds **Secure** to session cookies when a configured trusted proxy
   reports HTTPS. Plain HTTP keeps working. For the per-IP login limit, the
   same trust list enables rightmost-untrusted `X-Forwarded-For` handling.
   A single Argon2id permit preserves the 19 MiB workspace peak. Up to eight
   requests wait asynchronously for up to five seconds, enough to absorb
   short bursts while bounding retained credential data and request latency.
   The active operation runs on a blocking worker. Queue overflow and
   timeouts return 429 with `Retry-After: 1`; the IP and global back-offs
   are checked again when a queued login reaches the worker.
9. Credentials are bound to **origin, not path**. The database stores plaintext
   tokens with their binding; no token is returned. Tests stop at **any redirect**
   rather than following even same-origin redirects. Public roots only: no
   self-signed certificates or fingerprint pinning (D21 step b not built).
10. M2 adds the five-step owner setup wizard, resumable SQLite drafts, and
    an idempotent commit that opens a first board containing the owner's
    selections. Empty boards offer Add and Discover. The first-board tour is
    skippable, saved per account, and replayable. Service-specific providers
    and the provider catalog remain deferred (D19). Integration icons remain
    opt-in dashboard-icons.
11. UI SDK is installed under the owner's explicit D2 age exception, but its
    project-local skill was **not installed**: skills CLI 1.9.0, published
    2026-11-16, was younger than seven days (`web/AGENTS.md`). Other npm packages
    and new Rust crates retain the seven-day rule; no new exception was made.
12. `scripts/deploy.sh` now requires confirmation; unattended use needs
    **DEPLOY_YES=1**. It was not run. No live data was accessed.
13. M5 added the requested API reference drift test and its Docker context input.
    The memory asset graph required two isolated script fixes; the original
    run and the stopped intermediate run are not passed off as full workloads.
14. M2 closes SET-42 for legacy TOML collection arrows with staged moves
    and explicit Save order and Cancel. One Save writes the complete order
    in one revision. Widget tiles and manual HTTP integration retests now
    show retained results with an aged stale badge and the latest error. An
    initial failure displays only the error, and recovery clears the badge.

## Budgets still missed

- Original whole-dist ceilings of 1.3 MB raw and 400 KB gzip. The M5 build
  is 1,349,132 B raw and 438,069 B gzip. The configured size checks pass.
- Preferred lifetime memory under 10 MiB. M5 peaked at 12.45 MiB with full history.
- Uncapped cold SQLite database peak under 20 MiB. The cold probe reached
  187,559,936 B (178.87 MiB), primarily charged file cache.
- First sign-in with a legacy hash exceeded 20 MiB in the migration probe.
  Fresh-hash workload results are in the M5 table above.
- Unpacked release image ceiling of 13.5 MB. M5 is 15,147,903 B, compared
  with 15,135,612 B at the pre-M5 baseline.
- LCP ≤400/1,200 ms on the failing route/profile rows; phone table load ≤200 ms
  in run 1; first chart/History mounts also exceed that diagnostic threshold.
- Live-update ≤50 ms on table and several other routes; INP ≤200 ms on chart
  phone run 1 (240 ms).
- Log burst ≤100 ms on phone dark (107 ms).

Passing checks at raised caps do not constitute the owner's acceptance of
these deviations. [QA outcomes](../qa/QA.md#react-port-outcomes-of-the-deferred-findings)
state what actually changed for the fourteen deferred findings.

## Not verified

Firefox/Safari, aarch64, cold-cache application memory, real screen readers,
a full visual review of every screenshot, real mobile devices, a 400 kbit/s
first visit, independently timed in-app route-change budgets, 30/90-day mature
history sizes, real service providers, certificate pinning, native systemd
packaging and concurrent-login throughput behind a real reverse proxy.
The measurements use short-lived/fake-container data for browser/JSON tests;
they are not long-running production guarantees. All throwaway containers and
fresh data directories created here were removed after completion.
