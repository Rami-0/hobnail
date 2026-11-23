# perf-check 2026-11-22T04:19:19.007Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 3101.3 | 150 | 828 | 1512 | 0.0087 | 2025 | 158.9 / 3 / 1993.5 | 3 (max 195) | 37.1, 19.8, 21.3 | 104 / 72.9 | app phone: LCP 1512 ms (> 1200) |
| app | desktop | 1689 | 150 | 324 | 636 | 0.0177 | 3460 | 56.8 / 4 / 1760.2 | 1 (max 66) | 54.5, 54.3, 52.1 | 192 / 100.8 | app desktop: LCP 636 ms (> 400); app desktop: live update 1: a task took 54.5 ms (> 50 ms); app desktop: live update 2: a task took 54.3 ms (> 50 ms); app desktop: live update 3: a task took 52.1 ms (> 50 ms) |
| chart | phone | 1970.9 | 2 | 796 | 1388 | 0.002 | 530 | 361.5 / 5 / 1249.8 | 6 (max 361) | 16.6, 18, 12.5 | 40 / 57.1 | chart phone: trace load task 361.5 ms (> 200); chart phone: observer load task 361 ms (> 200); chart phone: LCP 1388 ms (> 1200) |
| chart | desktop | 823.3 | 2 | 332 | 692 | 0.0025 | 1844 | 65.4 / 1 / 522.7 | 1 (max 65) | 4.7, 5.6, 5.4 | - / 32.7 | chart desktop: LCP 692 ms (> 400); chart desktop: missing INP measurement |
| overview | phone | 1289.2 | 1 | 748 | 1436 | 0.0088 | 2066 | 117.8 / 3 / 552.4 | 5 (max 179) | 21.8, 15.8, 17.6 | 120 / 135.2 | overview phone: LCP 1436 ms (> 1200) |
| overview | desktop | 694.6 | 1 | 344 | 768 | 0.0028 | 5595 | 36.6 / 0 / 643.3 | 1 (max 60) | 8, 14.6, 28.6 | 80 / 48.9 | overview desktop: LCP 768 ms (> 400) |
| history | phone | 1428.1 | 8 | 744 | 1140 | 0.0059 | 678 | 304.4 / 5 / 1339.7 | 6 (max 304) | 112.8, 98.3, 98.6 | 216 / 140.1 | history phone: trace load task 304.4 ms (> 200); history phone: observer load task 304 ms (> 200); history phone: INP 216 ms (> 200); history phone: live update 1: a task took 112.8 ms (> 50 ms); history phone: live update 2: a task took 98.3 ms (> 50 ms); history phone: live update 3: a task took 9 |
| history | desktop | 610.3 | 8 | 276 | 580 | 0.0026 | 2124 | 62.4 / 1 / 627.7 | 1 (max 62) | 42.7, 44.9, 48 | 40 / 9.8 | history desktop: LCP 580 ms (> 400) |
| host | phone | 1253.5 | 1 | 764 | 1372 | 0.002 | 466 | 132.7 / 3 / 501.8 | 5 (max 174) | 24.7, 18.8, 14.5 | 72 / 28.6 | host phone: LCP 1372 ms (> 1200) |
| host | desktop | 535.9 | 1 | 276 | 616 | 0.0237 | 1770 | 30.8 / 0 / 421.2 | 1 (max 51) | 12.7, 10.9, 11.7 | 40 / 7.1 | host desktop: LCP 616 ms (> 400) |
| board | phone | 1313.3 | 40 | 724 | 1648 | 0.0575 | 817 | 100.1 / 3 / 435 | 4 (max 182) | 23.5, 18, 17.6 | 64 / 320.4 | board phone: LCP 1648 ms (> 1200) |
| board | desktop | 622.8 | 40 | 340 | 708 | 0.0025 | 1810 | 49.4 / 0 / 536.5 | 2 (max 62) | 9.8, 30.4, 29.6 | 88 / 60.6 | board desktop: LCP 708 ms (> 400) |
| logs | phone | 1509.6 | 13 | 728 | 1736 | 0.028 | 337 | 127.3 / 3 / 827.4 | 5 (max 180) | 32.7, 27, 33 | 120 / 80.3 | logs phone: LCP 1736 ms (> 1200) |
| logs | desktop | 671 | 22 | 316 | 756 | 0.0093 | 1670 | 40.3 / 0 / 434.1 | 0 (max 0) | 26.4, 33.4, 28.7 | 136 / 81 | logs desktop: LCP 756 ms (> 400) |
| settings | phone | 1419.2 | 0 | 908 | 1484 | 0.002 | 344 | 97.8 / 1 / 357 | 3 (max 302) | 32.7, 27.3, 23.7 | 56 / 35.1 | settings phone: observer load task 302 ms (> 200); settings phone: LCP 1484 ms (> 1200) |
| settings | desktop | 578.8 | 0 | 300 | 608 | 0.0026 | 1648 | 23.6 / 0 / 256.3 | 2 (max 59) | 15, 22.1, 10.8 | 48 / 21.4 | settings desktop: LCP 608 ms (> 400) |

Budget violations:
- app phone: LCP 1512 ms (> 1200)
- app desktop: LCP 636 ms (> 400)
- app desktop: live update 1: a task took 54.5 ms (> 50 ms)
- app desktop: live update 2: a task took 54.3 ms (> 50 ms)
- app desktop: live update 3: a task took 52.1 ms (> 50 ms)
- chart phone: trace load task 361.5 ms (> 200)
- chart phone: observer load task 361 ms (> 200)
- chart phone: LCP 1388 ms (> 1200)
- chart desktop: LCP 692 ms (> 400)
- chart desktop: missing INP measurement
- overview phone: LCP 1436 ms (> 1200)
- overview desktop: LCP 768 ms (> 400)
- history phone: trace load task 304.4 ms (> 200)
- history phone: observer load task 304 ms (> 200)
- history phone: INP 216 ms (> 200)
- history phone: live update 1: a task took 112.8 ms (> 50 ms)
- history phone: live update 2: a task took 98.3 ms (> 50 ms)
- history phone: live update 3: a task took 98.6 ms (> 50 ms)
- history desktop: LCP 580 ms (> 400)
- host phone: LCP 1372 ms (> 1200)
- host desktop: LCP 616 ms (> 400)
- board phone: LCP 1648 ms (> 1200)
- board desktop: LCP 708 ms (> 400)
- logs phone: LCP 1736 ms (> 1200)
- logs desktop: LCP 756 ms (> 400)
- settings phone: observer load task 302 ms (> 200)
- settings phone: LCP 1484 ms (> 1200)
- settings desktop: LCP 608 ms (> 400)
