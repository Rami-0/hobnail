# perf-check 2026-11-22T04:56:54.614Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 2905.1 | 150 | 980 | 980 | 0.0087 | 2060 | 138.3 / 1 / 1197.3 | 5 (max 233) | 26.5, 21.5, 23.3 | 104 / 79.7 | app phone: observer load task 233 ms (> 200) |
| app | desktop | 1492.9 | 150 | 312 | 476 | 0.0177 | 3551 | 108.7 / 4 / 1581.7 | 2 (max 72) | 50.3, 54.5, 39.6 | 120 / 58.1 | app desktop: LCP 476 ms (> 400); app desktop: live update 1: a task took 50.3 ms (> 50 ms); app desktop: live update 2: a task took 54.5 ms (> 50 ms) |
| chart | phone | 2020.2 | 2 | 712 | 1488 | 0.002 | 539 | 275.2 / 5 / 1288.4 | 6 (max 275) | 15.4, 16, 15.9 | 112 / 84.7 | chart phone: trace load task 275.2 ms (> 200); chart phone: observer load task 275 ms (> 200); chart phone: LCP 1488 ms (> 1200) |
| chart | desktop | 709.7 | 2 | 324 | 548 | 0.0025 | 1843 | 59.8 / 2 / 555.8 | 3 (max 66) | 23.7, 6.4, 7.6 | - / 60.7 | chart desktop: LCP 548 ms (> 400); chart desktop: missing INP measurement |
| overview | phone | 1133.6 | 1 | 652 | 1312 | 0.0088 | 2067 | 159.3 / 2 / 491.3 | 5 (max 172) | 17.2, 26.5, 16.4 | 136 / 118.2 | overview phone: LCP 1312 ms (> 1200) |
| overview | desktop | 532 | 1 | 388 | 644 | 0.0027 | 5837 | 41.9 / 0 / 429.8 | 1 (max 72) | 7.5, 9, 12.7 | 48 / 46 | overview desktop: LCP 644 ms (> 400) |
| history | phone | 1329.1 | 8 | 588 | 884 | 0.0039 | 672 | 247.1 / 5 / 1271.3 | 7 (max 246) | 48.2, 43.5, 37 | 112 / 82.6 | history phone: trace load task 247.1 ms (> 200); history phone: observer load task 246 ms (> 200) |
| history | desktop | 550.8 | 8 | 312 | 448 | 0.0025 | 2116 | 50.6 / 1 / 553.7 | 1 (max 50) | 21.2, 18.9, 16 | 16 / 21.2 | history desktop: LCP 448 ms (> 400) |
| host | phone | 1200.3 | 1 | 656 | 1336 | 0.0021 | 466 | 120.8 / 2 / 420 | 3 (max 188) | 24.7, 16.2, 17.5 | 56 / 24.1 | host phone: LCP 1336 ms (> 1200) |
| host | desktop | 329.8 | 1 | 236 | 428 | 0.0025 | 1770 | 33.5 / 0 / 437.8 | 0 (max 0) | 15.9, 8.4, 14.5 | 40 / 9.5 | host desktop: LCP 428 ms (> 400) |
| board | phone | 1262.3 | 40 | 664 | 1592 | 0.0575 | 816 | 108.6 / 2 / 393.2 | 4 (max 170) | 29.3, 23.2, 19.2 | 72 / 303.2 | board phone: LCP 1592 ms (> 1200) |
| board | desktop | 459.6 | 40 | 332 | 540 | 0.0025 | 1809 | 54 / 1 / 417.4 | 2 (max 70) | 20.7, 10, 16.1 | 72 / 41.5 | board desktop: LCP 540 ms (> 400) |
| logs | phone | 1540.4 | 13 | 716 | 1772 | 0.028 | 336 | 151.3 / 4 / 832.1 | 5 (max 200) | 23.8, 32.1, 24.2 | 136 / 95.4 | logs phone: LCP 1772 ms (> 1200) |
| logs | desktop | 581.3 | 21 | 296 | 696 | 0.0132 | 1669 | 62.8 / 2 / 538.3 | 2 (max 62) | 30.5, 33.4, 30.9 | 96 / 64.6 | logs desktop: LCP 696 ms (> 400) |
| settings | phone | 1285.9 | 0 | 736 | 1344 | 0.002 | 344 | 109.7 / 1 / 357.3 | 3 (max 230) | 28.7, 21.7, 20.3 | 64 / 37.3 | settings phone: observer load task 230 ms (> 200); settings phone: LCP 1344 ms (> 1200) |
| settings | desktop | 465.3 | 0 | 324 | 516 | 0.0025 | 1648 | 30.5 / 0 / 339.6 | 1 (max 67) | 12.4, 10.5, 11 | 32 / 17 | settings desktop: LCP 516 ms (> 400) |

Budget violations:
- app phone: observer load task 233 ms (> 200)
- app desktop: LCP 476 ms (> 400)
- app desktop: live update 1: a task took 50.3 ms (> 50 ms)
- app desktop: live update 2: a task took 54.5 ms (> 50 ms)
- chart phone: trace load task 275.2 ms (> 200)
- chart phone: observer load task 275 ms (> 200)
- chart phone: LCP 1488 ms (> 1200)
- chart desktop: LCP 548 ms (> 400)
- chart desktop: missing INP measurement
- overview phone: LCP 1312 ms (> 1200)
- overview desktop: LCP 644 ms (> 400)
- history phone: trace load task 247.1 ms (> 200)
- history phone: observer load task 246 ms (> 200)
- history desktop: LCP 448 ms (> 400)
- host phone: LCP 1336 ms (> 1200)
- host desktop: LCP 428 ms (> 400)
- board phone: LCP 1592 ms (> 1200)
- board desktop: LCP 540 ms (> 400)
- logs phone: LCP 1772 ms (> 1200)
- logs desktop: LCP 696 ms (> 400)
- settings phone: observer load task 230 ms (> 200)
- settings phone: LCP 1344 ms (> 1200)
- settings desktop: LCP 516 ms (> 400)
