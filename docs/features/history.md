# History (local database)

Hobnail remembers. A small SQLite file records host and per-container
CPU and memory every 15 s, plus an event log, so the page can answer "what
happened last night?".

- **File:** `data/hobnail.db` (plus `-wal`/`-shm`). Override with `RH_DB`.
  If it cannot be opened the dashboard keeps working without history.
- **Sampler:** every `[history] sample_interval` (default `15s`, 5 s to 5 min),
  with or without a viewer. It keeps its own CPU/network counters, so the live
  view and history never disturb each other.
- **Recorded:** host CPU, load, memory, swap, disk %, network rx/tx,
  temperature; per container (by name, so recreations keep their history) CPU %
  and memory. Events: container start / stop / die (exit code) / OOM / health
  change, alerts fired / resolved / delivery failed, config saved / restored,
  logins, app start.
- **Retention:** raw 15 s samples for 6 h, 1-minute rollups (avg + max) for
  7 days, 10-minute rollups for 90 days; events 180 days (max 100k rows);
  config revisions: newest 200. Hourly maintenance rolls up and prunes.
- **Size:** about 400 KiB/h in the first 6 h, then much slower; projected
  steady state ~30–35 MiB for ~38 containers (budget 50 MB). See
  `docs/measurements.md`.

```toml
[history]
enabled = true            # samples; events are always recorded
sample_interval = "15s"
```

**Where it shows:** Overview charts and "Recent events", the vitals chips in the
top bar, container detail (6 h / 24 h / 7 d), the History page (1 h … 30 d,
restarts per day, incident timeline), the Alerts log.

**JSON API** (open like the page; GET only):

- `GET /api/history/system?metric=cpu&range=24h` — metrics `cpu mem swap disk
  net_rx net_tx temp load1`, ranges `1h 6h 24h 7d 30d`
- `GET /api/history/container/<name>?metric=mem&range=6h` — `cpu mem`
- `GET /api/events?limit=50&before=<id>&subject=<name>&kind=container.`

Responses: `{"range":"24h","step":240,"points":[[unix,value],…]}` (≤ ~400
points) and `{"events":[{id,ts,kind,level,subject,detail}]}` newest first.
Login events (`auth.*`, which include client IPs of failed attempts) are only
returned to a signed-in session.

**Settings → History and data** shows the database size and row counts, can
export a snapshot and clear the history (samples and events; revisions and the
password stay).
