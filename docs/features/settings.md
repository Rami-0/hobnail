# Settings in the app

`/app/settings`, Admin or Owner (see [login.md](login.md)). Tabs include
General, Containers and groups, Bookmarks, Widgets, Notifications, Appearance,
History and data, Config file, Security, Boards and Integrations; Users is
Owner-only. Scoped Admins can edit granted boards and integrations only.
The Settings link remembers this browser's last tab. Signed-out users see
sign-in; Viewers receive no configuration. Bookmark/widget dialogs also open
on Overview and notification dialogs on Alerts for unrestricted Admin/Owner.
Boards and integrations use SQLite, independently of TOML revisions; see
[boards.md](boards.md) and [integrations.md](integrations.md).

**How a save works** (`src/settings/`): load the file and its SHA-256 →
patch with `toml_edit` (comments, order, blank lines and unknown keys survive)
→ validate with the same parser the server uses → run the guards → write
atomically (temp file + rename, same mode) → add a revision → record a
`config.saved` event → reload the config at once. Open pages refresh.

- **Stale protection:** every form sends the hash it loaded; if the file changed
  on disk in between, the save is refused (HTTP 409) and the page offers to
  reload. A hand edit is never overwritten. A save from the page itself is
  not an outside change: the other controls on the page that showed the
  file it started from move on to the file it wrote, so switching Recording
  after saving the Sample interval just works (custom.css has its own hash
  and never moves a config one).
- **Line endings:** a config file or custom.css written with CRLF only stays
  CRLF (the editors give LF; new lines get CRLF too). A restore writes the
  revision exactly as it was.
- **Invalid is never written:** HTTP 422 with the field (or line:column) to fix.
- **Revisions:** newest 200, with author and note ("widgets: added mailpit",
  "raw edit", "restored #12"); hand edits show up as "on disk" revisions. Diff
  any revision against the current file; restore creates a new revision.
- **Raw editor:** validate-as-you-type (debounced), errors highlighted by line.
  Inline channel secrets are redacted in raw text, revisions and diffs and
  preserved on save. If config parsing fails, repair the file on the host
  before the editor can show it safely.
- **Errors on the field, in plain words:** a form's 422 names the field that
  caused it ("Use a duration like 30s or 5m.", "Use a whole number.", "Use a
  link that starts with http:// or https://.") and carries no config-file line;
  the browser checks required fields, links, numbers and lengths first and
  shows every problem at once.

**Forms** (the ux-pass dialog, DESIGN.md §11.6), the same on every tab:

- Add and edit open in a dialog from the row or button you clicked; the URL
  says which (`?add=bookmark`, `?edit=bookmark:2`, `?edit=channel:ops-hook`,
  `?edit=rule:disk-full`, `?edit=widget:mailpit`, `?add=exclude`,
  `?edit=group:media`, `?edit=override:grafana`), so a reload or a link opens
  the same dialog. "Edit" on a container page (signed in) opens its override
  dialog (`?name=<container>` still works). Esc, ×, Cancel or the backdrop
  close it, and ask first when something was typed. Enter saves (Ctrl+Enter
  from any field); the button shows progress; the list behind updates in
  place and the changed row flashes.
- Lists and form values come from `GET /api/settings/*`, Admin/Owner only,
  `no-store`; there are no HTML fragment routes.
- No staged lists: every add, edit and delete saves when confirmed, also for
  Hidden containers and Groups (each with Undo where it deletes).
- Delete asks in one confirm dialog; the message after it has **Undo**, which
  restores the revision from before the delete (refused if the file changed
  since). Up / down arrows (bookmarks, widgets and groups) move an entry one
  step, one revision per step. A channel or widget that alert rules still
  use is not offered for deleting: the message names the rules.
- An edit sends only the fields you changed (a bookmark sends its five
  fields, all of which the form shows); the server keeps every key the
  request leaves out (`null` removes one). A save therefore never removes
  something the form does not show, and Save with no change writes nothing
  ("No changes"). An untouched edit dialog sends nothing at all, so it says
  "No changes" even when the file moved meanwhile ("Save and send test" on
  an untouched channel only sends the test).
- Channel names and widget ids stay editable. Renaming one that rules refer
  to asks first, naming the rules; saving again renames it and changes those
  rules to match (one revision).
- A setting that is one control saves when changed: a rule's on / off in its
  row, System tiles, the default theme, Recording on / off. Switching System
  tiles or Recording back on removes the `enabled = false` that switching
  it off wrote (and an emptied `[system]` / `[history]` table), so off then
  on gives back the file as it was; a key or table with a comment stays and
  is set to `true`.
- Settings with several fields that stay on the page (General, Sample
  interval, custom.css, the raw editor, Account password) use the same submit
  path: every error at once on its field, a busy Save, only the changed
  fields sent, "No changes" when nothing changed, and a conflict note in the
  form with "Reload and keep my input" (which goes on only when nothing the
  form shows changed in the file; otherwise copy your input and load the
  current values).
- Every dialog, confirm and arrow click saves against the file version it
  was started from; nothing is refreshed under an open dialog. If the file
  changed meanwhile the save is refused and the dialog says so, with "Reload
  and keep my input": an edit then goes on only if the same entry is found
  again with all its original values, otherwise it cannot be saved (Discard).
  Session expired: sign in in a new tab, then save again; the input stays.
  A switch, an arrow or the bulk bar whose save meets an expired session is
  put back and shows the same banner with "Sign in (new tab)".
- The React application requires JavaScript. Authenticated writes use the
  session token from `GET /api/v1/session`; no plain-form write fallback.
- One toast system for the whole app: at most three, dismissible, above the
  phone's bottom bar, never over an open dialog (the dialog's note is used).

**Guards** (`src/settings/guard.rs`, applied to every write path):

1. *`command` widgets are file-only.* Any widget whose `type` is not exactly
   `custom-api`, or that has `command`/`env` keys, must be identical to one
   already in the file. Adding, changing (any key), enabling, renaming or
   converting one is refused (HTTP 403). Deleting is allowed.
2. *Secret files:* a new `*_file` path from the app must be inside
   `data/secrets/` (also after resolving symlinks). Paths set in the file on the
   host are left alone.
3. *Secrets stay bound to their destination:* the app cannot attach a secret
   header to a widget/webhook or move a secret-bearing one to another URL, and
   cannot change a Telegram channel's `api_base`, unless that exact pair is
   already in the file. Otherwise a stolen session could send `RH_TELEGRAM_TOKEN`
   to its own server. The **Fetch now** preview follows the same rule.
4. Pasted secret-looking values (Slack/Discord webhook URLs, Telegram tokens)
   produce a warning that names the line, never the value.

**Tabs:**

- *General:* Title, Greeting, Search URL (an http(s) link with `{q}`), Show
  stopped containers, Show live CPU and memory. Theme is set under Appearance;
  Columns and Layout are no longer offered. Existing `columns`, `layout` and
  `theme` keys in `[page]` (and `columns` / `layout` in `[[groups]]`) stay in
  the file untouched and are still accepted (see configuration.md for what
  they still do). A new title shows in the sidebar and tab at once.
- *Containers and groups:* Hidden containers (one pattern per row, how many
  containers it hides now, a live "matches" line in the dialog), Groups
  (Name, Title, Icon, Collapsed, Hidden; one-click up / down), and every
  container with its override dialog (Display name, Group, Icon, URL,
  Description, Hide, Order, Card size; "Remove overrides…" asks and offers
  Undo). A group's Icon and a container's Card size are saved, but the
  current pages do not draw them. Search and "Only with overrides" filter
  the list ; the bulk bar (Set
  group, Hide, Show, Remove overrides…, Clear selection) stays in view and
  each action is one revision with Undo. A pattern is added the way the `exclude` array is written (one per
  line with the same indentation when it is multi-line), deleting one takes
  its own comments with it, and a pattern with a line break or other control
  character is refused. `POST /api/settings/docker` and `/api/settings/groups`
  take one entry at a time (`{base_hash, action, id?, item?, order?}`); the
  older whole-list bodies are refused (400).
- *Bookmarks:* Name, URL, Group, Icon, Description; one-click up / down. A
  new or changed URL must start with `http://` or `https://`.
- *Widgets:* the System tiles switch, the `custom-api` widgets and, read
  only, the `command` widgets: see [widgets.md](widgets.md).
- *Notifications:* channels and alert rules: see
  [notifications.md](notifications.md).
- *Appearance:* Default theme (saves when changed; "Use the default here"
  drops this browser's own choice) and `data/custom.css` (atomic, 64 KiB
  max, event recorded, no revision; live preview, a warning for a brace,
  comment or string that does not close, Revert, and after a save "Previous
  version" until the page is left). When the
  saved custom.css sets a retired token name (`--ctl`, `--border-2`,
  `--accent-bg`, `--glow`, `--pop`, `--fs-label`, or `--border` without
  `--line`) a note names the token to set instead (`--glow`: remove it).
- *History and data:* Recording on / off (saves when switched), Sample
  interval, the database figures, export, and Clear history, which asks in
  the confirm dialog with the phrase "clear history" typed.
- *Config file:* the raw editor (Ctrl+S or Cmd+S saves; Revert; "Go to
  error"), the revisions 25 a page,
  newest first ("Newer" / "Older" links, `?rp=2` for the second page, which
  the script loads in place; a save or restore goes back to the first page),
  View (with "Open in the editor") and Diff in a dialog, Restore after a
  confirm.
- *Security:* recent authentication events and account links. Password and
  session management live on `/app/account`; password changes revoke sessions.
