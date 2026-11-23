### Finish-hobnail M5 browser budgets

Measured the client at `8a7bd3c`, the startup and chart change at `6a7ae87`,
the live and log trial, and the final client. The Rust release profile and
backend source were the same. Each browser sweep ran twice against a
throwaway 150-container instance on loopback port 3396 with `--log-fixtures`.
The CDP runner used cold browser caches, 1440×900 desktop at 1× CPU and
390×844 touch phone at 4×. Each visit measured one navigation, three live
update windows and the scripted interaction. Logs used a separate fresh
instance and one `logs-check.mjs` sweep per measured state, with both themes
at both widths and 4× CPU on phone. The runner verified delivery of the
1,500-line burst and kept the original observation windows.

The Overview interaction now clicks a visible health count. The old
disclosure selector discarded both Overview rows in the first baseline sweep.
Overview and Host readiness now use their visible content because a chart
below the viewport can wait until scroll. The comparable baseline and
startup rows use the replays with this probe. The replays include the Board
target to keep its fixture setup. They visit fewer preceding routes, so the
fixture history can be younger. Readiness selectors for the other routes,
throttling, update counts and thresholds were preserved.

Raw `perf.json`, `perf.md`, `logs/results.json`, host pressure samples, build
sizes and test output are under
`/tmp/claude-1000/-home-dev-projects-rust-homepage/c3c8671c-4fd5-4601-90a5-2f27d491fe9d/scratchpad/m5b/`.
The directories `baseline`, `startup`, `batches` and `final` identify the
builds. `ready-replay*` supplies the corrected Overview and Host runs.
The later baseline timings and the baseline log sweep overlapped compilation.
At the end of the second baseline browser sweep, host CPU pressure averaged
13.48% over 60 seconds. Phone log-route LCP was 1,832 ms in the first
baseline and 3,880 ms in the overlapping run. The candidate sweeps ran
sequentially after compilation and tests. Improvements against the
overlapping samples include host variance.

Each table cell shows **baseline median/worst → final median/worst**.
Times are milliseconds except CLS. Live values summarize the maximum of
the three update windows in each run. Load tracing and the long-task
observer measure different windows. An observer reading of zero means
no observed task of at least 50 ms. The first plot value is the trace
task containing the first Recharts mount. The Chart and Logs routes
below are the `svc002` detail and log views. The board fixture has 40 tiles.
