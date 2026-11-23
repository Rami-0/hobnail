# perf-check 2026-11-22T03:55:51.523Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 3202.7 | 150 | 864 | 1580 | 0.0087 | 2059 | 163.9 / 4 / 2094.3 | 4 (max 205) | 32.7, 20.7, 20.9 | 112 / 81 | app phone: observer load task 205 ms (> 200); app phone: LCP 1580 ms (> 1200) |
| app | desktop | 1692 | 150 | 332 | 612 | 0.0177 | 3550 | 63.9 / 3 / 1653.5 | 1 (max 64) | 48.2, 54.8, 45.1 | 136 / 65 | app desktop: LCP 612 ms (> 400); app desktop: live update 2: a task took 54.8 ms (> 50 ms) |
| chart | phone | 1956.2 | 2 | 796 | 1408 | 0.002 | 526 | 316.3 / 5 / 1177.3 | 6 (max 316) | 15.6, 11.1, 14.9 | 48 / 55.7 | chart phone: trace load task 316.3 ms (> 200); chart phone: observer load task 316 ms (> 200); chart phone: LCP 1408 ms (> 1200) |
| chart | desktop | 855.3 | 2 | 304 | 684 | 0.0026 | 1842 | 70.6 / 1 / 461 | 2 (max 70) | 5.2, 7.2, 5 | 16 / 35.2 | chart desktop: LCP 684 ms (> 400) |
| overview | phone | 1364.1 | 1 | 796 | 1508 | 0.0088 | 1768 | 119.8 / 2 / 499.5 | 4 (max 192) | 30.6, 15.7, 18.7 | 136 / 167.2 | overview phone: LCP 1508 ms (> 1200) |
| overview | desktop | 658.6 | 1 | 372 | 744 | 0.0028 | 5834 | 29.6 / 0 / 470.3 | 1 (max 100) | 20.9, 8, 9.6 | 72 / 49.1 | overview desktop: LCP 744 ms (> 400) |
| history | phone | 1400.6 | 8 | 676 | 1080 | 0.0059 | 676 | 273.7 / 5 / 1339 | 7 (max 273) | 97.2, 93.7, 96.2 | 168 / 118.9 | history phone: trace load task 273.7 ms (> 200); history phone: observer load task 273 ms (> 200); history phone: live update 1: a task took 97.2 ms (> 50 ms); history phone: live update 2: a task took 93.7 ms (> 50 ms); history phone: live update 3: a task took 96.2 ms (> 50 ms) |
| history | desktop | 625.6 | 8 | 280 | 580 | 0.0044 | 2122 | 57 / 1 / 704.2 | 1 (max 57) | 34.6, 40.1, 54.5 | 24 / 12.3 | history desktop: LCP 580 ms (> 400); history desktop: live update 3: a task took 54.5 ms (> 50 ms) |
| host | phone | 1189.8 | 1 | 696 | 1320 | 0.002 | 464 | 136.5 / 2 / 516.9 | 3 (max 184) | 17.5, 18, 17.6 | 72 / 28.6 | host phone: LCP 1320 ms (> 1200) |
| host | desktop | 545.5 | 1 | 256 | 648 | 0.0027 | 1768 | 46.5 / 0 / 306.8 | 0 (max 0) | 8.9, 17.8, 15.3 | 48 / 7.3 | host desktop: LCP 648 ms (> 400) |
| board | phone | 1263.4 | 40 | 708 | 1600 | 0.0575 | 815 | 88.9 / 2 / 395.7 | 4 (max 200) | 26.6, 18.7, 23.1 | 64 / 280.4 | board phone: LCP 1600 ms (> 1200) |
| board | desktop | 626.6 | 40 | 356 | 684 | 0.0025 | 1808 | 42.9 / 0 / 352.3 | 1 (max 79) | 9.9, 13.9, 10.6 | 80 / 50.7 | board desktop: LCP 684 ms (> 400) |
| logs | phone | 1525.4 | 13 | 760 | 1744 | 0.0366 | 335 | 119.2 / 3 / 828.5 | 5 (max 194) | 32.1, 33.2, 27 | 120 / 78.7 | logs phone: LCP 1744 ms (> 1200) |
| logs | desktop | 669.2 | 22 | 292 | 760 | 0.0124 | 1668 | 48.8 / 0 / 405.1 | 0 (max 0) | 31.8, 31.9, 36.4 | 96 / 51.4 | logs desktop: LCP 760 ms (> 400) |
| settings | phone | 1297.7 | 0 | 784 | 1360 | 0.002 | 342 | 100.7 / 1 / 344.8 | 3 (max 208) | 32.6, 29.9, 21 | 64 / 38.1 | settings phone: observer load task 208 ms (> 200); settings phone: LCP 1360 ms (> 1200) |
| settings | desktop | 740.7 | 0 | 344 | 780 | 0.0026 | 1646 | 34.1 / 0 / 406.7 | 1 (max 61) | 15.3, 13.3, 8.7 | 48 / 29.8 | settings desktop: LCP 780 ms (> 400) |

Budget violations:
- app phone: observer load task 205 ms (> 200)
- app phone: LCP 1580 ms (> 1200)
- app desktop: LCP 612 ms (> 400)
- app desktop: live update 2: a task took 54.8 ms (> 50 ms)
- chart phone: trace load task 316.3 ms (> 200)
- chart phone: observer load task 316 ms (> 200)
- chart phone: LCP 1408 ms (> 1200)
- chart desktop: LCP 684 ms (> 400)
- overview phone: LCP 1508 ms (> 1200)
- overview desktop: LCP 744 ms (> 400)
- history phone: trace load task 273.7 ms (> 200)
- history phone: observer load task 273 ms (> 200)
- history phone: live update 1: a task took 97.2 ms (> 50 ms)
- history phone: live update 2: a task took 93.7 ms (> 50 ms)
- history phone: live update 3: a task took 96.2 ms (> 50 ms)
- history desktop: LCP 580 ms (> 400)
- history desktop: live update 3: a task took 54.5 ms (> 50 ms)
- host phone: LCP 1320 ms (> 1200)
- host desktop: LCP 648 ms (> 400)
- board phone: LCP 1600 ms (> 1200)
- board desktop: LCP 684 ms (> 400)
- logs phone: LCP 1744 ms (> 1200)
- logs desktop: LCP 760 ms (> 400)
- settings phone: observer load task 208 ms (> 200)
- settings phone: LCP 1360 ms (> 1200)
- settings desktop: LCP 780 ms (> 400)
