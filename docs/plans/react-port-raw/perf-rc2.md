# perf-check 2026-11-18T01:43:42.958Z

base http://127.0.0.1:3441, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app | phone | 1569.8 | 10 | 1204 | 1204 | 0.0152 | 3408 | 168.5 / 2 / 1271.9 | 3 (max 220) | 26.9, 25.5, 24.4 | 96 / 68 | no visible sortable header th[data-sort="name"] |
| app | desktop | 432.5 | 10 | 348 | 348 | 0.1336 | 3613 | 46.4 / 0 / 530.8 | 0 (max 0) | 23.4, 24.3, 26.8 | 200 / 118.3 |  |
| chart | phone | 2365.2 | 2 | 912 | 1276 | 0.0002 | 1831 | 368.4 / 3 / 1195.2 | 7 (max 368) | 16.5, 13.6, 11.9 | 144 / 98.6 |  |
| chart | desktop | 912.9 | 2 | 380 | 480 | 0.0028 | 1831 | 94.7 / 2 / 341.4 | 3 (max 94) | 6.9, 6.8, 8.7 | 48 / 49.3 |  |
| old | phone | 260.9 | 2 | 2420 | 2420 | 0.0001 | 8195 | 1790.8 / 3 / 4141.9 | 1 (max 1790) | 182.2, 25.5, 203.5, 21.7 | 1176 / 800.8 | no visible sortable header th[data-sort="name"] a |
| old | desktop | 130.8 | 19 | 992 | 992 | 0 | 8195 | 612.1 / 3 / 2317.8 | 2 (max 612) | 30.8, 58, 27.5 | 352 / 210.7 |  |
| old-ctr | phone | 427 | 2 | 964 | 964 | 0.0304 | 1858 | 402 / 4 / 1502.2 | 2 (max 401) | 17.1, 173.8, 6.5 | 56 / 32.7 |  |
| old-ctr | desktop | 196.8 | 2 | 380 | 380 | 0.0089 | 1858 | 91.3 / 2 / 1069.9 | 2 (max 91) | 56.6, 14.3, 43.5, 11.2 | 24 / 20.2 |  |

No budget violation.
