# perf-check 2026-11-22T03:51:38.393Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 4552.8 | 150 | 1508 | 2636 | 0.0087 | 2023 | 311.8 / 6 / 2637.1 | 7 (max 370) | 37.4, 30.6, 28 | 120 / 93.4 | app phone: trace load task 311.8 ms (> 200); app phone: observer load task 370 ms (> 200); app phone: LCP 2636 ms (> 1200) |
| app | desktop | 1731.4 | 150 | 356 | 660 | 0.0178 | 3458 | 60.1 / 2 / 1845.8 | 1 (max 59) | 48.4, 70.1, 46.1 | 120 / 65.1 | app desktop: LCP 660 ms (> 400); app desktop: live update 2: a task took 70.1 ms (> 50 ms) |
| chart | phone | 1859.6 | 2 | 760 | 1368 | 0.002 | 526 | 278.8 / 5 / 1154.8 | 6 (max 278) | 13.5, 15.2, 14.7 | 48 / 69 | chart phone: trace load task 278.8 ms (> 200); chart phone: observer load task 278 ms (> 200); chart phone: LCP 1368 ms (> 1200) |
| chart | desktop | 761.1 | 2 | 292 | 636 | 0.0025 | 1842 | 64.4 / 1 / 558.1 | 2 (max 64) | 7.5, 15.1, 6.6 | 16 / 29.8 | chart desktop: LCP 636 ms (> 400) |
| overview | phone | 1309 | 1 | 772 | 1460 | 0.0088 | 2362 | 128.6 / 2 / 521.6 | 4 (max 184) | 21.9, 18.4, 22.1 | 152 / 146.4 | overview phone: LCP 1460 ms (> 1200) |
| overview | desktop | 592.5 | 1 | 320 | 684 | 0.0028 | 5295 | 46.3 / 0 / 450.4 | 1 (max 65) | 8.3, 11.4, 6.9 | 80 / 63.8 | overview desktop: LCP 684 ms (> 400) |
| history | phone | 1522.7 | 8 | 732 | 1224 | 0.0059 | 676 | 304.4 / 5 / 1416.3 | 7 (max 304) | 118.2, 89.5, 82.8 | 240 / 107.8 | history phone: trace load task 304.4 ms (> 200); history phone: observer load task 304 ms (> 200); history phone: LCP 1224 ms (> 1200); history phone: INP 240 ms (> 200); history phone: live update 1: a task took 118.2 ms (> 50 ms); history phone: live update 2: a task took 89.5 ms (> 50 ms); histor |
| history | desktop | 672.4 | 8 | 304 | 608 | 0.0044 | 2122 | 56.3 / 1 / 642.8 | 2 (max 56) | 46.7, 48.2, 45.1 | 32 / 23.6 | history desktop: LCP 608 ms (> 400) |
| host | phone | 1216.6 | 1 | 668 | 1324 | 0.002 | 464 | 135.1 / 3 / 549.8 | 3 (max 159) | 19.6, 19.7, 20.8 | 64 / 21.7 | host phone: LCP 1324 ms (> 1200) |
| host | desktop | 602.7 | 1 | 324 | 696 | 0.0237 | 1768 | 34.8 / 0 / 431.1 | 1 (max 59) | 12.7, 7.5, 15.5 | 56 / 25.3 | host desktop: LCP 696 ms (> 400) |
| board | phone | 1198.1 | 40 | 696 | 1540 | 0.0575 | 815 | 107.6 / 2 / 438 | 4 (max 174) | 43.5, 23.4, 19.9 | 64 / 282.2 | board phone: LCP 1540 ms (> 1200) |
| board | desktop | 655.4 | 40 | 408 | 784 | 0.0025 | 1808 | 63 / 1 / 370.1 | 3 (max 74) | 14.1, 11.8, 15.9 | 72 / 50.8 | board desktop: LCP 784 ms (> 400) |
| logs | phone | 1540.7 | 13 | 748 | 1756 | 0.028 | 335 | 150.8 / 3 / 818.3 | 5 (max 191) | 28.9, 25.4, 23.8 | 112 / 81.5 | logs phone: LCP 1756 ms (> 1200) |
| logs | desktop | 657.9 | 22 | 280 | 748 | 0.0124 | 1668 | 35 / 0 / 536.7 | 0 (max 0) | 28.5, 33.2, 8.9 | 88 / 50.5 | logs desktop: LCP 748 ms (> 400) |
| settings | phone | 1284.4 | 0 | 804 | 1344 | 0.002 | 342 | 89.5 / 1 / 329.7 | 3 (max 228) | 33.3, 24.5, 17.5 | 64 / 48 | settings phone: observer load task 228 ms (> 200); settings phone: LCP 1344 ms (> 1200) |
| settings | desktop | 572.6 | 0 | 300 | 608 | 0.0025 | 1646 | 23.6 / 0 / 437.2 | 1 (max 63) | 16.5, 11.3, 10.9 | 32 / 17.5 | settings desktop: LCP 608 ms (> 400) |

Budget violations:
- app phone: trace load task 311.8 ms (> 200)
- app phone: observer load task 370 ms (> 200)
- app phone: LCP 2636 ms (> 1200)
- app desktop: LCP 660 ms (> 400)
- app desktop: live update 2: a task took 70.1 ms (> 50 ms)
- chart phone: trace load task 278.8 ms (> 200)
- chart phone: observer load task 278 ms (> 200)
- chart phone: LCP 1368 ms (> 1200)
- chart desktop: LCP 636 ms (> 400)
- overview phone: LCP 1460 ms (> 1200)
- overview desktop: LCP 684 ms (> 400)
- history phone: trace load task 304.4 ms (> 200)
- history phone: observer load task 304 ms (> 200)
- history phone: LCP 1224 ms (> 1200)
- history phone: INP 240 ms (> 200)
- history phone: live update 1: a task took 118.2 ms (> 50 ms)
- history phone: live update 2: a task took 89.5 ms (> 50 ms)
- history phone: live update 3: a task took 82.8 ms (> 50 ms)
- history desktop: LCP 608 ms (> 400)
- host phone: LCP 1324 ms (> 1200)
- host desktop: LCP 696 ms (> 400)
- board phone: LCP 1540 ms (> 1200)
- board desktop: LCP 784 ms (> 400)
- logs phone: LCP 1756 ms (> 1200)
- logs desktop: LCP 748 ms (> 400)
- settings phone: observer load task 228 ms (> 200)
- settings phone: LCP 1344 ms (> 1200)
- settings desktop: LCP 608 ms (> 400)
