# Settings in the app

`/settings`, signed in (see [login.md](login.md)). Tabs: General, Containers and
groups, Bookmarks, Widgets, Notifications, Appearance, History and data, Config
file, Security. `/settings` itself is the General tab (there is no index page);
the Settings links point at the tab this browser used last (the script
remembers it; without it they open General). On a phone the tab strip fades
at the edge that has more tabs behind it. Signed out (or before
`set-password`), every tab shows only a lock panel: no tab strip, no config
data. The bookmark and widget dialogs also open from "Add" on the Overview,
and the channel and rule lists also sit on the Alerts page (signed in only;
see [notifications.md](notifications.md)).

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
  the same dialog. Customize on a container page opens its override dialog
  (`?name=<container>` still works). Esc, ×, Cancel or the backdrop close it, and ask first when
  something was typed. Enter saves; the button shows progress; the list behind
  updates in place and the changed row flashes.
- Lists and form values are rendered by the server, for a session only.
  `GET /settings/<tab>?frag=list` serves the lists again for those updates
  (Bookmarks, Widgets, Notifications, Containers and groups; the database
  figures on History and data; the revisions on Config file; 401 without a
  session). Signed out nothing of it is in the page.
- No staged lists: every add, edit and delete saves when confirmed, also for
  Hidden containers and Groups (each with Undo where it deletes).
- Delete asks in one confirm dialog; the message after it has **Undo**, which
  restores the revision from before the delete (refused if the file changed
  since). Up / down arrows (bookmarks, widgets) move an entry one step, one
  revision per step (bookmarks, widgets and groups). A channel or widget that alert rules still use is not
  offered for deleting: the message names the rules.
- An edit sends only the fields you changed; the server keeps every key the
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
  interval, custom.css, the raw editor, Change password) use the same submit
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
- **Without JavaScript** every page still shows its values, but nothing can be
  saved: writes need the `X-RH-CSRF` header, which a plain form cannot send.
  Every form is `method="post"` (so no value, a password least of all, can end
  up in a URL), its save button is disabled, and a note says why (also while
  the script is still loading, or if it failed to load). Settings pages are
  served `Cache-Control: no-store`, as is every page or fragment rendered
  with a session (the Overview and Alerts carry Settings dialogs). A plain form
  post that reaches the server anyway is refused by the write guard.
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
  Undo). Search and "Only with overrides" filter the list (without
  JavaScript they are shown disabled); the bulk bar (Set group, Hide, Show,
  Remove overrides…) stays in view and each action is one revision with
  Undo. A pattern is added the way the `exclude` array is written (one per
  line with the same indentation when it is multi-line), deleting one takes
  its own comments with it, and a pattern with a line break or other control
  character is refused. `POST /api/settings/docker` and `/api/settings/groups`
  take one entry at a time (`{base_hash, action, id?, item?, order?}`); the
  older whole-list bodies are refused (400).
- *Appearance:* Default theme (saves when changed) and `data/custom.css`
  (atomic, 64 KiB max, event recorded, no revision; live preview). When the
  saved custom.css sets a retired token name (`--ctl`, `--border-2`,
  `--accent-bg`, `--glow`, `--pop`, `--fs-label`, or `--border` without
  `--line`) a note names the token to set instead.
- *History and data:* Recording on / off (saves when switched), Sample
  interval, the database figures, export, and Clear history, which asks in
  the confirm dialog with the phrase "clear history" typed.
- *Config file:* the raw editor (Ctrl+S saves), the latest 25 revisions
  (`?revs=all` for all), View and Diff in a dialog, Restore after a confirm.
- *Security:* change password (signs out every session; all problems shown at
  once, a wrong current password on its field, passwords never trimmed), sign
  out other sessions (asks first), recent logins.
