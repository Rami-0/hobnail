### Memory benchmark: `hobnail-p1:rc` (sha256:4815e3001143), profile html, warm cache

2026-11-18T01:51:17Z; config seed-config.toml (9555f080f7d3); host 4 CPUs, 7.2 GiB, kernel 7.0.0-38-generic, Docker 29.5.2; start -> first /healthz 200: 419 ms

| Phase | Samples | `memory.current` median / max | Working set median / max | `memory.peak` at end | anon / file / kernel / sock median | CPU % of one core | Requests / errors / events | p50 / p99 ms |
|---|---|---|---|---|---|---|---|---|
| startup | 100 | 1.59 / 3.88 | 1.36 / 3.67 | 4.38 | 0.81 / 0.21 / 0.43 / 0.00 | 0.48 | – | – |
| idle | 180 | 2.00 / 4.76 | 1.34 / 4.20 | 5.01 | 0.92 / 0.59 / 0.42 / 0.00 | 0.47 | – | – |
| clients-1 | 120 | 2.75 / 3.34 | 1.99 / 2.56 | 5.01 | 1.15 / 0.78 / 0.44 / 0.00 | 1.02 | 52 / 0 / 48 | 4.4 / 11.6 |
| clients-10 | 120 | 3.52 / 4.10 | 2.59 / 3.22 | 5.66 | 1.51 / 0.91 / 0.52 / 0.00 | 6.08 | 520 / 0 / 459 | 9.7 / 21.5 |
| clients-25 | 120 | 4.18 / 5.12 | 3.09 / 4.07 | 6.16 | 1.91 / 1.08 / 0.58 / 0.00 | 13.02 | 1300 / 0 / 1189 | 5.8 / 39.5 |
| after | 120 | 2.87 / 3.58 | 1.59 / 2.29 | 6.16 | 1.13 / 1.25 / 0.46 / 0.00 | 0.25 | – | – |

Process RSS at phase ends (MiB, from /proc/<pid>/status):

| Phase | RssAnon | RssFile | RssShmem | VmRSS |
|---|---|---|---|---|
| startup | 0.93 | 4.42 | 0.00 | 5.35 |
| idle | 0.95 | 4.42 | 0.00 | 5.37 |
| clients-1 | 1.12 | 5.17 | 0.00 | 6.30 |
| clients-10 | 1.24 | 5.17 | 0.00 | 6.41 |
| clients-25 | 1.29 | 5.17 | 0.00 | 6.46 |
| after | 1.11 | 5.17 | 0.00 | 6.28 |

Endpoints: stream `/api/stream`, state `/api/state`, history `/api/history/system?metric=cpu&range=1h`, assets 3: `/`, `/static/app.css?v=ca28fb2fe08b584c`, `/static/app.js?v=89a72b6eb6f47863`

Raw samples: <scratch>/bench-p1html/samples.csv; RSS: <scratch>/bench-p1html/rss.csv; clients: <scratch>/bench-p1html/clients/
