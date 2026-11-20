### Memory benchmark: `hobnail-rp:m5` (sha256:14897f701f99), profile app, warm cache

2026-11-19T05:08:54Z; config none (empty); host 4 CPUs, 7.2 GiB, kernel 7.0.0-38-generic, Docker 29.5.2; start -> first /healthz 200: 391 ms

| Phase | Samples | `memory.current` median / max | Working set median / max | `memory.peak` at end | anon / file / kernel / sock median | CPU % of one core | Requests / errors / events | p50 / p99 ms |
|---|---|---|---|---|---|---|---|---|
| startup | 100 | 1.76 / 2.20 | 1.41 / 1.82 | 4.50 | 1.00 / 0.35 / 0.42 / 0.00 | 0.56 | – | – |
| idle | 180 | 2.02 / 3.86 | 1.49 / 3.39 | 4.50 | 1.02 / 0.53 / 0.42 / 0.00 | 0.29 | – | – |
| clients-1 | 120 | 2.60 / 3.45 | 1.78 / 2.57 | 21.85 | 1.15 / 0.83 / 0.50 / 0.00 | 0.63 | 91 / 0 / 39 | 2.2 / 32.1 |
| clients-10 | 120 | 3.42 / 3.93 | 2.32 / 2.83 | 22.39 | 1.36 / 1.11 / 0.55 / 0.00 | 2.35 | 928 / 0 / 382 | 2.2 / 27.6 |
| clients-25 | 120 | 4.35 / 5.91 | 2.93 / 4.52 | 22.71 | 1.77 / 1.47 / 0.61 / 0.00 | 4.61 | 2275 / 0 / 877 | 1.9 / 14.4 |
| after | 120 | 3.36 / 5.04 | 1.64 / 3.38 | 22.71 | 1.15 / 1.77 / 0.46 / 0.00 | 0.25 | – | – |

Process RSS at phase ends (MiB, from /proc/<pid>/status):

| Phase | RssAnon | RssFile | RssShmem | VmRSS |
|---|---|---|---|---|
| startup | 0.99 | 3.96 | 0.00 | 4.95 |
| idle | 0.99 | 4.13 | 0.00 | 5.12 |
| clients-1 | 1.16 | 5.12 | 0.00 | 6.28 |
| clients-10 | 1.18 | 5.12 | 0.00 | 6.30 |
| clients-25 | 1.20 | 5.12 | 0.00 | 6.33 |
| after | 1.07 | 5.12 | 0.00 | 6.20 |

Endpoints: stream `/api/v1/stream?topics=containers,stats,system`, state `/api/v1/containers`, history `/api/history/system?metric=cpu&range=1h`, assets 16: `/app`, `/assets/Button-DwrunF04.js`, `/assets/app-runtime-DLsVIt_d.js`, `/assets/button-dp3fTQDb.js`, `/assets/createLucideIcon-CRbusLLJ.js`, `/assets/events-BCX97W3_.js`, `/assets/index-0Y5ftNI3.js`, `/assets/layers-B7tDtED_.js`, `/assets/preload-helper-Dp5Bp1yV.js`, `/assets/react-CdlCbvX4.js`, `/assets/rolldown-runtime-CNOWq4Kh.js`, `/assets/session-7ttDCFor.js`, `/assets/triangle-exclamation-point-Bxrvyj48.js`, `/assets/use-primitive-BWYySHcm.js`, `/assets/web-DupGL4Wj.css`, `/assets/web-fgg0mksI.js`

Raw samples: /tmp/claude-1000/-home-dev-projects-rust-homepage/b1d6dc1b-01ae-438b-8462-73d11eda49a6/scratchpad/m5/bench/samples.csv; RSS: /tmp/claude-1000/-home-dev-projects-rust-homepage/b1d6dc1b-01ae-438b-8462-73d11eda49a6/scratchpad/m5/bench/rss.csv; clients: /tmp/claude-1000/-home-dev-projects-rust-homepage/b1d6dc1b-01ae-438b-8462-73d11eda49a6/scratchpad/m5/bench/clients/
