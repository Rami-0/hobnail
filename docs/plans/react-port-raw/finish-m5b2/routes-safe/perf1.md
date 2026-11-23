# perf-check 2026-11-22T04:37:44.514Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 4273.2 | 150 | 1292 | 2100 | 0.0087 | 2026 | 316.3 / 4 / 2138.3 | 7 (max 354) | 38.9, 21.8, 22.5 | 112 / 79.5 | app phone: trace load task 316.3 ms (> 200); app phone: observer load task 354 ms (> 200); app phone: LCP 2100 ms (> 1200) |
| app | desktop | 1727 | 150 | 352 | 476 | 0.0177 | 3461 | 81.6 / 3 / 1633.1 | 3 (max 81) | 43.6, 57.3, 45 | 136 / 61.9 | app desktop: LCP 476 ms (> 400); app desktop: live update 2: a task took 57.3 ms (> 50 ms) |
| chart | phone | 2003.2 | 2 | 812 | 1396 | 0.002 | 544 | 376.6 / 4 / 1242.9 | 5 (max 376) | 21, 12.8, 14.9 | 104 / 79.2 | chart phone: trace load task 376.6 ms (> 200); chart phone: observer load task 376 ms (> 200); chart phone: LCP 1396 ms (> 1200) |
| chart | desktop | 699.2 | 2 | 292 | 504 | 0.0026 | 1848 | 66.7 / 1 / 433.5 | 2 (max 66) | 5.9, 6, 6 | 16 / 28.3 | chart desktop: LCP 504 ms (> 400) |
| overview | phone | 1151.4 | 1 | 764 | 1304 | 0.0088 | 2071 | 135 / 2 / 445.9 | 5 (max 218) | 22, 17.6, 21.4 | 144 / 128.9 | overview phone: observer load task 218 ms (> 200); overview phone: LCP 1304 ms (> 1200) |
| overview | desktop | 441 | 1 | 328 | 592 | 0.0027 | 5600 | 56.8 / 1 / 477.3 | 2 (max 63) | 10.5, 11.6, 24.2 | 80 / 47.2 | overview desktop: LCP 592 ms (> 400) |
| history | phone | 1349.9 | 8 | 684 | 828 | 0.0059 | 682 | 300.7 / 6 / 1251 | 7 (max 299) | 108.8, 99.7, 86.9 | 200 / 116.4 | history phone: trace load task 300.7 ms (> 200); history phone: observer load task 299 ms (> 200); history phone: live update 1: a task took 108.8 ms (> 50 ms); history phone: live update 2: a task took 99.7 ms (> 50 ms); history phone: live update 3: a task took 86.9 ms (> 50 ms) |
| history | desktop | 562.6 | 8 | 268 | 420 | 0.0044 | 2130 | 63.7 / 1 / 712.9 | 2 (max 63) | 49.4, 40.6, 45 | 40 / 11.1 | history desktop: LCP 420 ms (> 400) |
| host | phone | 1168.6 | 1 | 732 | 1320 | 0.002 | 467 | 135.7 / 2 / 442.3 | 3 (max 165) | 22.5, 18.4, 20.7 | 64 / 29 | host phone: LCP 1320 ms (> 1200) |
| host | desktop | 326.4 | 1 | 236 | 428 | 0.0026 | 1771 | 34.6 / 0 / 299.1 | 0 (max 0) | 13, 15.2, 5 | 48 / 13.1 | host desktop: LCP 428 ms (> 400) |
| board | phone | 1265.6 | 40 | 724 | 1600 | 0.0575 | 816 | 84 / 2 / 406.9 | 4 (max 190) | 27.3, 19.6, 20.8 | 64 / 298.8 | board phone: LCP 1600 ms (> 1200) |
| board | desktop | 447.9 | 40 | 308 | 516 | 0.0026 | 1808 | 36 / 0 / 358 | 1 (max 72) | 21.1, 11.3, 20.6 | 56 / 30.5 | board desktop: LCP 516 ms (> 400) |
| logs | phone | 1529.7 | 13 | 800 | 1748 | 0.0366 | 337 | 125.3 / 3 / 803.3 | 5 (max 192) | 23.4, 23.7, 32.1 | 112 / 77 | logs phone: LCP 1748 ms (> 1200) |
| logs | desktop | 536.5 | 22 | 292 | 680 | 0.0124 | 1670 | 43.2 / 0 / 330.2 | 2 (max 57) | 36.6, 33.5, 12 | 96 / 52.2 | logs desktop: LCP 680 ms (> 400) |
| settings | phone | - | 0 | 800 | 800 | 0.002 | 60 | 224.1 / 4 / 1367.4 | 3 (max 222) | none | - / 1.4 | not ready within 30000 ms (!!document.querySelector('input[name="greeting"]')); settings phone: not ready within 30000 ms (!!document.querySelector('input[name="greeting"]')); settings phone: trace load task 224.1 ms (> 200); settings phone: observer load task 222 ms (> 200); settings phone: missing |
| settings | desktop | - | 0 | 300 | 300 | 0.0025 | 60 | 63.7 / 1 / 584 | 1 (max 63) | none | - / 0.3 | not ready within 30000 ms (!!document.querySelector('input[name="greeting"]')); settings desktop: not ready within 30000 ms (!!document.querySelector('input[name="greeting"]')); settings desktop: missing INP measurement |

Budget violations:
- app phone: trace load task 316.3 ms (> 200)
- app phone: observer load task 354 ms (> 200)
- app phone: LCP 2100 ms (> 1200)
- app desktop: LCP 476 ms (> 400)
- app desktop: live update 2: a task took 57.3 ms (> 50 ms)
- chart phone: trace load task 376.6 ms (> 200)
- chart phone: observer load task 376 ms (> 200)
- chart phone: LCP 1396 ms (> 1200)
- chart desktop: LCP 504 ms (> 400)
- overview phone: observer load task 218 ms (> 200)
- overview phone: LCP 1304 ms (> 1200)
- overview desktop: LCP 592 ms (> 400)
- history phone: trace load task 300.7 ms (> 200)
- history phone: observer load task 299 ms (> 200)
- history phone: live update 1: a task took 108.8 ms (> 50 ms)
- history phone: live update 2: a task took 99.7 ms (> 50 ms)
- history phone: live update 3: a task took 86.9 ms (> 50 ms)
- history desktop: LCP 420 ms (> 400)
- host phone: LCP 1320 ms (> 1200)
- host desktop: LCP 428 ms (> 400)
- board phone: LCP 1600 ms (> 1200)
- board desktop: LCP 516 ms (> 400)
- logs phone: LCP 1748 ms (> 1200)
- logs desktop: LCP 680 ms (> 400)
- settings phone: not ready within 30000 ms (!!document.querySelector('input[name="greeting"]'))
- settings phone: trace load task 224.1 ms (> 200)
- settings phone: observer load task 222 ms (> 200)
- settings phone: missing INP measurement
- settings desktop: not ready within 30000 ms (!!document.querySelector('input[name="greeting"]'))
- settings desktop: missing INP measurement
