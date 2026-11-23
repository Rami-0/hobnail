# Boards

Boards, sections, items, layout and private edit drafts are stored in SQLite.
Create an empty board in Settings → Boards, then use edit mode to add content.
There is no automatic All containers board. Discovery supplies suggestions;
it never inserts a service without a choice. The Containers page remains a
separate discovered inventory, filtered by account scopes.

A board has a name, icon and position. Sections have a title and ordered items.
Item kinds are container, link, widget and integration. Container targets use
Compose project/service where available, otherwise the container name. Widget
items show an existing configured widget's structured read model, or a provider
widget's data. Provider widgets are placed from an integration's "Choose
widgets" dialog (or the guided setup), and each placement keeps its own widget options
and refresh interval while sharing the saved connection; switching a placed
widget off in that dialog removes it from the board in the same board revision.
Integration tiles show connection metadata and the outcome of a manual
connection test. A deleted or missing target remains visible as missing
instead of silently vanishing.

A reader who can see the board but not an integration on it gets placeholders:
that integration's provider widgets read "Widget unavailable" and its
integration tile reads "Integration unavailable", with neither the integration
ID nor its custom title in the response. A scoped editor's save maps the
placeholders back, so saving the board keeps those items.

Each section is a canvas of 12 columns and fixed-height rows (80 px, or
64 px when the board is compact). A tile is a rectangle on it: drag a tile
anywhere and the others move out of the way, with a placeholder showing
where it lands; tiles always float up so the canvas has no holes. Drag a
tile's bottom-right corner to resize it in whole cells, down to the widget's
minimum size. On a touch screen, press and hold a tile before dragging it.
From the keyboard, focus a tile's grip: arrow keys move it one cell and
Shift + arrow keys resize it, and each step is announced. Sections have a
title, can be moved up and down, and can be collapsed (collapsed sections
start folded; readers can unfold them). On a phone the board is one column
in reading order (top to bottom, then left to right), and editing there is
limited to reordering with Move earlier and Move later. Ctrl+Z and
Ctrl+Shift+Z (or Ctrl+Y) undo and redo within an edit session.

Add puts the chosen item in the first free slot of the section. A tile's
**Customize** panel sets its title, icon, link, accent colour, size and
section, and whether it shows its status and metrics. **Style** sets the
board's name and icon, a background preset, compact or comfortable
spacing, the tile style (solid, glass or outline) and the board's accent.
Backgrounds are built-in gradients: the page's CSP allows only same-origin
images, so custom image URLs are not offered. A document has at most 64
sections and 512 items; there are at most 64 boards. Server IDs retain
identity across edits. Boards saved before the canvas keep every section
and item: each item was placed, in saved order, in the first free slot of
its old size.

Save commits one versioned board document, including the name, icon and style; Cancel discards that edit. Private
server drafts are saved separately while editing. Leaving with unsaved work
offers to keep the draft; resuming and discarding are explicit choices. A 409
never silently overwrites another edit: reload or rebase and review. In-session
conflicts use a three-way merge. A stale recovered draft has no historical base,
so it preserves server additions and may need old removals applied again.

Settings chooses a default board for the home route; without a usable default,
the app shows Overview. A deleted board clears its default reference. The Owner
can grant access to individual boards and integrations. Viewers read; Admin and
Owner edit. Scoped Admins edit granted documents but cannot create or delete
boards or integrations. Scoped containers are the union of container targets
on granted boards; host-wide pages are unavailable. Suggestions require Admin
and obey visibility. See [login.md](login.md) and [api.md](../api.md).

## Hobnail widgets

The board **Add** picker opens on **Hobnail widgets**: live widgets drawn
in the browser from data Hobnail already collects, so they need no
service, credential or extra server memory. They belong to the built-in
`hobnail` integration, created the first time one is added.

| Widget | Shows | Data |
|---|---|---|
| Host overview (4x4) | CPU, memory and network tiles with live trends, load, uptime, the fullest disks as meters | the `system` stream |
| Top containers (4x4) | the busiest running containers by CPU or memory (3–12) | the `containers` and `stats` streams |
| Stack health (4x3) | each compose stack, running of total | the container list |
| Recent events (4x3) | starts, stops, crashes and health changes of the last 24 h | `/api/v1/history?metric=none` |
| Log tail (6x4) | the last 5–30 lines of one container, every 20 s (empty: the busiest running container) | `/api/logs/download?tail=` |
| History chart (6x3) | one series (CPU, memory, disk, load, network in/out, temperature, swap) over 1, 6 or 24 h | `/api/v1/history?metric=` |

The server only validates the options and names the view
(`{"kind":"hobnail","view":…,"options":…}`). Viewers see what their
role and scopes allow on the pages these widgets draw from.

The clock, Markdown note, bookmarks, weather and RSS widgets sit on the
same tab. Third-party integrations come after them.

## First board and tour

[Guided setup](login.md#guided-setup) creates Home from the owner's selection
and sets it as the landing page. If the owner selects nothing, Home opens
with Add and Discover. Those actions open the standard board editor and picker.

On the first visit to the default board (or the first accessible board if
there is no default), Hobnail offers a short tour of the controls the account
can use. The tour points to the controls on the page. Use Next to advance,
Skip tour or Escape to dismiss, and Finish at the end. The tour moves keyboard
focus to its actions and returns focus to the highlighted control on dismissal.
Completion and dismissal are saved per account across browser sessions.
Use **Replay board tour** in Settings or Keyboard shortcuts to run it again.

## Refresh failures

When a configured widget refresh fails, its tile keeps the last successful
reading and shows a stale badge with its age and the latest failure reason.
A failure before the first successful refresh shows the error by itself.
The next successful refresh replaces the reading and clears the badge.

Generic HTTP integration tiles show evidence from manual connection tests.
A failed retest retains the timestamp and latency of the previous successful
test with a stale badge and the failure reason. See [Integrations](integrations.md).
