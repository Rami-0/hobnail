# Widgets

- **custom-api:** fetch JSON from any URL, pick fields with paths, show them
  with a template or rows, optional status dot. Add and edit them in
  Settings → Widgets, with a **Fetch now** preview before saving.
- **command:** run a program in the container and show its JSON. Off unless
  `enabled = true`, and **file-only**: the app can never create, change or enable
  one (it would be remote command execution in a container with the Docker
  socket). In Settings they are listed read-only with a lock.
- One failing widget never breaks the page; each has its own timeout and
  refresh; widgets are fetched only while someone is viewing (or an alert rule
  uses them).
- Header secrets (`headers_env`, `headers_file`) are set in the file on the
  host: they are bound to their URL so the app cannot redirect them.

## Add a widget

Widgets show data from anything that speaks JSON, without writing Rust. Add a
`[[widgets]]` block to `data/config.toml` and save; it appears within ~2 s.
Widgets are only fetched while someone has the page open (unless an alert rule
uses them). One failing widget shows an error in its own tile and never breaks
the page.

Examples:

```toml
[[widgets]]
id = "beszel"
type = "custom-api"
title = "Beszel"
link = "http://192.168.1.10:8090"
url = "http://192.168.1.10:8090/api/health"
template = "{{message}}"
status = { field = "code", ok = [200] }
[widgets.fields]
message = "message"
code = "code"

[[widgets]]
id = "shop"
type = "custom-api"
title = "Shop"
url = "http://192.168.1.10:8070/health"
template = "{{status | upper}}"
status = { field = "status", ok = ["pass"] }
[widgets.fields]
status = "status"

[[widgets]]
id = "procs"
type = "command"
title = "Host processes"
enabled = true                       # command widgets are off unless enabled
command = ["sh", "-c", 'set -- /host/proc/[0-9]*; printf "{\"procs\": %d}" "$#"']
template = "{{procs | number}}"
[widgets.fields]
procs = "procs"
```

`command` widgets are defined **only in the file on the host**; the app shows
them read-only and refuses any save that adds, changes or enables one. Add
`custom-api` widgets in Settings → Widgets (with a **Fetch now** preview) or in
the file. Secret headers (`headers_env`, `headers_file`) are file-only too:
they are bound to their URL.

The image contains only the binary and busybox `sh` (shell builtins, no
`curl`/`jq`): put anything more in a static binary under `data/` and call it
from `command`. Full reference:

```toml
[[widgets]]
id = "mailpit"                 # unique, [A-Za-z0-9_-]; DOM id and alert-rule key
type = "custom-api"            # or "command"
title = "Mailpit"              # default: the id
link = "http://192.168.1.10:8025"  # optional, makes the title a link
icon = "mailpit.svg"           # optional, a file in data/icons/
refresh = "30s"                # default 30s, minimum 1s
timeout = "5s"                 # default 5s
# enabled = false              # turn a widget off without deleting it

# --- custom-api ---
url = "http://192.168.1.10:8025/api/v1/info"
# method = "GET"               # any method; default GET
# body = '{"query": "..."}'    # request body (content-type defaults to JSON)
# headers = { Accept = "application/json" }
# Secrets never go in the config. Header values can come from an
# environment variable (give its NAME) or a file (first line, trimmed):
# headers_env = { Authorization = "MAILPIT_AUTH" }
# headers_file = { Authorization = "/data/secrets/mailpit" }

# --- command (disabled unless `enabled = true`) ---
# command = ["sh", "-c", "echo '{\"a\": 1}'"]   # argv, no implicit shell
# env = { FOO = "bar" }        # the environment is cleared except PATH
# Runs in /data (if it exists); stdout is capped at 256 KiB; the timeout
# kills the whole process group; a non-zero exit is an error.

# --- both ---
# output = "json"              # or "text": show stdout/body as-is (`{{_text}}`)

[widgets.fields]               # name = path into the JSON
messages = "Messages"
unread = "Unread"
size = "DatabaseSize"
uptime = "RuntimeStats.Uptime"

# How to show it. All optional:
# template = "{{unread}} unread"   # one big headline value
[[widgets.rows]]                   # label / value rows under it
label = "Unread"
value = "{{unread | number}} / {{messages | number}}"
[[widgets.rows]]
label = "DB"
value = "{{size | bytes}}"
# With neither template nor rows: one row per field, sorted by name
# (no fields either: the raw JSON, shortened).

# Status dot: which field decides, and the values that mean "ok".
# Without `ok`, any truthy value (true, non-zero, non-empty) is ok.
# status = { field = "status", ok = ["pass"] }
```

**Paths** (see `path.rs`): `a.b.c`, `a[0]`, `a[-1]`, `a.0`, `a.#` (length),
`a."x.y"` / `a["x.y"]` for odd keys, `""` or `$` for the whole document.
A path that matches nothing gives `null`, shown as "—".

**Templates** (see `template.rs`): `{{name}}` or `{{name | filter | …}}` with
filters `bytes`, `duration` (seconds), `round:N`, `percent`, `number`,
`upper`, `lower`, `default:"x"`. Besides the fields, templates and the
status rule can use `_status` (HTTP status or exit code), `_ms` (latency)
and, with `output = "text"`, `_text`.

Unknown fields, filters, bad paths and duplicate ids are config errors that
name the widget, so a typo keeps the last good config instead of silently
showing nothing. At runtime a failing widget shows a compact error; it
never breaks the page or the other widgets.
