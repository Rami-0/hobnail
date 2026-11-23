# perf-check 2026-11-22T03:06:39.910Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 5499.5 | 150 | 2392 | 2392 | 0.0087 | 2023 | 319 / 4 / 2688.2 | 9 (max 318) | 32, 27.8, 17.1 | 120 / 89 | app phone: trace load task 319 ms (> 200); app phone: observer load task 318 ms (> 200); app phone: LCP 2392 ms (> 1200) |
| app | desktop | 1691.2 | 150 | 456 | 456 | 0.0429 | 3458 | 139.5 / 3 / 1904 | 1 (max 88) | 45.3, 63.6, 48.5 | 128 / 62.3 | app desktop: LCP 456 ms (> 400); app desktop: live update 2: a task took 63.6 ms (> 50 ms) |
| chart | phone | 2266.1 | 2 | 1244 | 1524 | 0.002 | 535 | 396.6 / 2 / 820.6 | 6 (max 396) | 15.2, 11.7, 12.1 | 160 / 91.2 | chart phone: trace load task 396.6 ms (> 200); chart phone: observer load task 396 ms (> 200); chart phone: LCP 1524 ms (> 1200) |
| chart | desktop | 985.9 | 2 | 532 | 532 | 0.0019 | 1839 | 102.8 / 1 / 412.1 | 2 (max 102) | 4.9, 13.7, 4.7 | 24 / 50 | chart desktop: LCP 532 ms (> 400) |
| overview | phone | - | - | - | - | - | - | - | - | - | - / - | run crashed: Error: no visible element .overview-strips summary
    at Object.click (file:///repo/web/e2e/cdp.mjs:124:21)
    at async run (file:///repo/web/e2e/perf-check.mjs:252:7)
    at async file:///repo/web/e2e/perf-check.mjs:379:76; overview phone: run crashed: Error: no visible element .over |
| overview | desktop | - | - | - | - | - | - | - | - | - | - / - | run crashed: Error: no visible element .overview-strips summary
    at Object.click (file:///repo/web/e2e/cdp.mjs:124:21)
    at async run (file:///repo/web/e2e/perf-check.mjs:252:7)
    at async file:///repo/web/e2e/perf-check.mjs:379:76; overview desktop: run crashed: Error: no visible element .ov |
| history | phone | 2430.2 | 8 | 932 | 932 | 0.0077 | 668 | 341 / 4 / 1191.6 | 5 (max 340) | 111.7, 116, 106.3 | 376 / 147.3 | history phone: trace load task 341 ms (> 200); history phone: observer load task 340 ms (> 200); history phone: INP 376 ms (> 200); history phone: live update 1: a task took 111.7 ms (> 50 ms); history phone: live update 2: a task took 116 ms (> 50 ms); history phone: live update 3: a task took 106. |
| history | desktop | 1196.1 | 8 | 396 | 396 | 0.0351 | 2114 | 78 / 1 / 636.1 | 2 (max 78) | 42.6, 38, 66.9 | 32 / 29.8 | history desktop: live update 3: a task took 66.9 ms (> 50 ms) |
| host | phone | 1113.2 | 1 | 868 | 1224 | 0.002 | 523 | 118 / 4 / 1057.6 | 3 (max 174) | 21.8, 19.2, 28 | 64 / 22.1 | host phone: LCP 1224 ms (> 1200) |
| host | desktop | 356.5 | 1 | 296 | 488 | 0.0493 | 1827 | 40.5 / 0 / 596.3 | 0 (max 0) | 11.3, 17.7, 19.1 | 48 / 6.1 | host desktop: LCP 488 ms (> 400) |
| board | phone | 1411.7 | 40 | 1236 | 1748 | 0.0555 | 815 | 134.1 / 4 / 658.2 | 5 (max 292) | 28.5, 29, 45.5 | 72 / 303.6 | board phone: observer load task 292 ms (> 200); board phone: LCP 1748 ms (> 1200) |
| board | desktop | 518.6 | 40 | 464 | 612 | 0.028 | 1808 | 34.5 / 0 / 586.9 | 1 (max 65) | 15, 8.7, 32.6 | 88 / 60.4 | board desktop: LCP 612 ms (> 400) |
| logs | phone | 1605.1 | 13 | 1088 | 1832 | 0.0118 | 347 | 63.7 / 1 / 443.3 | 5 (max 229) | 34.1, 30.8, 25.2 | 120 / 79.1 | logs phone: observer load task 229 ms (> 200); logs phone: LCP 1832 ms (> 1200) |
| logs | desktop | 566.4 | 21 | 372 | 684 | 0.0387 | 1687 | 59.6 / 1 / 355.1 | 1 (max 54) | 34.4, 31, 37.7 | 88 / 52.5 | logs desktop: LCP 684 ms (> 400) |
| settings | phone | 1296.9 | 0 | 1024 | 1368 | 0.0057 | 342 | 116.3 / 3 / 631.3 | 4 (max 230) | 37.3, 27.1, 23.4 | 72 / 38.5 | settings phone: observer load task 230 ms (> 200); settings phone: LCP 1368 ms (> 1200) |
| settings | desktop | 475.4 | 0 | 404 | 536 | 0.1032 | 1646 | 40.8 / 0 / 472.3 | 1 (max 55) | 13.1, 9.6, 9.9 | 56 / 37 | settings desktop: LCP 536 ms (> 400); settings desktop: CLS 0.1032 (> 0.1) |

Budget violations:
- app phone: trace load task 319 ms (> 200)
- app phone: observer load task 318 ms (> 200)
- app phone: LCP 2392 ms (> 1200)
- app desktop: LCP 456 ms (> 400)
- app desktop: live update 2: a task took 63.6 ms (> 50 ms)
- chart phone: trace load task 396.6 ms (> 200)
- chart phone: observer load task 396 ms (> 200)
- chart phone: LCP 1524 ms (> 1200)
- chart desktop: LCP 532 ms (> 400)
- overview phone: run crashed: Error: no visible element .overview-strips summary
    at Object.click (file:///repo/web/e2e/cdp.mjs:124:21)
    at async run (file:///repo/web/e2e/perf-check.mjs:252:7)
    at async file:///repo/web/e2e/perf-check.mjs:379:76
- overview desktop: run crashed: Error: no visible element .overview-strips summary
    at Object.click (file:///repo/web/e2e/cdp.mjs:124:21)
    at async run (file:///repo/web/e2e/perf-check.mjs:252:7)
    at async file:///repo/web/e2e/perf-check.mjs:379:76
- history phone: trace load task 341 ms (> 200)
- history phone: observer load task 340 ms (> 200)
- history phone: INP 376 ms (> 200)
- history phone: live update 1: a task took 111.7 ms (> 50 ms)
- history phone: live update 2: a task took 116 ms (> 50 ms)
- history phone: live update 3: a task took 106.3 ms (> 50 ms)
- history desktop: live update 3: a task took 66.9 ms (> 50 ms)
- host phone: LCP 1224 ms (> 1200)
- host desktop: LCP 488 ms (> 400)
- board phone: observer load task 292 ms (> 200)
- board phone: LCP 1748 ms (> 1200)
- board desktop: LCP 612 ms (> 400)
- logs phone: observer load task 229 ms (> 200)
- logs phone: LCP 1832 ms (> 1200)
- logs desktop: LCP 684 ms (> 400)
- settings phone: observer load task 230 ms (> 200)
- settings phone: LCP 1368 ms (> 1200)
- settings desktop: LCP 536 ms (> 400)
- settings desktop: CLS 0.1032 (> 0.1)
