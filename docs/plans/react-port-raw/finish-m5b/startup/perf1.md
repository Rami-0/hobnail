# perf-check 2026-11-22T03:26:19.150Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 3098.6 | 150 | 832 | 1544 | 0.0087 | 2023 | 172.5 / 3 / 1998.1 | 4 (max 190) | 32.4, 19.1, 23.3 | 104 / 100.3 | app phone: LCP 1544 ms (> 1200) |
| app | desktop | 1709.9 | 150 | 364 | 664 | 0.0177 | 3458 | 60.9 / 3 / 1719.1 | 1 (max 78) | 44.8, 66.2, 50.1 | 136 / 62.5 | app desktop: LCP 664 ms (> 400); app desktop: live update 2: a task took 66.2 ms (> 50 ms); app desktop: live update 3: a task took 50.1 ms (> 50 ms) |
| chart | phone | 1906.6 | 2 | 784 | 1372 | 0.002 | 528 | 327.2 / 4 / 1171.3 | 5 (max 327) | 18.1, 15, 11.9 | 40 / 61.4 | chart phone: trace load task 327.2 ms (> 200); chart phone: observer load task 327 ms (> 200); chart phone: LCP 1372 ms (> 1200) |
| chart | desktop | 826.1 | 2 | 344 | 700 | 0.0025 | 1844 | 64.2 / 1 / 563.4 | 2 (max 64) | 7.9, 7.3, 7.4 | 24 / 52.7 | chart desktop: LCP 700 ms (> 400) |
| overview | phone | - | 0 | 744 | 1572 | 0.002 | 598 | 189.7 / 7 / 2556.4 | 5 (max 190) | none | - / 1.9 | not ready within 30000 ms (!!document.querySelector('[data-testid="overview"] .overview-counts strong') && !document.querySelector('[data-testid="overview"] [aria-busy="true"]')); overview phone: not ready within 30000 ms (!!document.querySelector('[data-testid="overview"] .overview-counts strong')  |
| overview | desktop | - | 0 | 252 | 644 | 0.0025 | 1902 | 70.8 / 1 / 2339.9 | 1 (max 70) | none | - / 0.5 | not ready within 30000 ms (!!document.querySelector('[data-testid="overview"] .overview-counts strong') && !document.querySelector('[data-testid="overview"] [aria-busy="true"]')); overview desktop: not ready within 30000 ms (!!document.querySelector('[data-testid="overview"] .overview-counts strong' |
| history | phone | 1388.1 | 8 | 684 | 1072 | 0.0059 | 676 | 321.4 / 5 / 1409.6 | 7 (max 321) | 116.2, 93.5, 86.7 | 80 / 112.5 | history phone: trace load task 321.4 ms (> 200); history phone: observer load task 321 ms (> 200); history phone: live update 1: a task took 116.2 ms (> 50 ms); history phone: live update 2: a task took 93.5 ms (> 50 ms); history phone: live update 3: a task took 86.7 ms (> 50 ms) |
| history | desktop | 680.8 | 8 | 340 | 632 | 0.0044 | 2122 | 60.5 / 1 / 663.9 | 2 (max 60) | 43.8, 41.3, 48.8 | 32 / 24.6 | history desktop: LCP 632 ms (> 400) |
| host | phone | 1212 | 1 | 692 | 1328 | 0.002 | 464 | 166.6 / 2 / 504 | 3 (max 168) | 17.7, 17.8, 14 | 64 / 25.4 | host phone: LCP 1328 ms (> 1200) |
| host | desktop | 529.9 | 1 | 244 | 608 | 0.0238 | 1768 | 35.4 / 0 / 429.2 | 0 (max 0) | 6.1, 9, 5.5 | 48 / 22.5 | host desktop: LCP 608 ms (> 400) |
| board | phone | 1266.3 | 40 | 740 | 1612 | 0.0575 | 815 | 79.8 / 2 / 392.5 | 4 (max 214) | 27.6, 18, 23.5 | 72 / 298.8 | board phone: observer load task 214 ms (> 200); board phone: LCP 1612 ms (> 1200) |
| board | desktop | 619.8 | 40 | 364 | 684 | 0.0025 | 1808 | 39.4 / 0 / 465.2 | 2 (max 74) | 18.3, 11.9, 11.1 | 72 / 49.3 | board desktop: LCP 684 ms (> 400) |
| logs | phone | 1641.3 | 13 | 776 | 1852 | 0.0366 | 347 | 150 / 4 / 880 | 5 (max 202) | 33.3, 35.2, 26 | 120 / 82.1 | logs phone: observer load task 202 ms (> 200); logs phone: LCP 1852 ms (> 1200) |
| logs | desktop | 661.3 | 21 | 300 | 760 | 0.0092 | 1687 | 45.2 / 0 / 416.5 | 1 (max 52) | 35.8, 34, 12.2 | 88 / 51.8 | logs desktop: LCP 760 ms (> 400) |
| settings | phone | 1447.6 | 0 | 888 | 1528 | 0.002 | 342 | 141.9 / 1 / 404.1 | 3 (max 292) | 45.4, 26.7, 22.3 | 64 / 36.5 | settings phone: observer load task 292 ms (> 200); settings phone: LCP 1528 ms (> 1200) |
| settings | desktop | 555.4 | 0 | 296 | 604 | 0.0025 | 1646 | 22.8 / 0 / 361.7 | 1 (max 51) | 14, 10.7, 15.3 | 32 / 17.9 | settings desktop: LCP 604 ms (> 400) |

Budget violations:
- app phone: LCP 1544 ms (> 1200)
- app desktop: LCP 664 ms (> 400)
- app desktop: live update 2: a task took 66.2 ms (> 50 ms)
- app desktop: live update 3: a task took 50.1 ms (> 50 ms)
- chart phone: trace load task 327.2 ms (> 200)
- chart phone: observer load task 327 ms (> 200)
- chart phone: LCP 1372 ms (> 1200)
- chart desktop: LCP 700 ms (> 400)
- overview phone: not ready within 30000 ms (!!document.querySelector('[data-testid="overview"] .overview-counts strong') && !document.querySelector('[data-testid="overview"] [aria-busy="true"]'))
- overview phone: LCP 1572 ms (> 1200)
- overview phone: missing INP measurement
- overview desktop: not ready within 30000 ms (!!document.querySelector('[data-testid="overview"] .overview-counts strong') && !document.querySelector('[data-testid="overview"] [aria-busy="true"]'))
- overview desktop: LCP 644 ms (> 400)
- overview desktop: missing INP measurement
- history phone: trace load task 321.4 ms (> 200)
- history phone: observer load task 321 ms (> 200)
- history phone: live update 1: a task took 116.2 ms (> 50 ms)
- history phone: live update 2: a task took 93.5 ms (> 50 ms)
- history phone: live update 3: a task took 86.7 ms (> 50 ms)
- history desktop: LCP 632 ms (> 400)
- host phone: LCP 1328 ms (> 1200)
- host desktop: LCP 608 ms (> 400)
- board phone: observer load task 214 ms (> 200)
- board phone: LCP 1612 ms (> 1200)
- board desktop: LCP 684 ms (> 400)
- logs phone: observer load task 202 ms (> 200)
- logs phone: LCP 1852 ms (> 1200)
- logs desktop: LCP 760 ms (> 400)
- settings phone: observer load task 292 ms (> 200)
- settings phone: LCP 1528 ms (> 1200)
- settings desktop: LCP 604 ms (> 400)
