# perf-check 2026-11-17T22:30:18.093Z

base http://127.0.0.1:3440, container svc002, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).

| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| old | phone | 509.9 | 150 | 2776 | 2776 | 0 | 8195 | 2151.6 / 5 / 4508.7 | 1 (max 2150) | 35.5, 196.7, 765.8, 765.8 | 968 / 717 | no visible sortable header th[data-sort="name"] a |
| old | desktop | 208.9 | 150 | 1104 | 1104 | 0 | 8195 | 619.5 / 2 / 2215.4 | 1 (max 619) | 27.2, 73.9, 21 | 536 / 302.6 |  |
| old-ctr | phone | 981.9 | 2 | 2136 | 2136 | 0.0304 | 1859 | 819.5 / 6 / 3195.4 | 3 (max 817) | 486.5, 24.8, 21.5, 249.5 | 160 / 71.6 |  |
| old-ctr | desktop | 458.4 | 2 | 924 | 924 | 0.0089 | 1884 | 248 / 4 / 1677.8 | 2 (max 247) | 50.5, 9.3, 15.1, 80.7 | 32 / 29.9 |  |

No budget violation.
