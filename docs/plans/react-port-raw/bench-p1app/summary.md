### Memory benchmark: `hobnail-p1:rc` (sha256:4815e3001143), profile app, warm cache

2026-11-18T02:03:23Z; config seed-config.toml (9555f080f7d3); host 4 CPUs, 7.2 GiB, kernel 7.0.0-38-generic, Docker 29.5.2; start -> first /healthz 200: 381 ms

| Phase | Samples | `memory.current` median / max | Working set median / max | `memory.peak` at end | anon / file / kernel / sock median | CPU % of one core | Requests / errors / events | p50 / p99 ms |
|---|---|---|---|---|---|---|---|---|
| startup | 100 | 1.61 / 2.33 | 1.38 / 2.10 | 4.31 | 0.83 / 0.21 / 0.44 / 0.00 | 0.45 | – | – |
| idle | 180 | 2.03 / 3.04 | 1.39 / 2.34 | 4.56 | 0.95 / 0.59 / 0.43 / 0.00 | 0.35 | – | – |
| clients-1 | 120 | 2.80 / 3.41 | 2.02 / 2.62 | 4.61 | 1.11 / 0.78 / 0.47 / 0.00 | 0.82 | 55 / 0 / 33 | 5.0 / 15.5 |
| clients-10 | 120 | 3.45 / 3.90 | 2.54 / 2.97 | 5.67 | 1.42 / 0.91 / 0.52 / 0.00 | 3.83 | 556 / 0 / 330 | 5.4 / 15.5 |
| clients-25 | 120 | 4.11 / 4.55 | 3.00 / 3.39 | 5.82 | 1.83 / 1.08 / 0.58 / 0.00 | 8.37 | 1375 / 0 / 825 | 5.0 / 17.4 |
| after | 120 | 2.85 / 3.61 | 1.57 / 2.43 | 6.29 | 1.11 / 1.25 / 0.46 / 0.00 | 0.34 | – | – |

Process RSS at phase ends (MiB, from /proc/<pid>/status):

| Phase | RssAnon | RssFile | RssShmem | VmRSS |
|---|---|---|---|---|
| startup | 0.95 | 4.37 | 0.00 | 5.31 |
| idle | 0.96 | 4.37 | 0.00 | 5.33 |
| clients-1 | 1.12 | 4.93 | 0.00 | 6.05 |
| clients-10 | 1.23 | 4.93 | 0.00 | 6.16 |
| clients-25 | 1.24 | 4.93 | 0.00 | 6.17 |
| after | 1.08 | 4.93 | 0.00 | 6.01 |

Endpoints: stream `/api/v1/stream?topics=containers,stats,system`, state `/api/v1/containers`, history `/api/history/system?metric=cpu&range=1h`, assets 4: `/app`, `/assets/index-CJbnETpO.css`, `/assets/index-nBXt6Jtp.js`, `/assets/jsx-runtime-Wy9s6hX0.js`

Raw samples: <scratch>/bench-p1app/samples.csv; RSS: <scratch>/bench-p1app/rss.csv; clients: <scratch>/bench-p1app/clients/
