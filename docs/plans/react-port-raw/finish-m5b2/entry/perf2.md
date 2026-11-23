# perf-check 2026-11-22T04:23:33.204Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 3147 | 150 | 812 | 1492 | 0.0087 | 2061 | 161.1 / 3 / 2086.4 | 3 (max 208) | 27.5, 16.5, 20 | 120 / 85.4 | app phone: observer load task 208 ms (> 200); app phone: LCP 1492 ms (> 1200) |
| app | desktop | 1695.8 | 150 | 348 | 664 | 0.0178 | 3552 | 52.8 / 3 / 1943.2 | 1 (max 70) | 44.8, 90, 52.4 | 136 / 65.6 | app desktop: LCP 664 ms (> 400); app desktop: live update 2: a task took 90 ms (> 50 ms); app desktop: live update 3: a task took 52.4 ms (> 50 ms) |
| chart | phone | 2005.6 | 2 | 816 | 1468 | 0.002 | 540 | 286.7 / 5 / 1233.4 | 6 (max 286) | 14.9, 11.9, 10.7 | 112 / 81.8 | chart phone: trace load task 286.7 ms (> 200); chart phone: observer load task 286 ms (> 200); chart phone: LCP 1468 ms (> 1200) |
| chart | desktop | 821.1 | 2 | 316 | 656 | 0.0025 | 1844 | 68 / 1 / 561.6 | 3 (max 68) | 8.2, 4.9, 7.1 | 16 / 30.1 | chart desktop: LCP 656 ms (> 400) |
| overview | phone | 1227.4 | 1 | 688 | 1368 | 0.0088 | 2364 | 115.5 / 3 / 548.4 | 4 (max 159) | 20.9, 22.3, 16.5 | 136 / 141.6 | overview phone: LCP 1368 ms (> 1200) |
| overview | desktop | 669.8 | 1 | 332 | 776 | 0.0401 | 5297 | 47.7 / 0 / 432.8 | 1 (max 55) | 6.2, 8.4, 26.9 | 80 / 44.3 | overview desktop: LCP 776 ms (> 400) |
| history | phone | 1410.7 | 8 | 724 | 1132 | 0.0059 | 678 | 296.2 / 5 / 1390.5 | 7 (max 295) | 125.5, 97.2, 82 | 248 / 102.8 | history phone: trace load task 296.2 ms (> 200); history phone: observer load task 295 ms (> 200); history phone: INP 248 ms (> 200); history phone: live update 1: a task took 125.5 ms (> 50 ms); history phone: live update 2: a task took 97.2 ms (> 50 ms); history phone: live update 3: a task took 8 |
| history | desktop | 689.5 | 8 | 308 | 620 | 0.0044 | 2124 | 67.5 / 1 / 685.4 | 2 (max 67) | 47.6, 44.6, 57.8 | 32 / 22.3 | history desktop: LCP 620 ms (> 400); history desktop: live update 3: a task took 57.8 ms (> 50 ms) |
| host | phone | 1196 | 1 | 704 | 1304 | 0.002 | 466 | 121.8 / 3 / 507 | 3 (max 173) | 24, 23.3, 15.6 | 64 / 24.6 | host phone: LCP 1304 ms (> 1200) |
| host | desktop | 547.4 | 1 | 252 | 624 | 0.0027 | 1770 | 32.6 / 0 / 302.5 | 0 (max 0) | 6.7, 10.1, 7.7 | 48 / 5.9 | host desktop: LCP 624 ms (> 400) |
| board | phone | 1239.2 | 40 | 720 | 1576 | 0.0575 | 817 | 100.8 / 2 / 415.6 | 4 (max 190) | 27.8, 18.2, 22.2 | 64 / 294.6 | board phone: LCP 1576 ms (> 1200) |
| board | desktop | 597.2 | 40 | 320 | 664 | 0.0026 | 1810 | 67.7 / 1 / 496.2 | 2 (max 68) | 16.2, 10.8, 14.4 | 80 / 51 | board desktop: LCP 664 ms (> 400) |
| logs | phone | 1598.7 | 13 | 760 | 1792 | 0.0366 | 337 | 134.2 / 3 / 801.1 | 5 (max 189) | 24.7, 30.9, 23.6 | 120 / 89 | logs phone: LCP 1792 ms (> 1200) |
| logs | desktop | 724.2 | 22 | 336 | 816 | 0.0125 | 1670 | 47.7 / 0 / 555 | 1 (max 55) | 19.3, 33.7, 33.1 | 96 / 47.2 | logs desktop: LCP 816 ms (> 400) |
| settings | phone | 1330.4 | 0 | 840 | 1392 | 0.002 | 344 | 96.9 / 1 / 339.6 | 3 (max 235) | 30.6, 35.5, 24.5 | 56 / 41.9 | settings phone: observer load task 235 ms (> 200); settings phone: LCP 1392 ms (> 1200) |
| settings | desktop | 590.5 | 0 | 312 | 620 | 0.0025 | 1648 | 19.1 / 0 / 241.8 | 2 (max 66) | 12, 10.6, 8.8 | 48 / 20.8 | settings desktop: LCP 620 ms (> 400) |

Budget violations:
- app phone: observer load task 208 ms (> 200)
- app phone: LCP 1492 ms (> 1200)
- app desktop: LCP 664 ms (> 400)
- app desktop: live update 2: a task took 90 ms (> 50 ms)
- app desktop: live update 3: a task took 52.4 ms (> 50 ms)
- chart phone: trace load task 286.7 ms (> 200)
- chart phone: observer load task 286 ms (> 200)
- chart phone: LCP 1468 ms (> 1200)
- chart desktop: LCP 656 ms (> 400)
- overview phone: LCP 1368 ms (> 1200)
- overview desktop: LCP 776 ms (> 400)
- history phone: trace load task 296.2 ms (> 200)
- history phone: observer load task 295 ms (> 200)
- history phone: INP 248 ms (> 200)
- history phone: live update 1: a task took 125.5 ms (> 50 ms)
- history phone: live update 2: a task took 97.2 ms (> 50 ms)
- history phone: live update 3: a task took 82 ms (> 50 ms)
- history desktop: LCP 620 ms (> 400)
- history desktop: live update 3: a task took 57.8 ms (> 50 ms)
- host phone: LCP 1304 ms (> 1200)
- host desktop: LCP 624 ms (> 400)
- board phone: LCP 1576 ms (> 1200)
- board desktop: LCP 664 ms (> 400)
- logs phone: LCP 1792 ms (> 1200)
- logs desktop: LCP 816 ms (> 400)
- settings phone: observer load task 235 ms (> 200)
- settings phone: LCP 1392 ms (> 1200)
- settings desktop: LCP 620 ms (> 400)
