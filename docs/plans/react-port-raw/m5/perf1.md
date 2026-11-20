# perf-check 2026-11-19T05:13:44.602Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 5268.4 | 150 | 2112 | 2112 | 0.0132 | 2032 | 301.4 / 7 / 2919.9 | 9 (max 224) | 52.8, 33.9, 40.5 | 160 / 112.5 | app phone: trace load task 301.4 ms (> 200); app phone: observer load task 224 ms (> 200); app phone: LCP 2112 ms (> 1200); app phone: live update 1: a task took 52.8 ms (> 50 ms) |
| app | desktop | 2340.8 | 150 | 836 | 836 | 0.0596 | 3500 | 97.5 / 12 / 1554 | 12 (max 114) | 57.7, 52.3, 110.5 | 176 / 120.5 | app desktop: LCP 836 ms (> 400); app desktop: live update 1: a task took 57.7 ms (> 50 ms); app desktop: live update 2: a task took 52.3 ms (> 50 ms); app desktop: live update 3: a task took 110.5 ms (> 50 ms) |
| chart | phone | 3793.4 | 2 | 2124 | 2896 | 0.002 | 541 | 576 / 1 / 871.8 | 9 (max 576) | 26.3, 29.1, 29.6 | 240 / 375.6 | chart phone: trace load task 576 ms (> 200); chart phone: observer load task 576 ms (> 200); chart phone: LCP 2896 ms (> 1200); chart phone: INP 240 ms (> 200) |
| chart | desktop | 1064.3 | 2 | 500 | 684 | 0.0194 | 1850 | 122.5 / 1 / 380.8 | 3 (max 122) | 3.6, 3.7, 7.8 | 32 / 47.9 | chart desktop: LCP 684 ms (> 400) |
| overview | phone | 2185.9 | 2 | 1204 | 1760 | 0.002 | 575 | 160.4 / 1 / 467.9 | 6 (max 266) | 57.9, 60, 38.5 | 104 / 746.8 | overview phone: observer load task 266 ms (> 200); overview phone: LCP 1760 ms (> 1200); overview phone: live update 1: a task took 57.9 ms (> 50 ms); overview phone: live update 2: a task took 60 ms (> 50 ms) |
| overview | desktop | 1948.8 | 2 | 780 | 1228 | 0.0213 | 1966 | 114.9 / 6 / 810.4 | 7 (max 122) | 11.5, 10.5, 40.3 | 64 / 19.5 | overview desktop: LCP 1228 ms (> 400) |
| history | phone | 2504 | 8 | 916 | 916 | 0.0047 | 452 | 482.4 / 4 / 1298.2 | 6 (max 481) | 227.4, 261.3, 219.5 | 128 / 281.6 | history phone: trace load task 482.4 ms (> 200); history phone: observer load task 481 ms (> 200); history phone: live update 1: a task took 227.4 ms (> 50 ms); history phone: live update 2: a task took 261.3 ms (> 50 ms); history phone: live update 3: a task took 219.5 ms (> 50 ms) |
| history | desktop | 2038 | 8 | 604 | 604 | 0.026 | 1997 | 170.5 / 5 / 1208.6 | 7 (max 170) | 60.1, 96.4, 34.3 | 72 / 48 | history desktop: LCP 604 ms (> 400); history desktop: live update 1: a task took 60.1 ms (> 50 ms); history desktop: live update 2: a task took 96.4 ms (> 50 ms) |
| host | phone | 1358.9 | 8 | 1036 | 1412 | 0.002 | 499 | 164 / 4 / 1287 | 6 (max 164) | 22, 21.5, 19.1 | 64 / 24.4 | host phone: LCP 1412 ms (> 1200) |
| host | desktop | 384.3 | 8 | 324 | 432 | 0.0782 | 1967 | 122.6 / 1 / 709.9 | 2 (max 122) | 16.1, 18.7, 14.6 | 48 / 11.2 | host desktop: LCP 432 ms (> 400) |
| board | phone | 1521.4 | 40 | 1376 | 1680 | 0.0368 | 620 | 128 / 2 / 553 | 2 (max 356) | 43.4, 33.7, 42.3 | 136 / 485.3 | board phone: observer load task 356 ms (> 200); board phone: LCP 1680 ms (> 1200) |
| board | desktop | 391.4 | 40 | 512 | 512 | 0.0002 | 1813 | 53.4 / 1 / 622.4 | 2 (max 69) | 19.1, 11.5, 10.8 | 128 / 92.6 | board desktop: LCP 512 ms (> 400) |
| logs | phone | 2760.8 | 13 | 2164 | 3024 | 0.0032 | 344 | 309.3 / 5 / 1263.6 | 6 (max 354) | 40.8, 36, 28.8 | 168 / 79.7 | logs phone: trace load task 309.3 ms (> 200); logs phone: observer load task 354 ms (> 200); logs phone: LCP 3024 ms (> 1200) |
| logs | desktop | 1041 | 21 | 632 | 1192 | 0.033 | 1695 | 62.1 / 2 / 397.4 | 4 (max 87) | 18.1, 17.2, 13.5 | 104 / 45.1 | logs desktop: LCP 1192 ms (> 400) |
| settings | phone | 1703.6 | 0 | 1068 | 1828 | 0.002 | 258 | 233.6 / 2 / 389.2 | 5 (max 250) | 58.1, 25.8, 42.1 | 128 / 102.7 | settings phone: trace load task 233.6 ms (> 200); settings phone: observer load task 250 ms (> 200); settings phone: LCP 1828 ms (> 1200); settings phone: live update 1: a task took 58.1 ms (> 50 ms) |
| settings | desktop | 776.7 | 0 | 388 | 828 | 0.0224 | 1569 | 94 / 1 / 309.1 | 2 (max 94) | 7.3, 26, 28.9 | 96 / 30.2 | settings desktop: LCP 828 ms (> 400) |

Budget violations:
- app phone: trace load task 301.4 ms (> 200)
- app phone: observer load task 224 ms (> 200)
- app phone: LCP 2112 ms (> 1200)
- app phone: live update 1: a task took 52.8 ms (> 50 ms)
- app desktop: LCP 836 ms (> 400)
- app desktop: live update 1: a task took 57.7 ms (> 50 ms)
- app desktop: live update 2: a task took 52.3 ms (> 50 ms)
- app desktop: live update 3: a task took 110.5 ms (> 50 ms)
- chart phone: trace load task 576 ms (> 200)
- chart phone: observer load task 576 ms (> 200)
- chart phone: LCP 2896 ms (> 1200)
- chart phone: INP 240 ms (> 200)
- chart desktop: LCP 684 ms (> 400)
- overview phone: observer load task 266 ms (> 200)
- overview phone: LCP 1760 ms (> 1200)
- overview phone: live update 1: a task took 57.9 ms (> 50 ms)
- overview phone: live update 2: a task took 60 ms (> 50 ms)
- overview desktop: LCP 1228 ms (> 400)
- history phone: trace load task 482.4 ms (> 200)
- history phone: observer load task 481 ms (> 200)
- history phone: live update 1: a task took 227.4 ms (> 50 ms)
- history phone: live update 2: a task took 261.3 ms (> 50 ms)
- history phone: live update 3: a task took 219.5 ms (> 50 ms)
- history desktop: LCP 604 ms (> 400)
- history desktop: live update 1: a task took 60.1 ms (> 50 ms)
- history desktop: live update 2: a task took 96.4 ms (> 50 ms)
- host phone: LCP 1412 ms (> 1200)
- host desktop: LCP 432 ms (> 400)
- board phone: observer load task 356 ms (> 200)
- board phone: LCP 1680 ms (> 1200)
- board desktop: LCP 512 ms (> 400)
- logs phone: trace load task 309.3 ms (> 200)
- logs phone: observer load task 354 ms (> 200)
- logs phone: LCP 3024 ms (> 1200)
- logs desktop: LCP 1192 ms (> 400)
- settings phone: trace load task 233.6 ms (> 200)
- settings phone: observer load task 250 ms (> 200)
- settings phone: LCP 1828 ms (> 1200)
- settings phone: live update 1: a task took 58.1 ms (> 50 ms)
- settings desktop: LCP 828 ms (> 400)
