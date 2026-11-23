# perf-check 2026-11-23T09:17:17.298Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 3102.7 | 150 | 916 | 1456 | 0.0091 | 2021 | 194 / 3 / 1604.2 | 5 (max 229) | 34.2, 25.7, 22.7 | 128 / 111.5 | app phone: observer load task 229 ms (> 200); app phone: LCP 1456 ms (> 1200) |
| app | desktop | 1448 | 150 | 328 | 492 | 0.0177 | 3469 | 137.3 / 3 / 1122.6 | 4 (max 137) | 47.5, 58.6, 40.5 | 144 / 65.7 | app desktop: LCP 492 ms (> 400); app desktop: live update 2: a task took 58.6 ms (> 50 ms) |
| history | phone | 1591.1 | 8 | 680 | 980 | 0.0059 | 673 | 253.3 / 5 / 1410.8 | 7 (max 252) | 62.4, 55.4, 57.8 | 120 / 71.8 | history phone: trace load task 253.3 ms (> 200); history phone: observer load task 252 ms (> 200); history phone: live update 1: a task took 62.4 ms (> 50 ms); history phone: live update 2: a task took 55.4 ms (> 50 ms); history phone: live update 3: a task took 57.8 ms (> 50 ms) |
| history | desktop | 731 | 8 | 432 | 584 | 0.0025 | 2132 | 54.9 / 1 / 697.6 | 2 (max 71) | 16.8, 18, 20.9 | - / 29.6 | history desktop: LCP 584 ms (> 400); history desktop: missing INP measurement |

Budget violations:
- app phone: observer load task 229 ms (> 200)
- app phone: LCP 1456 ms (> 1200)
- app desktop: LCP 492 ms (> 400)
- app desktop: live update 2: a task took 58.6 ms (> 50 ms)
- history phone: trace load task 253.3 ms (> 200)
- history phone: observer load task 252 ms (> 200)
- history phone: live update 1: a task took 62.4 ms (> 50 ms)
- history phone: live update 2: a task took 55.4 ms (> 50 ms)
- history phone: live update 3: a task took 57.8 ms (> 50 ms)
- history desktop: LCP 584 ms (> 400)
- history desktop: missing INP measurement
