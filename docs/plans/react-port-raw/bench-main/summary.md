### Memory benchmark: `hobnail-p0-main:680a339` (sha256:db421e3bd503), profile html, warm cache

2026-11-18T01:45:33Z; config seed-config.toml (9555f080f7d3); host 4 CPUs, 7.2 GiB, kernel 7.0.0-38-generic, Docker 29.5.2; start -> first /healthz 200: 440 ms

| Phase | Samples | `memory.current` median / max | Working set median / max | `memory.peak` at end | anon / file / kernel / sock median | CPU % of one core | Requests / errors / events | p50 / p99 ms |
|---|---|---|---|---|---|---|---|---|
| startup | 100 | 3.81 / 4.23 | 3.56 / 4.02 | 6.10 | 0.85 / 2.44 / 0.44 / 0.00 | 0.45 | – | – |
| idle | 180 | 4.22 / 5.00 | 3.56 / 4.42 | 6.69 | 0.91 / 2.82 / 0.42 / 0.00 | 0.35 | – | – |
| clients-1 | 120 | 5.38 / 6.01 | 4.59 / 5.19 | 7.86 | 1.15 / 3.45 / 0.45 / 0.00 | 1.14 | 52 / 0 / 48 | 5.2 / 39.2 |
| clients-10 | 120 | 6.18 / 6.55 | 5.25 / 5.59 | 8.07 | 1.48 / 3.57 / 0.52 / 0.00 | 6.03 | 520 / 0 / 449 | 9.5 / 22.6 |
| clients-25 | 120 | 6.85 / 7.51 | 5.72 / 6.45 | 8.84 | 1.84 / 3.75 / 0.59 / 0.00 | 13.28 | 1300 / 0 / 1206 | 6.0 / 30.6 |
| after | 120 | 5.55 / 6.45 | 4.25 / 5.10 | 8.84 | 1.13 / 3.93 / 0.46 / 0.00 | 0.41 | – | – |

Process RSS at phase ends (MiB, from /proc/<pid>/status):

| Phase | RssAnon | RssFile | RssShmem | VmRSS |
|---|---|---|---|---|
| startup | 0.92 | 4.03 | 0.00 | 4.95 |
| idle | 0.94 | 4.03 | 0.00 | 4.97 |
| clients-1 | 1.14 | 4.74 | 0.00 | 5.88 |
| clients-10 | 1.22 | 4.74 | 0.00 | 5.96 |
| clients-25 | 1.28 | 4.77 | 0.00 | 6.05 |
| after | 1.11 | 4.77 | 0.00 | 5.88 |

Endpoints: stream `/api/stream`, state `/api/state`, history `/api/history/system?metric=cpu&range=1h`, assets 3: `/`, `/static/app.css?v=ca28fb2fe08b584c`, `/static/app.js?v=89a72b6eb6f47863`

Raw samples: <scratch>/bench-main/samples.csv; RSS: <scratch>/bench-main/rss.csv; clients: <scratch>/bench-main/clients/
