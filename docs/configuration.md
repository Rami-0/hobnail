# Configuration

Two ways, same file: **Settings in the app** (signed in) or editing
`data/config.toml` on the host. Both end up in the same TOML file; the app
edits it losslessly (your comments and ordering survive) and keeps every save
as a revision. See [features/settings.md](features/settings.md).

The TOML file is optional. Accounts, boards, layout, integrations, dashboard
settings and setup drafts live in SQLite, not in TOML. A new board is empty;
discovery supplies suggestions, with no automatic All containers board.
Login is required. **With no config file at all** the Containers page lists every
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

Theming: every color, size and font is a CSS variable (see `web/src/styles/tokens.css`).
Put overrides in `data/custom.css`; it is loaded last and re-read on every page
load. Example:

```css
:root { --accent: #2f81f7; --r: 4px; --font: "Inter", system-ui, sans-serif; }
```

## Incoming reverse proxy

Set `RH_TRUSTED_PROXIES` to a comma-separated list of exact IPv4 or IPv6
addresses and restart Hobnail. The default is empty. List only proxies you
control. Include the TCP peer IP as Hobnail sees it and any upstream proxy IPs
that should be skipped when walking `X-Forwarded-For`. A shared address such as
a Docker gateway can also represent direct clients. Use a dedicated proxy
address and restrict direct access to the backend.

Hobnail serves HTTP. Configure the trusted proxy to overwrite
`X-Forwarded-Proto` with a single `https` or `http` value based on the
external connection. HTTPS requests then receive a Secure session cookie.
Preserve the external Host and add it to `RH_ALLOWED_HOSTS` if necessary.

For login throttling, the trusted proxy must append the IP of its TCP peer to
`X-Forwarded-For` or replace the header with that IP if it is the edge proxy.
Hobnail walks the list from right to left, skips configured proxy IPs and
uses the first other IP. Use bare IPv4 or IPv6 literals without ports.
A missing list, malformed hop in the trusted part of the chain, or list
with only trusted IPs falls back to the TCP peer.

Connection limits keep slow, silent or oversized requests within the memory
budget and stop a few peers from holding every connection:

- A request line plus headers (cookies included) may total 16 KiB; a larger
  request gets `431 Request Header Fields Too Large`.
- A connection that has not sent a complete request head within 10 s is
  closed, including an idle keep-alive connection.
- A request body must arrive in full within 15 s of its head, or the request
  gets `408 Request Timeout`.
- A response write that makes no progress for 15 s (a client that stopped
  reading) closes the connection. Event streams and log streams and
  downloads are exempt.
- At most 128 connections are open at once, event streams included; further
  connections wait until one closes.
- One peer may hold at most 32 of them (IPv6 peers count by /64); more get
  `503` and are closed. Addresses in `RH_TRUSTED_PROXIES` are exempt, because
  every client arrives through them. If clients reach Hobnail through
  another shared address that is not a trusted proxy, such as Docker's
  userland proxy for ports published on `127.0.0.1`, they share that peer's
  32 connections.

A proxy that keeps a pool of idle upstream connections reconnects when
Hobnail closes one.

## Outgoing HTTP

```toml
[http]
allow_hosts = ["192.168.1.10", "127.0.0.1", "*.example.com"]   # empty = any host
```

Applies to widgets, HTTP checks, icon fetches, notifications and integration
tests and refreshes. It matches hostnames (not URL paths). Provider requests check
all resolved addresses and connect using those checked addresses. With no list,
LAN addresses are allowed but loopback/link-local/metadata, multicast, reserved
and selected translation addresses are refused. Explicit matching hosts are the
host operator override. Matching hostnames' DNS answers are trusted for that
address-policy exemption. Provider requests stop at redirects and use public
certificate roots by default. The integration editor also supports an explicit
SHA-256 leaf pin. Public provider HTTP requests require the SQLite dashboard
setting **Provider internet access**, which is off by default. See
[Integrations](features/integrations.md).

Widgets, notification channels, `[history]` and `[logs]` are configured in
the same file: see [features/widgets.md](features/widgets.md),
[features/notifications.md](features/notifications.md),
[features/history.md](features/history.md) and [features/logs.md](features/logs.md).

`[logs] require_login` is accepted for old files but is a no-op: every log
endpoint requires a session, regardless of that value.
