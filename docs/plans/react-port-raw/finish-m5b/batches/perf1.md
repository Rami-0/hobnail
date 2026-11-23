# perf-check 2026-11-22T03:34:50.141Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 3114.9 | 150 | 832 | 1504 | 0.0087 | 2023 | 180.1 / 4 / 2011.2 | 5 (max 182) | 34.8, 21, 22.2 | 112 / 90.3 | app phone: LCP 1504 ms (> 1200) |
| app | desktop | 1760.9 | 150 | 344 | 644 | 0.0177 | 3458 | 72 / 6 / 1751.6 | 1 (max 88) | 48.1, 55.9, 54.9 | 128 / 70.5 | app desktop: LCP 644 ms (> 400); app desktop: live update 2: a task took 55.9 ms (> 50 ms); app desktop: live update 3: a task took 54.9 ms (> 50 ms) |
| chart | phone | 1976.4 | 2 | 808 | 1472 | 0.002 | 528 | 297.9 / 5 / 1202.7 | 6 (max 297) | 14.4, 12.9, 12.2 | 48 / 65.4 | chart phone: trace load task 297.9 ms (> 200); chart phone: observer load task 297 ms (> 200); chart phone: LCP 1472 ms (> 1200) |
| chart | desktop | 762.1 | 2 | 280 | 648 | 0.0025 | 1844 | 63.5 / 1 / 419.5 | 1 (max 63) | 12.3, 5.7, 6.2 | 24 / 44.2 | chart desktop: LCP 648 ms (> 400) |
| overview | phone | 1225.3 | 1 | 668 | 1368 | 0.0088 | 1768 | 118.6 / 3 / 538 | 5 (max 178) | 18.8, 18.8, 23.2 | 136 / 151.2 | overview phone: LCP 1368 ms (> 1200) |
| overview | desktop | 613.5 | 1 | 368 | 724 | 0.0028 | 5295 | 53.7 / 1 / 534.1 | 3 (max 56) | 10.6, 7.6, 7.2 | 72 / 43.5 | overview desktop: LCP 724 ms (> 400) |
| history | phone | 1459.4 | 8 | 712 | 1176 | 0.0059 | 676 | 300.9 / 6 / 1368 | 8 (max 300) | 99.8, 108.9, 89.5 | 176 / 116.2 | history phone: trace load task 300.9 ms (> 200); history phone: observer load task 300 ms (> 200); history phone: live update 1: a task took 99.8 ms (> 50 ms); history phone: live update 2: a task took 108.9 ms (> 50 ms); history phone: live update 3: a task took 89.5 ms (> 50 ms) |
| history | desktop | 617.3 | 8 | 284 | 592 | 0.0025 | 2122 | 63.4 / 1 / 613 | 2 (max 63) | 41, 35.9, 46.9 | 32 / 15.7 | history desktop: LCP 592 ms (> 400) |
| host | phone | 1270.6 | 1 | 820 | 1404 | 0.002 | 464 | 126.1 / 2 / 498.7 | 3 (max 208) | 18.1, 16.6, 16.5 | 72 / 26.7 | host phone: observer load task 208 ms (> 200); host phone: LCP 1404 ms (> 1200) |
| host | desktop | 530.3 | 1 | 248 | 608 | 0.0025 | 1768 | 31.9 / 0 / 396.6 | 0 (max 0) | 13.7, 18.9, 7.3 | 40 / 11.9 | host desktop: LCP 608 ms (> 400) |
| board | phone | 1296.9 | 40 | 744 | 1632 | 0.0575 | 815 | 87.7 / 2 / 406.8 | 4 (max 212) | 26.1, 21, 21.9 | 56 / 276.6 | board phone: observer load task 212 ms (> 200); board phone: LCP 1632 ms (> 1200) |
| board | desktop | 672.1 | 40 | 384 | 740 | 0.0025 | 1808 | 43.1 / 0 / 450.3 | 1 (max 80) | 17, 43.2, 10.9 | 80 / 52.5 | board desktop: LCP 740 ms (> 400) |
| logs | phone | 1546.1 | 13 | 744 | 1740 | 0.028 | 335 | 124.9 / 4 / 801.3 | 5 (max 174) | 33.8, 25.4, 23.5 | 112 / 79.8 | logs phone: LCP 1740 ms (> 1200) |
| logs | desktop | 679 | 22 | 272 | 788 | 0.0124 | 1668 | 56.8 / 1 / 452.7 | 2 (max 56) | 21.8, 9.7, 9.4 | 104 / 51.4 | logs desktop: LCP 788 ms (> 400) |
| settings | phone | 1272.1 | 0 | 776 | 1332 | 0.002 | 342 | 99.1 / 1 / 350.6 | 3 (max 221) | 29.4, 28, 21.3 | 56 / 34.4 | settings phone: observer load task 221 ms (> 200); settings phone: LCP 1332 ms (> 1200) |
| settings | desktop | 581.2 | 0 | 308 | 616 | 0.0025 | 1646 | 20.5 / 0 / 374.2 | 2 (max 81) | 13.7, 10.9, 10.7 | 80 / 38.5 | settings desktop: LCP 616 ms (> 400) |

Budget violations:
- app phone: LCP 1504 ms (> 1200)
- app desktop: LCP 644 ms (> 400)
- app desktop: live update 2: a task took 55.9 ms (> 50 ms)
- app desktop: live update 3: a task took 54.9 ms (> 50 ms)
- chart phone: trace load task 297.9 ms (> 200)
- chart phone: observer load task 297 ms (> 200)
- chart phone: LCP 1472 ms (> 1200)
- chart desktop: LCP 648 ms (> 400)
- overview phone: LCP 1368 ms (> 1200)
- overview desktop: LCP 724 ms (> 400)
- history phone: trace load task 300.9 ms (> 200)
- history phone: observer load task 300 ms (> 200)
- history phone: live update 1: a task took 99.8 ms (> 50 ms)
- history phone: live update 2: a task took 108.9 ms (> 50 ms)
- history phone: live update 3: a task took 89.5 ms (> 50 ms)
- history desktop: LCP 592 ms (> 400)
- host phone: observer load task 208 ms (> 200)
- host phone: LCP 1404 ms (> 1200)
- host desktop: LCP 608 ms (> 400)
- board phone: observer load task 212 ms (> 200)
- board phone: LCP 1632 ms (> 1200)
- board desktop: LCP 740 ms (> 400)
- logs phone: LCP 1740 ms (> 1200)
- logs desktop: LCP 788 ms (> 400)
- settings phone: observer load task 221 ms (> 200)
- settings phone: LCP 1332 ms (> 1200)
- settings desktop: LCP 616 ms (> 400)
