# History (local database)

Hobnail remembers. A small SQLite file records host and per-container
CPU and memory every 15 s, plus an event log, so the page can answer "what
happened last night?".

- **File:** `data/hobnail.db` (plus `-wal`/`-shm`). Override with `RH_DB`.
  A data folder that already has a `rust-homepage.db` (the project's earlier
  name) keeps using that file.
  If it cannot be opened the dashboard keeps working without history.
- **Sampler:** every `[history] sample_interval` (default `15s`, 5 s to 5 min),
  with or without a viewer. It keeps its own CPU/network counters, so the live
  view and history never disturb each other. Each tick runs on a blocking
  thread, not on the thread that answers requests, and the SQLite page cache
  is given back after every tick and after maintenance.
- **Recorded:** host CPU, load, memory, swap, disk % (the first disk),
  network rx/tx, temperature, plus the Host page's series (per core, disk,
  interface and sensor, pressure); per container (by name, so recreations
  keep their history) CPU % and memory, plus the Metrics tab's series (CPU
  throttling, page cache and anonymous memory, memory limit, OOM kills,
  network, block I/O, PIDs). Events: container start / stop / die (exit
  code) / OOM / health change, alerts fired / resolved / delivery failed,
  test notifications, config saved / restored, logins and failed logins,
  password changes, history cleared, app start.
- **Retention:** raw 15 s samples for 6 h, 1-minute rollups (avg + max) for
  7 days, 10-minute rollups for 90 days; events 180 days (max 100k rows);
  config revisions: newest 200. Hourly maintenance rolls up and prunes.
- **Schema:** migrations are applied when the file is opened; the current
  version is 6 (it adds two time indexes on the events table). A binary
  older than the file refuses to open it and runs without history, so keep a
  copy of the file before going back to an older build.
- **Size:** about 400 KiB/h in the first 6 h, then much slower. With the
  Host page series, the container Metrics series and log level counts the
  projection is 60–70 MiB at 90 days for ~40 containers; this has not been
  observed over a full 90 days. See `docs/measurements.md`.

```toml
[history]
enabled = true            # samples; events are always recorded
sample_interval = "15s"
```

**Where it shows:** Overview charts and "Recent events", the vitals chips in the
top bar, container detail and the Host page (1 h … 30 d), the History page
(1 h … 30 d, container starts per day over 7 days, incident timeline), the
Alerts log.

**The History page** (`/history`, public; login events only for a session):

- **Window:** a range (`?range=1h` … `30d`, default `24h`) or a zoom (`?from=&to=`, unix
  seconds; 5 minutes at least, a shorter one is widened around its middle;
  30 days back at most). Drag across a chart to zoom, with a mouse or
  sideways with a finger (vertical drags still scroll). The toolbar's
  "‹ Earlier", "Zoom in", "Zoom out" and "Later ›" are plain links, so the
  window moves by touch, keyboard and without JavaScript. "Compare with
  yesterday" (`&cmp=1`) stays in its place and is greyed out above 24 h.
  From 24 h up the charts also draw the bucket maxima as a band.
- **Incident timeline:** the events inside the chart window, newest first,
  60 per page, "Older events" pages back (`&before=<id>`); "Incidents" shows
  warnings and errors, "All events" (`&events=all`) everything. Each row has a clock time,
  links its subject (a container, or what an alert was about) and has the
  container's logs at that moment.
- **Markers** on the charts (crashes, OOM kills, failing health checks, fired
  alerts) open their timeline row, highlighted; on the Overview they open
  History an hour around the event. Under the crosshair the read-out says
  what the marker is (touch has no tooltips).
- The avg / max / yesterday legend toggles act on every chart and are
  remembered in the browser.
- The page reads the events of the window in one pass over the time index,
  newest first, at most 10,000 per page view (never on a timer); a window
  with more goes on with "Older events".

**JSON API** (open like the page; GET only):

- `GET /api/history/system?metric=cpu&range=24h` — metrics `cpu mem swap disk
  net_rx net_tx temp load1` (required), ranges `1h 6h 24h 7d 30d` (default
  `1h`); `&agg=max` gives the bucket maxima instead of the averages
- `GET /api/history/container/<name>?metric=mem&range=6h` — `cpu mem`, same
  `range` and `agg`
- `GET /api/events?limit=50&before=<id>&subject=<name>&kind=container.` —
  `limit` 1 to 500 (default 50), `kind` is a prefix

Responses: `{"range":"24h","step":240,"points":[[unix,value],…]}` (≤ ~400
points, oldest first) and `{"events":[{id,ts,kind,level,subject,detail}]}`
newest first. A bad parameter answers `400` with `{"error":"…"}`.
Login events (`auth.*`, which include client IPs of failed attempts) are only
returned to a signed-in session.

**Settings → History and data** turns recording on or off (saves when
switched), sets the sample interval (a duration such as `30s`; 5 s to 5 min),
shows the database size and row counts, can export a snapshot (JSON: the
newest 500 events, the revision list and the database figures) and clear the
history (samples, log level counts and events; revisions and the password stay) after the
phrase "clear history" is typed in the confirm dialog.
