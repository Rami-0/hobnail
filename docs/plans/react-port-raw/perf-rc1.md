# perf-check 2026-11-18T01:41:11.917Z

base http://127.0.0.1:3441, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 1328.7 | 10 | 920 | 920 | 0.0158 | 3342 | 150.7 / 1 / 1106.5 | 3 (max 169) | 32.2, 26.5, 23.4 | 96 / 65.3 | no visible sortable header th[data-sort="name"] |
| app | desktop | 439.7 | 10 | 340 | 340 | 0.1337 | 3513 | 57.8 / 1 / 545 | 1 (max 62) | 30.9, 29.7, 40.5 | 152 / 82.7 |  |
| chart | phone | 2355.9 | 2 | 1036 | 1332 | 0.0002 | 1831 | 369.3 / 3 / 1137.6 | 9 (max 369) | 10.7, 10.3, 9.2 | 152 / 74.8 |  |
| chart | desktop | 843.3 | 2 | 272 | 400 | 0.0027 | 1831 | 123.2 / 1 / 363.7 | 1 (max 123) | 9.8, 12.3, 4.5 | 16 / 25.3 |  |
| old | phone | 240.5 | 7 | 2440 | 2440 | 0 | 8195 | 1838.4 / 6 / 4338.4 | 1 (max 1837) | 34.5, 159.4, 23.7 | 936 / 922.4 | no visible sortable header th[data-sort="name"] a |
| old | desktop | 102.3 | 11 | 904 | 904 | 0 | 8195 | 535 / 2 / 2125 | 1 (max 535) | 24.3, 49.3, 21.2 | 312 / 179.6 |  |
| old-ctr | phone | 400.6 | 2 | 896 | 896 | 0.0304 | 1858 | 346.6 / 5 / 1593.8 | 3 (max 345) | 6.7, 177.4, 7.3 | 40 / 38.1 |  |
| old-ctr | desktop | 151.1 | 2 | 360 | 360 | 0.0089 | 1858 | 77.7 / 3 / 956.6 | 2 (max 77) | 61.3, 19.3, 38.2, 13.3 | 32 / 26.5 |  |

No budget violation.
