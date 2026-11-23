# perf-check 2026-11-22T09:33:30.438Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 3076.6 | 150 | 772 | 1280 | 0.0087 | 2056 | 189.7 / 4 / 1777.5 | 4 (max 185) | 27.6, 18.5, 21.8 | 136 / 91.6 | app phone: LCP 1280 ms (> 1200) |
| app | desktop | 1508.3 | 150 | 328 | 472 | 0.0222 | 3547 | 77.7 / 4 / 1152.4 | 5 (max 77) | 45.2, 72.4, 41.1 | 120 / 70.3 | app desktop: LCP 472 ms (> 400); app desktop: live update 2: a task took 72.4 ms (> 50 ms) |
| history | phone | 1409.5 | 8 | 676 | 972 | 0.0059 | 668 | 238.7 / 4 / 1174.2 | 6 (max 238) | 64.4, 39, 50.6 | 32 / 68.6 | history phone: trace load task 238.7 ms (> 200); history phone: observer load task 238 ms (> 200); history phone: live update 1: a task took 64.4 ms (> 50 ms); history phone: live update 3: a task took 50.6 ms (> 50 ms) |
| history | desktop | 518.4 | 8 | 280 | 416 | 0.0026 | 2112 | 57.1 / 1 / 596.5 | 1 (max 57) | 22.2, 36.4, 14.4 | 16 / 39.6 | history desktop: LCP 416 ms (> 400) |

Budget violations:
- app phone: LCP 1280 ms (> 1200)
- app desktop: LCP 472 ms (> 400)
- app desktop: live update 2: a task took 72.4 ms (> 50 ms)
- history phone: trace load task 238.7 ms (> 200)
- history phone: observer load task 238 ms (> 200)
- history phone: live update 1: a task took 64.4 ms (> 50 ms)
- history phone: live update 3: a task took 50.6 ms (> 50 ms)
- history desktop: LCP 416 ms (> 400)
