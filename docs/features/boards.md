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
tiles show connection metadata and the outcome of a manual connection test. A deleted or missing
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
