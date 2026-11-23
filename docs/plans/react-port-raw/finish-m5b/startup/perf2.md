# perf-check 2026-11-22T03:30:30.681Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 3131.2 | 150 | 860 | 1536 | 0.0087 | 2059 | 172.4 / 3 / 2008.1 | 3 (max 186) | 24.8, 21.8, 29.9 | 112 / 86.9 | app phone: LCP 1536 ms (> 1200) |
| app | desktop | 1792.3 | 150 | 332 | 636 | 0.0179 | 3550 | 81.7 / 4 / 1703.3 | 2 (max 81) | 47.2, 48.9, 59.5 | 128 / 61.4 | app desktop: LCP 636 ms (> 400); app desktop: live update 3: a task took 59.5 ms (> 50 ms) |
| chart | phone | 1968.4 | 2 | 804 | 1436 | 0.002 | 526 | 313.6 / 5 / 1203.2 | 6 (max 312) | 11.4, 12.8, 11.8 | 40 / 55.6 | chart phone: trace load task 313.6 ms (> 200); chart phone: observer load task 312 ms (> 200); chart phone: LCP 1436 ms (> 1200) |
| chart | desktop | 750.7 | 2 | 292 | 640 | 0.0025 | 1842 | 66.7 / 1 / 542.6 | 2 (max 66) | 6.4, 7.5, 6.7 | 24 / 34.1 | chart desktop: LCP 640 ms (> 400) |
| overview | phone | 1341.5 | 1 | 832 | 1492 | 0.0088 | 2064 | 135.2 / 3 / 559.3 | 6 (max 239) | 41.2, 37.8, 44.4 | 152 / 153.1 | overview phone: observer load task 239 ms (> 200); overview phone: LCP 1492 ms (> 1200) |
| overview | desktop | 613 | 1 | 344 | 704 | 0.0027 | 5834 | 44.5 / 0 / 320.9 | 1 (max 62) | 30.6, 15, 11.2 | 72 / 77 | overview desktop: LCP 704 ms (> 400) |
| history | phone | 1419.3 | 8 | 716 | 1132 | 0.0059 | 676 | 359.6 / 5 / 1382.4 | 7 (max 359) | 101.6, 103, 105.1 | 192 / 108.8 | history phone: trace load task 359.6 ms (> 200); history phone: observer load task 359 ms (> 200); history phone: live update 1: a task took 101.6 ms (> 50 ms); history phone: live update 2: a task took 103 ms (> 50 ms); history phone: live update 3: a task took 105.1 ms (> 50 ms) |
| history | desktop | 652.7 | 8 | 268 | 604 | 0.0044 | 2122 | 58.4 / 1 / 706.5 | 1 (max 58) | 44.8, 47, 42.5 | 32 / 23.5 | history desktop: LCP 604 ms (> 400) |
| host | phone | 1185.2 | 1 | 712 | 1304 | 0.002 | 464 | 138 / 2 / 519.4 | 3 (max 167) | 22.9, 14.8, 17.1 | 72 / 29.9 | host phone: LCP 1304 ms (> 1200) |
| host | desktop | 546.2 | 1 | 260 | 644 | 0.0026 | 1768 | 43.2 / 0 / 276 | 1 (max 52) | 17.2, 6.2, 10.6 | 40 / 14.6 | host desktop: LCP 644 ms (> 400) |
| board | phone | 1257.6 | 40 | 728 | 1600 | 0.0575 | 815 | 96.5 / 2 / 419.1 | 4 (max 189) | 28.3, 27.1, 21.3 | 72 / 277.3 | board phone: LCP 1600 ms (> 1200) |
| board | desktop | 618.5 | 40 | 352 | 692 | 0.0025 | 1808 | 44.3 / 0 / 471.2 | 1 (max 72) | 19.5, 12.5, 8 | 80 / 52.2 | board desktop: LCP 692 ms (> 400) |
| logs | phone | 1550.5 | 13 | 748 | 1776 | 0.028 | 347 | 129.5 / 5 / 830.7 | 6 (max 176) | 38.1, 28.8, 32.2 | 120 / 80.1 | logs phone: LCP 1776 ms (> 1200) |
| logs | desktop | 682.1 | 21 | 300 | 784 | 0.0091 | 1687 | 53.2 / 1 / 545 | 2 (max 53) | 31.3, 10.9, 35.1 | 120 / 47.7 | logs desktop: LCP 784 ms (> 400) |
| settings | phone | 1316.7 | 0 | 828 | 1380 | 0.002 | 342 | 99.4 / 1 / 344.1 | 3 (max 221) | 52.1, 28.2, 23.9 | 64 / 36.9 | settings phone: observer load task 221 ms (> 200); settings phone: LCP 1380 ms (> 1200); settings phone: live update 1: a task took 52.1 ms (> 50 ms) |
| settings | desktop | 556.3 | 0 | 292 | 592 | 0.0025 | 1646 | 21.7 / 0 / 371.8 | 1 (max 57) | 28.6, 9.7, 10.2 | 32 / 20.1 | settings desktop: LCP 592 ms (> 400) |

Budget violations:
- app phone: LCP 1536 ms (> 1200)
- app desktop: LCP 636 ms (> 400)
- app desktop: live update 3: a task took 59.5 ms (> 50 ms)
- chart phone: trace load task 313.6 ms (> 200)
- chart phone: observer load task 312 ms (> 200)
- chart phone: LCP 1436 ms (> 1200)
- chart desktop: LCP 640 ms (> 400)
- overview phone: observer load task 239 ms (> 200)
- overview phone: LCP 1492 ms (> 1200)
- overview desktop: LCP 704 ms (> 400)
- history phone: trace load task 359.6 ms (> 200)
- history phone: observer load task 359 ms (> 200)
- history phone: live update 1: a task took 101.6 ms (> 50 ms)
- history phone: live update 2: a task took 103 ms (> 50 ms)
- history phone: live update 3: a task took 105.1 ms (> 50 ms)
- history desktop: LCP 604 ms (> 400)
- host phone: LCP 1304 ms (> 1200)
- host desktop: LCP 644 ms (> 400)
- board phone: LCP 1600 ms (> 1200)
- board desktop: LCP 692 ms (> 400)
- logs phone: LCP 1776 ms (> 1200)
- logs desktop: LCP 784 ms (> 400)
- settings phone: observer load task 221 ms (> 200)
- settings phone: LCP 1380 ms (> 1200)
- settings phone: live update 1: a task took 52.1 ms (> 50 ms)
- settings desktop: LCP 592 ms (> 400)
