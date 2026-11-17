# Configuration

Two ways, same file: **Settings in the app** (signed in) or editing
`data/config.toml` on the host. Both end up in the same TOML file; the app
edits it losslessly (your comments and ordering survive) and keeps every save
as a revision. See [features/settings.md](features/settings.md).

Everything is optional. **With no config file at all** the page lists every
container, grouped by compose project, with running/stopped state, health,
published ports and links, plus CPU, memory, disk, network, temperature and
uptime. The file holds only overrides and additions. Edit it and save: the page
updates within ~2 s. If the edit is invalid, the last good config stays active
and the page shows the error with file, line and column (also:
`docker exec hobnail /hobnail check-config`).

A 10-line config:

```toml
[page]
title = "homelab"

[containers.media-jellyfin-1]
name = "Jellyfin"
icon = "jellyfin"

[[groups]]
name = "monitoring"
title = "Monitoring"
```

## Page, groups and containers

```toml
[page]
title = "homelab"           # default: host name
greeting = "Welcome back"   # default: time-of-day greeting
columns = 4                 # v1 card grid only: no effect (accepted, kept; not in Settings any more)
layout = "grid"             # "list" makes the flat (ungrouped) table the default view (file only)
theme = "auto"              # auto | dark | light | ember | contrast (Settings › Appearance); the ◐ button overrides per browser
search_url = "https://duckduckgo.com/?q={q}"
show_stopped = true
show_stats = true           # live CPU / memory on container cards

[docker]
# enabled = false           # do not discover containers at all (default true)
exclude = ["ci-test-*"]     # hide containers by name (globs)
standalone_group = "Standalone"
# logs_url / metrics_url (links to Dozzle / Beszel) are retired: logs and metrics
# are in the app. A file that still has them loads; the values are ignored.

[[groups]]                  # order of [[groups]] = order on the page; others follow A-Z
name = "media"              # a compose project name, or a group set by an override/label
title = "Media"
# icon = "jellyfin"         # accepted and kept; the current pages do not draw it
# columns / layout per group: v1 only, no effect (accepted and kept; not in Settings)
collapsed = true
# hide = true

[containers.grafana]        # key = container name
name = "Grafana"
group = "Dashboards"
icon = "grafana"
url = "http://192.168.1.10:3000"
description = "Metrics dashboards"
order = 1                   # sort weight in the group
size = "wide"               # small | normal | wide: accepted and kept; the current pages do not use it
# hide = true

[[bookmarks]]
name = "Router"
url = "http://192.168.1.1"
group = "Links"             # default "Bookmarks"
icon = "builtin:globe"
description = "Admin page"  # second line; default: the URL's host

[system]                    # host tiles and vitals
# enabled = false           # hide them (Settings › Widgets › System tiles)
# refresh = "3s"            # while someone is viewing
# disks = [{ path = "/data", label = "/" }]   # paths as seen in the container; this is the default
# net_iface = "eth0"        # default: the interface with the default route
# temp_sensor = "coretemp"  # hwmon sensor name
```

## Container labels

Set on a container (e.g. in its compose file) instead of in the config. A
config override wins over a label.

| Label | Effect |
|---|---|
| `hobnail.hide=true` | do not show it |
| `hobnail.name=Jellyfin` | display name |
| `hobnail.group=Apps` | group (default: compose project) |
| `hobnail.icon=jellyfin` | icon (see below) |
| `hobnail.url=https://jellyfin.lan` | main link (default: first HTTP-looking published port) |

## Icons and theming

`icon = "name.svg"` / `"name.png"` → your file in `data/icons/`.
`icon = "builtin:database"` → built-in glyph (`database mail globe terminal
chart box server bookmark bot storage`). `icon = "beszel"` → a
[dashboard-icons](https://dashboardicons.com) name, served from
`data/icons/di/` once cached. Containers without an icon get one guessed from
their image. Fetching dashboard-icons from the internet is **off** unless you set:

```toml
[icons]
remote = true    # fetch missing dashboard-icons from the CDN once, then serve from data/icons/di/
# cdn = "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons"
# auto = true    # guess icons from image names
```

Theming: every color, size and font is a CSS variable (see `static/app.css`).
Put overrides in `data/custom.css`; it is loaded last and re-read on every page
load. Example:

```css
:root { --accent: #2f81f7; --r: 4px; --font: "Inter", system-ui, sans-serif; }
```

## Outgoing HTTP

```toml
[http]
allow_hosts = ["192.168.1.10", "127.0.0.1", "*.example.com"]   # empty = any host
```

Applies to widgets, HTTP checks, icon fetches and notifications.

Widgets, notification channels, `[history]` and `[logs]` are configured in
the same file: see [features/widgets.md](features/widgets.md),
[features/notifications.md](features/notifications.md),
[features/history.md](features/history.md) and [features/logs.md](features/logs.md).
