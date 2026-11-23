# perf-check 2026-11-22T04:42:37.344Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 2945.9 | 150 | 844 | 1136 | 0.0087 | 2062 | 101.1 / 2 / 1292.2 | 5 (max 184) | 18.2, 17.8, 23.1 | 104 / 75.2 |  |
| app | desktop | 1686.9 | 150 | 332 | 476 | 0.0177 | 3553 | 83.3 / 4 / 1629.3 | 3 (max 83) | 70, 46.4, 56 | 128 / 65.7 | app desktop: LCP 476 ms (> 400); app desktop: live update 1: a task took 70 ms (> 50 ms); app desktop: live update 3: a task took 56 ms (> 50 ms) |
| chart | phone | 1928.9 | 2 | 808 | 1392 | 0.002 | 530 | 310.8 / 4 / 1187 | 5 (max 310) | 12.6, 13.3, 11.3 | 48 / 64.3 | chart phone: trace load task 310.8 ms (> 200); chart phone: observer load task 310 ms (> 200); chart phone: LCP 1392 ms (> 1200) |
| chart | desktop | 719.4 | 2 | 288 | 540 | 0.0025 | 1846 | 69.2 / 1 / 566.1 | 2 (max 69) | 6.9, 4.8, 17.1 | - / 27 | chart desktop: LCP 540 ms (> 400); chart desktop: missing INP measurement |
| overview | phone | 1203.8 | 1 | 796 | 1364 | 0.0088 | 1775 | 142.7 / 2 / 506.5 | 5 (max 235) | 18.7, 22, 24.9 | 152 / 173.1 | overview phone: observer load task 235 ms (> 200); overview phone: LCP 1364 ms (> 1200) |
| overview | desktop | 478.5 | 1 | 372 | 584 | 0.0027 | 5841 | 42.9 / 0 / 315.1 | 1 (max 58) | 7, 5.7, 7.2 | 64 / 47.8 | overview desktop: LCP 584 ms (> 400) |
| history | phone | 1309.2 | 8 | 648 | 848 | 0.0059 | 677 | 281.8 / 5 / 1253 | 7 (max 281) | 94.4, 103, 99.5 | 184 / 100.3 | history phone: trace load task 281.8 ms (> 200); history phone: observer load task 281 ms (> 200); history phone: live update 1: a task took 94.4 ms (> 50 ms); history phone: live update 2: a task took 103 ms (> 50 ms); history phone: live update 3: a task took 99.5 ms (> 50 ms) |
| history | desktop | 524.5 | 8 | 288 | 412 | 0.0025 | 2123 | 59.1 / 1 / 732.7 | 2 (max 59) | 42.7, 32.2, 30.4 | 16 / 25.4 | history desktop: LCP 412 ms (> 400) |
| host | phone | 1214.7 | 1 | 680 | 1332 | 0.002 | 467 | 196.2 / 2 / 469.8 | 3 (max 196) | 20.8, 20.9, 19.9 | 80 / 34.5 | host phone: LCP 1332 ms (> 1200) |
| host | desktop | 403.7 | 1 | 276 | 528 | 0.0238 | 1771 | 78.4 / 2 / 314.9 | 2 (max 78) | 10.8, 24.1, 17 | 48 / 5.7 | host desktop: LCP 528 ms (> 400) |
| board | phone | 1223.4 | 40 | 728 | 1556 | 0.0575 | 816 | 79.4 / 2 / 372.3 | 4 (max 182) | 28.7, 22.2, 22.2 | 56 / 287.9 | board phone: LCP 1556 ms (> 1200) |
| board | desktop | 446.6 | 40 | 332 | 552 | 0.0025 | 1808 | 59.2 / 1 / 361 | 2 (max 94) | 12, 15.1, 7.8 | 88 / 51.7 | board desktop: LCP 552 ms (> 400) |
| logs | phone | 1506.4 | 13 | 680 | 1708 | 0.028 | 337 | 142.6 / 3 / 789 | 5 (max 191) | 32.4, 29.7, 25.1 | 152 / 123.4 | logs phone: LCP 1708 ms (> 1200) |
| logs | desktop | 534 | 22 | 280 | 640 | 0.0027 | 1670 | 37.9 / 0 / 318.2 | 1 (max 56) | 11.9, 20, 10.6 | 104 / 52.4 | logs desktop: LCP 640 ms (> 400) |
| settings | phone | - | 0 | 788 | 788 | 0.002 | 60 | 225.4 / 4 / 1369.4 | 3 (max 217) | none | - / 1 | not ready within 30000 ms (!!document.querySelector('input[name="greeting"]')); settings phone: not ready within 30000 ms (!!document.querySelector('input[name="greeting"]')); settings phone: trace load task 225.4 ms (> 200); settings phone: observer load task 217 ms (> 200); settings phone: missing |
| settings | desktop | - | 0 | 300 | 300 | 0.0025 | 60 | 76.1 / 1 / 603.1 | 1 (max 76) | none | - / 0.5 | not ready within 30000 ms (!!document.querySelector('input[name="greeting"]')); settings desktop: not ready within 30000 ms (!!document.querySelector('input[name="greeting"]')); settings desktop: missing INP measurement |

Budget violations:
- app desktop: LCP 476 ms (> 400)
- app desktop: live update 1: a task took 70 ms (> 50 ms)
- app desktop: live update 3: a task took 56 ms (> 50 ms)
- chart phone: trace load task 310.8 ms (> 200)
- chart phone: observer load task 310 ms (> 200)
- chart phone: LCP 1392 ms (> 1200)
- chart desktop: LCP 540 ms (> 400)
- chart desktop: missing INP measurement
- overview phone: observer load task 235 ms (> 200)
- overview phone: LCP 1364 ms (> 1200)
- overview desktop: LCP 584 ms (> 400)
- history phone: trace load task 281.8 ms (> 200)
- history phone: observer load task 281 ms (> 200)
- history phone: live update 1: a task took 94.4 ms (> 50 ms)
- history phone: live update 2: a task took 103 ms (> 50 ms)
- history phone: live update 3: a task took 99.5 ms (> 50 ms)
- history desktop: LCP 412 ms (> 400)
- host phone: LCP 1332 ms (> 1200)
- host desktop: LCP 528 ms (> 400)
- board phone: LCP 1556 ms (> 1200)
- board desktop: LCP 552 ms (> 400)
- logs phone: LCP 1708 ms (> 1200)
- logs desktop: LCP 640 ms (> 400)
- settings phone: not ready within 30000 ms (!!document.querySelector('input[name="greeting"]'))
- settings phone: trace load task 225.4 ms (> 200)
- settings phone: observer load task 217 ms (> 200)
- settings phone: missing INP measurement
- settings desktop: not ready within 30000 ms (!!document.querySelector('input[name="greeting"]'))
- settings desktop: missing INP measurement
