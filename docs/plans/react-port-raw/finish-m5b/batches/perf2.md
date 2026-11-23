# perf-check 2026-11-22T03:39:04.094Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 3090.1 | 150 | 828 | 1468 | 0.0087 | 2059 | 182.4 / 3 / 2216.4 | 2 (max 183) | 26, 24.1, 19.6 | 120 / 83.7 | app phone: LCP 1468 ms (> 1200) |
| app | desktop | 1722.1 | 150 | 352 | 628 | 0.0177 | 3550 | 53.2 / 1 / 1726.4 | 1 (max 69) | 59.4, 56.5, 40.1 | 128 / 65.3 | app desktop: LCP 628 ms (> 400); app desktop: live update 1: a task took 59.4 ms (> 50 ms); app desktop: live update 2: a task took 56.5 ms (> 50 ms) |
| chart | phone | 1973.4 | 2 | 748 | 1348 | 0.002 | 526 | 352 / 5 / 1310 | 6 (max 352) | 16.5, 15.5, 16.7 | 40 / 64.6 | chart phone: trace load task 352 ms (> 200); chart phone: observer load task 352 ms (> 200); chart phone: LCP 1348 ms (> 1200) |
| chart | desktop | 778.9 | 2 | 324 | 668 | 0.0025 | 1842 | 61.3 / 1 / 557.6 | 2 (max 67) | 6.4, 6.5, 13.3 | 16 / 36.4 | chart desktop: LCP 668 ms (> 400) |
| overview | phone | 1289.5 | 1 | 748 | 1488 | 0.0088 | 1466 | 173.8 / 3 / 601.4 | 5 (max 195) | 15.8, 20.1, 18.3 | 152 / 149.5 | overview phone: LCP 1488 ms (> 1200) |
| overview | desktop | 614.6 | 1 | 356 | 684 | 0.0027 | 5295 | 35.2 / 0 / 461.3 | 2 (max 57) | 12.6, 7.7, 9.3 | 72 / 44.1 | overview desktop: LCP 684 ms (> 400) |
| history | phone | 1380.6 | 8 | 704 | 1116 | 0.0059 | 676 | 308.6 / 6 / 1326.1 | 7 (max 308) | 112.1, 100.1, 88.5 | 208 / 134.8 | history phone: trace load task 308.6 ms (> 200); history phone: observer load task 308 ms (> 200); history phone: INP 208 ms (> 200); history phone: live update 1: a task took 112.1 ms (> 50 ms); history phone: live update 2: a task took 100.1 ms (> 50 ms); history phone: live update 3: a task took  |
| history | desktop | 622.4 | 8 | 288 | 596 | 0.0025 | 2122 | 61.6 / 1 / 621.8 | 1 (max 61) | 43.2, 32.4, 39.1 | 24 / 17.9 | history desktop: LCP 596 ms (> 400) |
| host | phone | 1195.8 | 1 | 704 | 1348 | 0.002 | 464 | 147.4 / 2 / 550.6 | 3 (max 176) | 24.1, 22.6, 14.5 | 56 / 20.6 | host phone: LCP 1348 ms (> 1200) |
| host | desktop | 554.2 | 1 | 256 | 628 | 0.0238 | 1768 | 30 / 0 / 448.3 | 0 (max 0) | 8.1, 7.6, 7.6 | 40 / 9.7 | host desktop: LCP 628 ms (> 400) |
| board | phone | 1245.4 | 40 | 716 | 1580 | 0.0575 | 815 | 88.8 / 2 / 390.8 | 4 (max 190) | 26.3, 19.3, 20.8 | 64 / 337.1 | board phone: LCP 1580 ms (> 1200) |
| board | desktop | 647.9 | 40 | 360 | 712 | 0.0025 | 1808 | 40.3 / 0 / 475.7 | 1 (max 80) | 18.4, 7.2, 11.8 | 88 / 64.9 | board desktop: LCP 712 ms (> 400) |
| logs | phone | 1567.8 | 13 | 728 | 1768 | 0.028 | 335 | 150.4 / 3 / 866.8 | 4 (max 191) | 28.3, 25.4, 27.6 | 144 / 105.7 | logs phone: LCP 1768 ms (> 1200) |
| logs | desktop | 672.9 | 22 | 284 | 768 | 0.0124 | 1668 | 57.1 / 1 / 428.8 | 1 (max 57) | 9.2, 34, 38.5 | 168 / 83.7 | logs desktop: LCP 768 ms (> 400) |
| settings | phone | 1320.2 | 0 | 776 | 1384 | 0.002 | 342 | 113.6 / 2 / 370.4 | 3 (max 206) | 32.9, 26.5, 20.7 | 56 / 38.5 | settings phone: observer load task 206 ms (> 200); settings phone: LCP 1384 ms (> 1200) |
| settings | desktop | 560.7 | 0 | 308 | 592 | 0.0025 | 1646 | 20.7 / 0 / 311.2 | 1 (max 53) | 11.1, 10.7, 12 | 48 / 31.2 | settings desktop: LCP 592 ms (> 400) |

Budget violations:
- app phone: LCP 1468 ms (> 1200)
- app desktop: LCP 628 ms (> 400)
- app desktop: live update 1: a task took 59.4 ms (> 50 ms)
- app desktop: live update 2: a task took 56.5 ms (> 50 ms)
- chart phone: trace load task 352 ms (> 200)
- chart phone: observer load task 352 ms (> 200)
- chart phone: LCP 1348 ms (> 1200)
- chart desktop: LCP 668 ms (> 400)
- overview phone: LCP 1488 ms (> 1200)
- overview desktop: LCP 684 ms (> 400)
- history phone: trace load task 308.6 ms (> 200)
- history phone: observer load task 308 ms (> 200)
- history phone: INP 208 ms (> 200)
- history phone: live update 1: a task took 112.1 ms (> 50 ms)
- history phone: live update 2: a task took 100.1 ms (> 50 ms)
- history phone: live update 3: a task took 88.5 ms (> 50 ms)
- history desktop: LCP 596 ms (> 400)
- host phone: LCP 1348 ms (> 1200)
- host desktop: LCP 628 ms (> 400)
- board phone: LCP 1580 ms (> 1200)
- board desktop: LCP 712 ms (> 400)
- logs phone: LCP 1768 ms (> 1200)
- logs desktop: LCP 768 ms (> 400)
- settings phone: observer load task 206 ms (> 200)
- settings phone: LCP 1384 ms (> 1200)
- settings desktop: LCP 592 ms (> 400)
