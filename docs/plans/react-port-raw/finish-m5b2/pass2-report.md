#### Second browser budget pass

The final build ran two sweeps on port 3396 with the same release profile,
150-container fixture, route order, cold browser caches, 1440×900 desktop at
1× CPU and 390×844 touch phone at 4×. The readiness selectors, three live
windows and input scripts stayed the same. All 32 visits reached readiness.
One fresh four-profile Logs sweep followed. Compilation and automated tests
finished before measuring. Raw browser results, build sizes and host pressure
are in
`/tmp/claude-1000/-home-dev-projects-rust-homepage/c3c8671c-4fd5-4601-90a5-2f27d491fe9d/scratchpad/m5b2/`.
The directories `entry`, `routes-safe` and `final` contain the timed sweeps.

LCP across routes is the median and worst of each sweep's slowest route.
Live values use the largest task in the three windows of each sweep.
Pass/fail uses the worse sample. Times are in milliseconds.

| Budget | Cap | Pass 1 median/worst | Pass 2 median/worst | Result |
|---|---:|---:|---:|---|
| Desktop LCP across routes | 400 | 782/784 | 668/696 | Fail |
| Phone LCP across routes | 1,200 | 2,190/2,636 | 1,756/1,772 | Fail |
| Phone table LCP | 1,200 | 2,108/2,636 | 1,084/1,188 | Pass |
| Phone table load trace | 200 | 237.85/311.8 | 147/155.7 | Pass |
| Phone table load observer | 200 | 287.5/370 | 217/233 | Fail |
| Phone first container plot | 200 | 297.55/316.3 | 285.5/295.8 | Fail |
| Phone first History plot | 200 | 289.05/304.4 | 251.45/255.8 | Fail |
| Phone Settings load observer | 200 | 218/228 | 219/230 | Fail |
| Desktop table live | 50 | 62.45/70.1 | 56.45/58.4 | Fail |
| Phone History live | 50 | 107.7/118.2 | 51.35/54.5 | Fail |
| Desktop History live | 50 | 51.35/54.5 | 25.2/29.2 | Pass |
| Phone History INP | 200 | 204/240 | 72/112 | Pass |
| CLS across routes | 0.1 | 0.0575/0.0575 | 0.0575/0.0575 | Pass |

The early-read entry alone moved the phone table bootstrap start to
607–647 ms, from up to 1,166 ms in pass 1. Its phone table LCP was
1,502/1,512 ms and its load trace was 160/161.1 ms. The all-route desktop
LCP was 792/816 ms. The final build adds same-origin `Link` preloads for
authenticated dashboard requests, with the existing API authorization
and D5 session/CSRF flow. The phone table bootstrap started at 53–74 ms,
before the session at 70–104 ms. Bootstrap completed at 64–88 ms and the
phone table reached LCP at 980–1,188 ms. The protected HTML variants vary
by cookie and retain the document CSP.

The active page starts early and the direct-entry placeholder is replaced
when its module resolves. Link hover and focus prime subsequent routes.
Sign-in and route styles load separately from the shell. The first route
split hit the 30 s readiness timeout on the phone table and container
chart because a grouped chunk pulled React into the entry's initialization
cycle. The one completed desktop table visit in that run was 408 ms LCP.
Removing the runtime grouping fixed that cycle. With automatic icon
splitting, all four Settings visits still timed out at 30 s with React
error 130. Among the other completed visits, desktop LCP was 660/680 ms,
phone LCP was 1,904/2,100 ms and phone History INP was 192/200 ms. The
icon split was reversed. Both final Settings profiles reached readiness
on both sweeps. The source module breakdown from the trial put React DOM
first in the entry chunk, followed by SDK utilities and the shell.

Plots now prepare tick labels before mount and defer tooltips and event
decorations to a later frame. The existing pixel-width downsampling stays
in place. History applies refreshes after active input settles and feeds
updated plots one per frame. The worst History input delay fell from
143.2 ms to 96.6 ms. The table reuses completed section renders, shares one
resource subscription per visible row, skips live sort snapshots for static
sorts, compares stable row fields directly and formats each resource value
with one decimal conversion. Desktop table cold-load tracing increased
from 62/63.9 to 108.4/108.7 ms, while its live maximum fell by 11.7 ms. The
final sweeps tested the preload, chart and table changes together, so their
overall gains cannot be assigned to a single change.

A larger combined build was rejected by the size gate at 1,350,691 B raw
and 446,421 B gzip. The retained build fits the existing limits.

| Bundle gate | Pass 1 B | Pass 2 B | Pass 2 headroom B |
|---|---:|---:|---:|
| Whole dist raw (1,350,000) | 1,346,772 | 1,349,605 | 395 |
| Whole dist gzip (450,000) | 437,641 | 444,293 | 5,707 |
| First route gzip (180,000) | 144,667 | 136,716 | 43,284 |
| Largest capped lazy chunk gzip (60,000) | 54,814 | 54,812 | 5,188 |

The slowest LCP route in both sweeps was container Logs on both profiles.
In the slower phone visit, the log picker was downloaded by 68 ms and
the shell painted at 716 ms. The log heading painted at 1,424 ms and the
visible summary was the final LCP at 1,772 ms. On desktop the picker
finished by 78 ms, FCP was 296 ms and the first log text was LCP at 696 ms.
Further LCP reductions need to shorten the first render of the shell,
route and log pane. The data transfers alone are too short to close either
LCP gap. The same client could do less work before those elements paint.
Server-rendering them would change the binding architecture.

PROFILE_NOTES

The largest recorded INP across routes was 152/168 ms. The desktop
container chart returned no eligible event timing in either sweep, so
there is no desktop chart INP conclusion. The separate drag-to-zoom
regression checks the interaction itself.

The web check, 152 web tests, build, size check, Rust fmt, clippy and
501 Rust tests passed.

REGRESSION_NOTES

