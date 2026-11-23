# perf-check 2026-11-22T09:32:20.077Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 2739.5 | 150 | 752 | 1176 | 0.0087 | 2020 | 179.4 / 3 / 1561.5 | 5 (max 197) | 34.9, 19.9, 21.4 | 144 / 98.9 |  |
| app | desktop | 1431.1 | 150 | 336 | 480 | 0.0177 | 3455 | 93.4 / 5 / 1099.7 | 5 (max 93) | 44.4, 46.5, 43.9 | 136 / 72 | app desktop: LCP 480 ms (> 400) |
| history | phone | 1370.6 | 8 | 660 | 944 | 0.0059 | 673 | 283.1 / 5 / 1248 | 7 (max 283) | 53.1, 51.3, 51 | 184 / 66.7 | history phone: trace load task 283.1 ms (> 200); history phone: observer load task 283 ms (> 200); history phone: live update 1: a task took 53.1 ms (> 50 ms); history phone: live update 2: a task took 51.3 ms (> 50 ms); history phone: live update 3: a task took 51 ms (> 50 ms) |
| history | desktop | 693.3 | 8 | 304 | 444 | 0.0044 | 2112 | 56.8 / 1 / 667.8 | 1 (max 56) | 17.2, 16.3, 15.2 | - / 53.4 | history desktop: LCP 444 ms (> 400); history desktop: missing INP measurement |

Budget violations:
- app desktop: LCP 480 ms (> 400)
- history phone: trace load task 283.1 ms (> 200)
- history phone: observer load task 283 ms (> 200)
- history phone: live update 1: a task took 53.1 ms (> 50 ms)
- history phone: live update 2: a task took 51.3 ms (> 50 ms)
- history phone: live update 3: a task took 51 ms (> 50 ms)
- history desktop: LCP 444 ms (> 400)
- history desktop: missing INP measurement
