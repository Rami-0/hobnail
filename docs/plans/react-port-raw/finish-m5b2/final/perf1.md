# perf-check 2026-11-22T04:52:40.758Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 2877.5 | 150 | 676 | 1188 | 0.0087 | 2024 | 155.7 / 3 / 1657.9 | 5 (max 201) | 25.6, 19, 22.1 | 168 / 151.8 | app phone: observer load task 201 ms (> 200) |
| app | desktop | 1456.5 | 150 | 332 | 472 | 0.0177 | 3459 | 108.1 / 5 / 1533.2 | 1 (max 52) | 55.9, 58.4, 53.3 | 144 / 71.6 | app desktop: LCP 472 ms (> 400); app desktop: live update 1: a task took 55.9 ms (> 50 ms); app desktop: live update 2: a task took 58.4 ms (> 50 ms); app desktop: live update 3: a task took 53.3 ms (> 50 ms) |
| chart | phone | 2025.6 | 2 | 728 | 1476 | 0.002 | 527 | 295.8 / 5 / 1275.8 | 6 (max 295) | 14.9, 14.2, 17.2 | 56 / 61.3 | chart phone: trace load task 295.8 ms (> 200); chart phone: observer load task 295 ms (> 200); chart phone: LCP 1476 ms (> 1200) |
| chart | desktop | 744.4 | 2 | 332 | 556 | 0.0025 | 1843 | 61 / 1 / 555.7 | 2 (max 73) | 7.1, 14.6, 5.3 | - / 49 | chart desktop: LCP 556 ms (> 400); chart desktop: missing INP measurement |
| overview | phone | 1220.9 | 1 | 680 | 1372 | 0.0088 | 1771 | 134.6 / 2 / 464.8 | 5 (max 183) | 20.4, 20.9, 22.2 | 144 / 139.8 | overview phone: LCP 1372 ms (> 1200) |
| overview | desktop | 454.8 | 1 | 368 | 556 | 0.0027 | 5837 | 39 / 0 / 571.1 | 1 (max 52) | 10.3, 11.5, 22.9 | 64 / 46.3 | overview desktop: LCP 556 ms (> 400) |
| history | phone | 1338.3 | 8 | 640 | 932 | 0.0039 | 677 | 255.8 / 5 / 1204.1 | 6 (max 255) | 54.5, 47.2, 45.6 | 32 / 74.7 | history phone: trace load task 255.8 ms (> 200); history phone: observer load task 255 ms (> 200); history phone: live update 1: a task took 54.5 ms (> 50 ms) |
| history | desktop | 537.6 | 8 | 280 | 372 | 0.0047 | 2123 | 50.5 / 1 / 655.2 | 1 (max 50) | 19.9, 29.2, 24 | 16 / 20.9 |  |
| host | phone | 1213.4 | 1 | 636 | 1392 | 0 | 466 | 167.4 / 2 / 483 | 3 (max 166) | 24.6, 24.8, 15.5 | 64 / 24.7 | host phone: LCP 1392 ms (> 1200) |
| host | desktop | 336 | 1 | 232 | 428 | 0.0238 | 1770 | 59.5 / 1 / 299.6 | 1 (max 59) | 7, 6.4, 19.2 | 48 / 10.1 | host desktop: LCP 428 ms (> 400) |
| board | phone | 1197.6 | 40 | 624 | 1532 | 0.0575 | 816 | 91 / 2 / 363.1 | 4 (max 177) | 33.5, 19.6, 19.4 | 64 / 331.9 | board phone: LCP 1532 ms (> 1200) |
| board | desktop | 461.5 | 40 | 344 | 536 | 0.0025 | 1809 | 54.1 / 1 / 437.7 | 2 (max 71) | 15.3, 14.8, 11.8 | 72 / 48.4 | board desktop: LCP 536 ms (> 400) |
| logs | phone | 1527.4 | 13 | 652 | 1740 | 0.028 | 336 | 158.3 / 3 / 841.5 | 5 (max 190) | 32.2, 31.2, 23.8 | 120 / 79.7 | logs phone: LCP 1740 ms (> 1200) |
| logs | desktop | 517.2 | 22 | 276 | 640 | 0.0029 | 1669 | 38.2 / 0 / 496.3 | 0 (max 0) | 28.2, 42.5, 23.7 | 88 / 39.8 | logs desktop: LCP 640 ms (> 400) |
| settings | phone | 1268 | 0 | 716 | 1336 | 0.002 | 344 | 102.2 / 1 / 362.8 | 3 (max 208) | 31.4, 22.6, 47.6 | 72 / 36.7 | settings phone: observer load task 208 ms (> 200); settings phone: LCP 1336 ms (> 1200) |
| settings | desktop | 404.5 | 0 | 276 | 460 | 0.0026 | 1648 | 23.2 / 0 / 270.5 | 1 (max 60) | 13.6, 13.5, 11.4 | 48 / 30.3 | settings desktop: LCP 460 ms (> 400) |

Budget violations:
- app phone: observer load task 201 ms (> 200)
- app desktop: LCP 472 ms (> 400)
- app desktop: live update 1: a task took 55.9 ms (> 50 ms)
- app desktop: live update 2: a task took 58.4 ms (> 50 ms)
- app desktop: live update 3: a task took 53.3 ms (> 50 ms)
- chart phone: trace load task 295.8 ms (> 200)
- chart phone: observer load task 295 ms (> 200)
- chart phone: LCP 1476 ms (> 1200)
- chart desktop: LCP 556 ms (> 400)
- chart desktop: missing INP measurement
- overview phone: LCP 1372 ms (> 1200)
- overview desktop: LCP 556 ms (> 400)
- history phone: trace load task 255.8 ms (> 200)
- history phone: observer load task 255 ms (> 200)
- history phone: live update 1: a task took 54.5 ms (> 50 ms)
- host phone: LCP 1392 ms (> 1200)
- host desktop: LCP 428 ms (> 400)
- board phone: LCP 1532 ms (> 1200)
- board desktop: LCP 536 ms (> 400)
- logs phone: LCP 1740 ms (> 1200)
- logs desktop: LCP 640 ms (> 400)
- settings phone: observer load task 208 ms (> 200)
- settings phone: LCP 1336 ms (> 1200)
- settings desktop: LCP 460 ms (> 400)
