# perf-check 2026-11-19T05:18:18.214Z

base http://127.0.0.1:3396, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 3417.2 | 150 | 1524 | 1524 | 0.0132 | 2073 | 160.5 / 3 / 1669.1 | 5 (max 164) | 67.7, 48, 21.7 | 120 / 98 | app phone: LCP 1524 ms (> 1200); app phone: live update 1: a task took 67.7 ms (> 50 ms) |
| app | desktop | 3140.6 | 150 | 552 | 552 | 0.0627 | 3602 | 167.9 / 21 / 3117.7 | 9 (max 118) | 53.8, 84.4, 83.2 | 192 / 92.6 | app desktop: LCP 552 ms (> 400); app desktop: live update 1: a task took 53.8 ms (> 50 ms); app desktop: live update 2: a task took 84.4 ms (> 50 ms); app desktop: live update 3: a task took 83.2 ms (> 50 ms) |
| chart | phone | 2513.8 | 2 | 1440 | 1708 | 0.002 | 542 | 429.7 / 1 / 705 | 7 (max 429) | 30.9, 31.3, 17.5 | 168 / 177.4 | chart phone: trace load task 429.7 ms (> 200); chart phone: observer load task 429 ms (> 200); chart phone: LCP 1708 ms (> 1200) |
| chart | desktop | 1262.9 | 2 | 700 | 840 | 0.0225 | 1879 | 107.6 / 3 / 425.5 | 5 (max 107) | 29.2, 17.2, 4.3 | 72 / 67.7 | chart desktop: LCP 840 ms (> 400) |
| overview | phone | 1840.8 | 2 | 936 | 1440 | 0.002 | 630 | 127 / 1 / 376.4 | 4 (max 220) | 43, 65.6, 36.8 | 128 / 597.4 | overview phone: observer load task 220 ms (> 200); overview phone: LCP 1440 ms (> 1200); overview phone: live update 2: a task took 65.6 ms (> 50 ms) |
| overview | desktop | 1424.4 | 2 | 548 | 812 | 0.0247 | 1993 | 181.6 / 2 / 605.3 | 4 (max 181) | 16.7, 16.7, 11.2 | 24 / 13.8 | overview desktop: LCP 812 ms (> 400) |
| history | phone | 4096.3 | 8 | 1540 | 1540 | 0.0047 | 452 | 714.5 / 4 / 1995.9 | 12 (max 714) | 108, 110.3, 85 | 168 / 217.8 | history phone: trace load task 714.5 ms (> 200); history phone: observer load task 714 ms (> 200); history phone: LCP 1540 ms (> 1200); history phone: live update 1: a task took 108 ms (> 50 ms); history phone: live update 2: a task took 110.3 ms (> 50 ms); history phone: live update 3: a task took  |
| history | desktop | 1262.3 | 8 | 516 | 516 | 0.0023 | 1997 | 123.2 / 3 / 700.8 | 3 (max 123) | 43.3, 38.7, 27.4 | 88 / 64.3 | history desktop: LCP 516 ms (> 400) |
| host | phone | 1431.8 | 8 | 1084 | 1496 | 0.002 | 499 | 194.1 / 3 / 1381.7 | 5 (max 194) | 24.8, 22, 14.9 | 80 / 31.3 | host phone: LCP 1496 ms (> 1200) |
| host | desktop | 487.6 | 8 | 412 | 568 | 0.0782 | 1967 | 119.5 / 2 / 810.2 | 3 (max 119) | 12.3, 21.4, 12 | 48 / 7.9 | host desktop: LCP 568 ms (> 400) |
| board | phone | 1412.5 | 40 | 1164 | 1624 | 0.0388 | 722 | 139.3 / 5 / 774.4 | 5 (max 238) | 27.4, 20.8, 25 | 64 / 324.9 | board phone: observer load task 238 ms (> 200); board phone: LCP 1624 ms (> 1200) |
| board | desktop | 614.9 | 40 | 628 | 732 | 0.0002 | 1813 | 49.2 / 0 / 651.4 | 2 (max 72) | 11.3, 7.4, 6.4 | 80 / 55.4 | board desktop: LCP 732 ms (> 400) |
| logs | phone | 1743.2 | 13 | 1216 | 2044 | 0.0032 | 344 | 259.7 / 5 / 1246.9 | 7 (max 258) | 38.7, 34.9, 35.3 | 192 / 113 | logs phone: trace load task 259.7 ms (> 200); logs phone: observer load task 258 ms (> 200); logs phone: LCP 2044 ms (> 1200) |
| logs | desktop | 737.6 | 21 | 472 | 896 | 0.0331 | 1695 | 60.1 / 1 / 502.3 | 2 (max 69) | 15.6, 18.9, 14.9 | 80 / 24.4 | logs desktop: LCP 896 ms (> 400) |
| settings | phone | 1673.9 | 0 | 1148 | 1740 | 0.002 | 258 | 110.7 / 1 / 229.2 | 3 (max 203) | 28.1, 30.5, 21.1 | 72 / 48.6 | settings phone: observer load task 203 ms (> 200); settings phone: LCP 1740 ms (> 1200) |
| settings | desktop | 730.2 | 0 | 420 | 756 | 0.0224 | 1569 | 34.9 / 0 / 260.4 | 2 (max 54) | 19.8, 14.5, 10.1 | 48 / 23.2 | settings desktop: LCP 756 ms (> 400) |

Budget violations:
- app phone: LCP 1524 ms (> 1200)
- app phone: live update 1: a task took 67.7 ms (> 50 ms)
- app desktop: LCP 552 ms (> 400)
- app desktop: live update 1: a task took 53.8 ms (> 50 ms)
- app desktop: live update 2: a task took 84.4 ms (> 50 ms)
- app desktop: live update 3: a task took 83.2 ms (> 50 ms)
- chart phone: trace load task 429.7 ms (> 200)
- chart phone: observer load task 429 ms (> 200)
- chart phone: LCP 1708 ms (> 1200)
- chart desktop: LCP 840 ms (> 400)
- overview phone: observer load task 220 ms (> 200)
- overview phone: LCP 1440 ms (> 1200)
- overview phone: live update 2: a task took 65.6 ms (> 50 ms)
- overview desktop: LCP 812 ms (> 400)
- history phone: trace load task 714.5 ms (> 200)
- history phone: observer load task 714 ms (> 200)
- history phone: LCP 1540 ms (> 1200)
- history phone: live update 1: a task took 108 ms (> 50 ms)
- history phone: live update 2: a task took 110.3 ms (> 50 ms)
- history phone: live update 3: a task took 85 ms (> 50 ms)
- history desktop: LCP 516 ms (> 400)
- host phone: LCP 1496 ms (> 1200)
- host desktop: LCP 568 ms (> 400)
- board phone: observer load task 238 ms (> 200)
- board phone: LCP 1624 ms (> 1200)
- board desktop: LCP 732 ms (> 400)
- logs phone: trace load task 259.7 ms (> 200)
- logs phone: observer load task 258 ms (> 200)
- logs phone: LCP 2044 ms (> 1200)
- logs desktop: LCP 896 ms (> 400)
- settings phone: observer load task 203 ms (> 200)
- settings phone: LCP 1740 ms (> 1200)
- settings desktop: LCP 756 ms (> 400)
