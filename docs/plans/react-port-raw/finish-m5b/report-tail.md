The final LCP worst case exceeded its cap for every route/profile pair.
Phone History met the cap in one run at 1,080 ms and missed in the other
at 1,224 ms. Desktop and phone table LCP worsened in the final comparison.
In the slowest phone table load, the session response finished at 195 ms,
the bootstrap request started at 1,166 ms and FCP was 1,508 ms. The
session and bootstrap transfers took 11 ms and 12 ms respectively. The
timing gap leaves startup and rendering on the critical path even with
early network reads.

The phone table load cap still fails at 311.8 ms by tracing and 370 ms by
the observer. The intermediate startup sweep passed at 172.5 ms and
190 ms respectively. The final phone container and History first mounts
reached 316.3 ms and 304.4 ms. The Settings phone observer peaked at
228 ms. The desktop table and both History live profiles still exceed
50 ms. The final peaks were 70.1 ms for the desktop table, 118.2 ms for
phone History and 54.5 ms for desktop History. Overview now passes in
both profiles. Both container chart profiles passed scripted INP. The
phone History click reached 240 ms with 143.2 ms of input delay and
42.1 ms of event processing. The final worst CLS was 0.0575.

The retained startup change avoids suspending on an already resolved
route and removes the font-warming render. The chart grid reserves space
before importing Recharts, and plot mounting stays viewport-driven. The
drag overlay updates independently of the chart and tooltips reuse
formatters. The intermediate phone chart INP was 40/40 ms and its first
mount was 320.4/327.2 ms. The intermediate desktop table LCP was
650/664 ms against 468/480 ms at baseline.

The retained live change reuses structurally shared container rows and
snapshots and confines Overview stats renders to the resource panels.
The desktop table live median/worst was 62.85/66.2 ms after the startup
change, 57.65/59.4 ms in the batch trial and 62.45/70.1 ms in the final
sweep. The generic transition on successful refetches was reversed.
With it, phone History live work moved only from 110.65/116.2 ms to
110.5/112.1 ms while INP rose from 136/192 ms to 192/208 ms.
The final phone History INP still exceeds the cap.

The retained log change skips a full-ring sort for ordered batches,
combines filtering and counting and avoids fitting the pane on
content-only resizes. The 128-line frame cap and the DOM window
continue to bound the burst. The batch trial observed no task of
50 ms or longer across the four profiles.

| Bundle gate | Baseline B | Final B | Final headroom B |
|---|---:|---:|---:|
| Whole dist raw (1,350,000) | 1,349,132 | 1,346,772 | 3,228 |
| Whole dist gzip (450,000) | 438,069 | 437,641 | 12,359 |
| First route gzip (180,000) | 144,646 | 144,667 | 35,333 |
| Largest capped lazy chunk gzip (60,000) | 54,813 | 54,814 | 5,186 |

The final web check, 152 web tests, build, size check, Rust fmt, clippy
and 500 Rust tests passed.
