# perf-check 2026-11-22T03:11:12.454Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 3166.5 | 150 | 1224 | 1224 | 0.0087 | 2059 | 168.2 / 3 / 1682.8 | 5 (max 191) | 31.6, 23, 23.4 | 120 / 81.2 | app phone: LCP 1224 ms (> 1200) |
| app | desktop | 1694.1 | 150 | 480 | 480 | 0.0429 | 3550 | 88.2 / 6 / 1572.8 | 3 (max 108) | 48.1, 55.6, 77 | 288 / 173.2 | app desktop: LCP 480 ms (> 400); app desktop: INP 288 ms (> 200); app desktop: live update 2: a task took 55.6 ms (> 50 ms); app desktop: live update 3: a task took 77 ms (> 50 ms) |
| chart | phone | 2574.6 | 2 | 1640 | 1904 | 0.002 | 535 | 343.2 / 2 / 731.8 | 8 (max 366) | 34.2, 14.7, 12.8 | 152 / 81 | chart phone: trace load task 343.2 ms (> 200); chart phone: observer load task 366 ms (> 200); chart phone: LCP 1904 ms (> 1200) |
| chart | desktop | 1040 | 2 | 428 | 568 | 0.0294 | 1839 | 104.1 / 1 / 458.1 | 2 (max 104) | 6.9, 5.8, 16.1 | 40 / 43 | chart desktop: LCP 568 ms (> 400) |
| overview | phone | 1748.4 | 1 | 984 | 1400 | 0.0088 | 870 | 59.5 / 1 / 291.6 | 7 (max 175) | 70, 101.3, 73.7 | 224 / 267.1 | overview phone: LCP 1400 ms (> 1200); overview phone: INP 224 ms (> 200); overview phone: live update 1: a task took 70 ms (> 50 ms); overview phone: live update 2: a task took 101.3 ms (> 50 ms); overview phone: live update 3: a task took 73.7 ms (> 50 ms) |
| overview | desktop | 1333.5 | 1 | 688 | 1040 | 0.072 | 5596 | 72.1 / 1 / 500.1 | 2 (max 127) | 20.5, 19.7, 11.4 | 96 / 49.2 | overview desktop: LCP 1040 ms (> 400) |
| history | phone | 3657.2 | 8 | 1352 | 1352 | 0.0077 | 668 | 525.5 / 5 / 2025.4 | 11 (max 526) | 192.4, 132, 122.3 | 256 / 191.1 | history phone: trace load task 525.5 ms (> 200); history phone: observer load task 526 ms (> 200); history phone: LCP 1352 ms (> 1200); history phone: INP 256 ms (> 200); history phone: live update 1: a task took 192.4 ms (> 50 ms); history phone: live update 2: a task took 132 ms (> 50 ms); history |
| history | desktop | 1325.1 | 8 | 528 | 528 | 0.0351 | 2114 | 101.5 / 3 / 705.2 | 4 (max 101) | 59.5, 57.2, 65.8 | 40 / 14 | history desktop: LCP 528 ms (> 400); history desktop: live update 1: a task took 59.5 ms (> 50 ms); history desktop: live update 2: a task took 57.2 ms (> 50 ms); history desktop: live update 3: a task took 65.8 ms (> 50 ms) |
| host | phone | 1711.4 | 1 | 1296 | 1896 | 0.002 | 523 | 225.1 / 5 / 1446.8 | 6 (max 247) | 25.5, 26, 23.6 | 88 / 37.9 | host phone: trace load task 225.1 ms (> 200); host phone: observer load task 247 ms (> 200); host phone: LCP 1896 ms (> 1200) |
| host | desktop | 478.2 | 1 | 464 | 604 | 0.0001 | 1827 | 41.1 / 0 / 599.2 | 1 (max 59) | 6.2, 5.3, 5.6 | 48 / 6.9 | host desktop: LCP 604 ms (> 400) |
| board | phone | 1738.4 | 40 | 1480 | 2092 | 0.0575 | 665 | 103.9 / 4 / 854.4 | 6 (max 271) | 45.5, 19.7, 26 | 144 / 781 | board phone: observer load task 271 ms (> 200); board phone: LCP 2092 ms (> 1200) |
| board | desktop | 794.2 | 40 | 668 | 944 | 0.028 | 1809 | 58.7 / 2 / 936.8 | 3 (max 93) | 16.7, 15.3, 14.5 | 160 / 105.8 | board desktop: LCP 944 ms (> 400) |
| logs | phone | 3403.5 | 13 | 2272 | 3880 | 0.0032 | 347 | 473.3 / 12 / 2579.3 | 14 (max 470) | 114.1, 94.7, 67.3 | 312 / 214.7 | logs phone: trace load task 473.3 ms (> 200); logs phone: observer load task 470 ms (> 200); logs phone: LCP 3880 ms (> 1200); logs phone: INP 312 ms (> 200); logs phone: live update 1: a task took 114.1 ms (> 50 ms); logs phone: live update 2: a task took 94.7 ms (> 50 ms); logs phone: live update  |
| logs | desktop | 950 | 21 | 640 | 1232 | 0.0387 | 1687 | 72.1 / 2 / 649.1 | 4 (max 113) | 25.5, 19.9, 20.4 | 136 / 52.6 | logs desktop: LCP 1232 ms (> 400) |
| settings | phone | 2517.1 | 0 | 2116 | 2600 | 0.0057 | 342 | 139.5 / 4 / 1014 | 5 (max 462) | 41.8, 31.4, 30.3 | 80 / 59.4 | settings phone: observer load task 462 ms (> 200); settings phone: LCP 2600 ms (> 1200) |
| settings | desktop | 560.7 | 0 | 464 | 604 | 0.1032 | 1647 | 57.1 / 1 / 374.7 | 2 (max 71) | 25.1, 9.3, 13.4 | 48 / 15.5 | settings desktop: LCP 604 ms (> 400); settings desktop: CLS 0.1032 (> 0.1) |

Budget violations:
- app phone: LCP 1224 ms (> 1200)
- app desktop: LCP 480 ms (> 400)
- app desktop: INP 288 ms (> 200)
- app desktop: live update 2: a task took 55.6 ms (> 50 ms)
- app desktop: live update 3: a task took 77 ms (> 50 ms)
- chart phone: trace load task 343.2 ms (> 200)
- chart phone: observer load task 366 ms (> 200)
- chart phone: LCP 1904 ms (> 1200)
- chart desktop: LCP 568 ms (> 400)
- overview phone: LCP 1400 ms (> 1200)
- overview phone: INP 224 ms (> 200)
- overview phone: live update 1: a task took 70 ms (> 50 ms)
- overview phone: live update 2: a task took 101.3 ms (> 50 ms)
- overview phone: live update 3: a task took 73.7 ms (> 50 ms)
- overview desktop: LCP 1040 ms (> 400)
- history phone: trace load task 525.5 ms (> 200)
- history phone: observer load task 526 ms (> 200)
- history phone: LCP 1352 ms (> 1200)
- history phone: INP 256 ms (> 200)
- history phone: live update 1: a task took 192.4 ms (> 50 ms)
- history phone: live update 2: a task took 132 ms (> 50 ms)
- history phone: live update 3: a task took 122.3 ms (> 50 ms)
- history desktop: LCP 528 ms (> 400)
- history desktop: live update 1: a task took 59.5 ms (> 50 ms)
- history desktop: live update 2: a task took 57.2 ms (> 50 ms)
- history desktop: live update 3: a task took 65.8 ms (> 50 ms)
- host phone: trace load task 225.1 ms (> 200)
- host phone: observer load task 247 ms (> 200)
- host phone: LCP 1896 ms (> 1200)
- host desktop: LCP 604 ms (> 400)
- board phone: observer load task 271 ms (> 200)
- board phone: LCP 2092 ms (> 1200)
- board desktop: LCP 944 ms (> 400)
- logs phone: trace load task 473.3 ms (> 200)
- logs phone: observer load task 470 ms (> 200)
- logs phone: LCP 3880 ms (> 1200)
- logs phone: INP 312 ms (> 200)
- logs phone: live update 1: a task took 114.1 ms (> 50 ms)
- logs phone: live update 2: a task took 94.7 ms (> 50 ms)
- logs phone: live update 3: a task took 67.3 ms (> 50 ms)
- logs desktop: LCP 1232 ms (> 400)
- settings phone: observer load task 462 ms (> 200)
- settings phone: LCP 2600 ms (> 1200)
- settings desktop: LCP 604 ms (> 400)
- settings desktop: CLS 0.1032 (> 0.1)
