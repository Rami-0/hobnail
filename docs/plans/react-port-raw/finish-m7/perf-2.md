# perf-check 2026-11-23T09:18:27.882Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 2878.8 | 150 | 760 | 1196 | 0.0091 | 2057 | 159.5 / 3 / 1693.3 | 4 (max 207) | 28.1, 18.8, 24.7 | 128 / 86.9 | app phone: observer load task 207 ms (> 200) |
| app | desktop | 1382.8 | 150 | 352 | 496 | 0.0177 | 3561 | 74 / 3 / 1063.7 | 4 (max 77) | 42.6, 58.3, 34.6 | 176 / 108 | app desktop: LCP 496 ms (> 400); app desktop: live update 2: a task took 58.3 ms (> 50 ms) |
| history | phone | 1368 | 8 | 680 | 968 | 0.0059 | 673 | 343.3 / 5 / 1320.2 | 7 (max 343) | 54.2, 49.6, 54 | 200 / 74.3 | history phone: trace load task 343.3 ms (> 200); history phone: observer load task 343 ms (> 200); history phone: live update 1: a task took 54.2 ms (> 50 ms); history phone: live update 3: a task took 54 ms (> 50 ms) |
| history | desktop | 580.5 | 8 | 272 | 448 | 0.0025 | 2132 | 53.1 / 1 / 630.7 | 1 (max 53) | 18.5, 24, 27.2 | 16 / 36.5 | history desktop: LCP 448 ms (> 400) |

Budget violations:
- app phone: observer load task 207 ms (> 200)
- app desktop: LCP 496 ms (> 400)
- app desktop: live update 2: a task took 58.3 ms (> 50 ms)
- history phone: trace load task 343.3 ms (> 200)
- history phone: observer load task 343 ms (> 200)
- history phone: live update 1: a task took 54.2 ms (> 50 ms)
- history phone: live update 3: a task took 54 ms (> 50 ms)
- history desktop: LCP 448 ms (> 400)
