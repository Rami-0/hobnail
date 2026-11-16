# Logs in the app

Code: `src/logs/`, `src/db/logcounts.rs`,
`static/logs.js`, `static/logs.css`.

## Pages

| Route | What |
|---|---|
| `/containers/<name>/logs` | One container: the container's **Logs tab** (its header and tabs above the viewer; `?tab=logs` on the detail page lands here too). Stopped containers show their last lines under a "stopped" banner. Under the viewer: error and warning counts (collapsed) over the range the detail page had (`?range=`; the tabs keep it). |
| `/stacks/<project>/logs` | Every container of a compose project, merged by timestamp; each line has a color chip per container (stable hue from the name, `<project>-` prefix dropped). Only compose projects have one (a config group has no stack logs). |
| `/logs` | Every container the dashboard lists, stopped ones too even with `[page] show_stopped = false` (hidden and excluded ones never), grouped by stack, each name one click from its logs; a filter box (`?q=` on the server, as-you-type with the script; Enter opens the first match), "Whole stack" for compose projects, and ticks for side by side. The filter and the ticks are separate forms: without the script Enter in the filter only filters. Coming back to the page recounts the ticks. |
| `/logs?c=a,b` | Split view, 1 to 4 containers side by side (stacked on phones). Every pane is named, links to the container's own logs (and its stack, for a compose project) and has a close button (in place with the script; without it, a link to the view without that pane). "Add" opens the list with the shown ones ticked. The breadcrumb leads back to `/logs`. Names that are not shown containers (gone, hidden, excluded or mistyped) are named in a "Not found" banner and never tailed; more than four are named too. |
| `/logs?sel=a,b` | The list with these ticked ("Add", and "side by side" under a container's log). `/logs?go=1` (the list's own form) with nothing ticked shows the list with "Tick one to four containers first." |

The Logs nav item is current on every logs page. Logs are one click from
every place a container is named: "Logs" in the Containers table, a logs icon
on sidebar items (and stacks that are compose projects), Overview ("Needs
attention", "Top CPU", "Recent events": at that moment), Host, History,
Alerts and Settings › Containers and groups; only for containers whose logs
may be read (`logs::tailable`: an excluded container gets no logs link
anywhere, the public Host page included). Access is unchanged: all of these
pages and `/api/logs/*` need a session unless `require_login = false`. On
touch screens these icons are 44 px and their rows grow to fit, so no hit
area reaches into another row or onto the row's own link.

**Following** means the newest line is in view: after the first load, after
every batch, after "Jump to latest" and after anything that changes heights
(Wrap, Time, filters, a resize), measured after layout. Lines have their real
height (no `content-visibility` placeholder) and a `ResizeObserver` per pane
re-asserts the bottom while following. Any move up stops it, however small or
slow (a wheel of a few pixels, a slow finger drag); the script's own moves and
a clamp after the content shrank do not count. Only a move down into the
bottom 24 px, "Jump to latest" or End starts it again; the pill counts new
lines meanwhile. Scrolled up, the reader stays on the same line when heights
change (Wrap, Time, a filter, a resize): the pane remembers the first line in
view on each scroll and puts it back after layout, and scroll anchoring is on
except while following. `scripts/logs-check.mjs` checks all of this in a real
browser (1440 and 390 px, Wrap on and off, single, stack and side by side;
slow wheels and real touch drags) and reports the page's CPU while following,
with the default tail and with a full ring.

A config change reloads other open pages, but never a logs page, the `/logs` list included (it keeps its
search, filters, pause, range and scroll), nor does coming back to a logs tab
that was hidden for a while.

**Toolbar.** One row: the search (with its match counter and older / newer
buttons, `.*` and `Aa`), then "Show" (all lines, only matching, or matching
with ±1/3/10 lines of context), the level chips, the range, Wrap, Time, Pause,
Download and the counter. On a phone the row is the search and "Filters"
(with the number of active filters); the rest opens under it, and the pane
fills the screen down to the bottom bar. Custom range: its own row with
"From" and "To" (minute precision); Apply refuses an empty range, a From in
the future and a To before From, on the field.

**Search** steps through the matches: a new search goes to the newest match,
Enter to the next older one, Shift+Enter to the next newer one (wrapping),
with "3/17" in the box; the current match is outlined. The search, `.*` and
`Aa` are kept in the address (`search=`, `re=1`, `case=1`), so a reload or a
shared link keeps them.

**Line actions.** Copy and Link (a `#t=` permalink) show on hover, on a line
that was tapped or clicked, and from the keyboard: Enter in a pane picks the
current match or the first line in view and focuses Copy; Arrow Up / Down
move to the neighbouring line, Esc goes back to the pane.

**Older lines.** A live tail offers "Load older lines" at its top: the lines
before the oldest one held (a stream with `until=`), put in front without
moving the reader, while the ring has room. In a stack, a container that sent
its full tail gets a mark where its loaded history starts. A `#t=` link to a
moment with no line says when the next line is; one to the future shows the
live tail and says so.

Features: live tail (SSE), pause on scroll-up with a "Jump to latest" pill and a
new-line count, Pause button, line counter, ANSI colors (SGR: 16/256/truecolor fg and
bg, bold, dim, italic, underline), JSON lines collapsible with `msg` and the first
scalar fields in the summary, level gutter and level filter chips with counts, search
(case-insensitive by default, `Aa` for case, `.*` for regex; an invalid regex says so
and never throws), "Only matching" with ±1/3/10 context lines, time range (live, 15 min,
1 h, 6 h, 24 h, since container start, custom from/to), wrap and timestamps toggles
(remembered per browser; times in the viewer's local time, another day as "26 Oct";
in a pane narrower than 340 px the time sits above the message), download as text, copy
line and copy a permalink (`#t=<unix ms>`; opening it loads the minutes around that
line and highlights it). Keys: `/` search, `Enter` / `Shift+Enter` older / newer match, `Esc` clears it,
`End` jumps to the latest; in a pane, `Enter` picks a line. Download says what it
downloads (the whole range, without the search and level filters).

Log text reaches the DOM only through `textContent` (a test fails if `logs.js`
contains `innerHTML`). Highlighting uses the CSS Custom Highlight API on lines in view,
so it never rewrites the DOM.

## API (GET only)

- `/api/logs/stream?c=a[,b…]` or `?stack=x`, `&tail=N` (≤ 5000), `&since=<unix s | 15m | 1h | start>`,
  `&until=<unix s>` (no follow). SSE events: `meta`, `l` (array of
  `{c, t (unix ms), m, s:2 for stderr, l: e|w|i|d, x:1 if cut at 16 KiB}`), `err`
  (`kind`: notfound, unreachable, error), `end`, `done`.
- `/api/logs/download?…` same parameters, plain text without ANSI, `Content-Disposition: attachment`.
- `/api/logs/levels?c=name&range=1h|6h|24h|7d|30d`: level counts.
- `/api/logs/status`: background tails (watched, over cap, lines seen, open viewer streams).

Docker calls: `GET /containers/{name}/json` (TTY? running? started?) and
`GET /containers/{id}/logs?follow=…&stdout=1&stderr=1&timestamps=1&tail=…&since=…`.
Container names are checked against Docker's name syntax before they go into a URL.

## Cost model

- One Docker connection per viewer per container, opened when the page's
  `EventSource` connects and closed when it goes away (dropped response body, a
  ping every 2 s catches half-open sockets). Tested: closed 53 ms after the client left.
- No unbounded buffer on the server: lines ≤ 16 KiB, a 128-line queue and a
  4-event body queue; a slow browser means backpressure on Docker, not memory.
- The browser keeps at most `[logs] ring` lines (default 5,000) per pane.

Measured (release, see docs/measurements.md): one viewer on a 550 MB/h log keeps the
process at ~6 MiB; the initial 5,000 lines arrive in ~84 ms; search over 5,000 lines
filters in 4–16 ms in Chromium.

## Background tails, log alerts and level counts

`logs::run` tails (from "now", `tail=0`) the running containers that an enabled
`log_match` rule watches, plus those matching `[logs] count` (globs, default none), at
most `[logs] max_watched` (default 20), alert-watched first. Containers over the cap
are listed in a banner on the logs pages, in the rule's state ("not watched: more than
20 containers to tail") and once in the app log. With no rule and no `count`, the task
opens nothing.

Every tailed line is counted per level per minute into `log_counts` (v3 migration:
counts only, never text; each write updates the 1-minute and 10-minute rows, kept 7 d
and 90 d like the other rollups, pruned hourly by the same task; "clear history"
clears them). Viewers' streams are not counted (a page reload would count lines twice).
Cost: ~0.3 MiB and ~3 % of one core for 5 containers at ~280 lines/s together.

`log_match` rule (see `src/alerts/mod.rs` for the full example):

```toml
[[alerts]]
name = "app-errors"
type = "log_match"
pattern = '(?i)\b(panic|fatal)\b'
containers = ["shop-*"]   # and/or stack = "shop"
count = 3                 # matching lines within
window = "5m"             # 10s..1d (default: 1 line in 5m)
channels = ["telegram"]
```

The server-side matcher (`src/logs/matcher.rs`) is a small Pike VM, linear time, no
`regex` crate: literals, `.`, classes, `\d \w \s \b`, groups, `|`, `* + ? {n,m}`,
`^ $`, `(?i)`. Backreferences and lookaround are rejected at config validation.
Alert messages carry the count, never log text.

## Config

```toml
[logs]
require_login = true    # default; false makes logs as open as the rest of the page
count = []              # container globs tailed in the background for level counts
max_watched = 20        # most background tails
ring = 5000             # lines kept per pane in the browser (1000..50000)
tail = 500              # lines loaded when a page opens (100..5000)
```

**Logs need a login by default** (fail closed, after a security review):
container logs often contain secrets (tokens in URLs, connection strings,
emails). Until a password is set with `set-password` the logs pages explain
how to unlock them. Set `require_login = false` to make logs as open as the
rest of the dashboard. Containers hidden or excluded by the owner
(`hide`, `hobnail.hide`, `[docker] exclude`) cannot be tailed at all,
and names that are not shown containers never reach Docker.
