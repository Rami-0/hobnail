# Boards

Boards, sections, items, layout and private edit drafts are stored in SQLite.
Create an empty board in Settings → Boards, then use edit mode to add content.
There is no automatic All containers board. Discovery supplies suggestions;
it never inserts a service without a choice. The Containers page remains a
separate discovered inventory, filtered by account scopes.

A board has a name, icon and position. Sections have a title and ordered items.
Item kinds are container, link, widget and integration. Container targets use
Compose project/service where available, otherwise the container name. Widget
items show an existing configured widget's structured read model. Integration
tiles show saved connection metadata, not provider health. A deleted or missing
target remains visible as missing instead of silently vanishing.

Responsive spans are columns, not pixels: mobile 1–4, tablet 1–8, desktop 1–12,
rows 1–12. Edit mode offers move/resize controls and pointer dragging; Alt+arrows
moves and Shift+arrows resizes. A document has at most 64 sections and 512 items;
there are at most 64 boards. Server IDs retain identity across edits.

Save commits one versioned board document; Cancel discards that edit. Private
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
