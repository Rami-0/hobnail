# Settings in the app

`/settings`, signed in (see [login.md](login.md)). Tabs: General, Containers and
groups, Bookmarks, Widgets, Notifications, Appearance, History and data, Config
file, Security. Signed out (or before `set-password`), every tab shows a lock
panel and no config data.

**How a save works** (`src/settings/`): load the file and its SHA-256 →
patch with `toml_edit` (comments, order, blank lines and unknown keys survive)
→ validate with the same parser the server uses → run the guards → write
atomically (temp file + rename, same mode) → add a revision → record a
`config.saved` event → reload the config at once. Open pages refresh.

- **Stale protection:** every form sends the hash it loaded; if the file changed
  on disk in between, the save is refused (HTTP 409) and the page offers to
  reload. A hand edit is never overwritten.
- **Invalid is never written:** HTTP 422 with the field (or line:column) to fix.
- **Revisions:** newest 200, with author and note ("widgets: added mailpit",
  "raw edit", "restored #12"); hand edits show up as "on disk" revisions. Diff
  any revision against the current file; restore creates a new revision.
- **Raw editor:** validate-as-you-type (debounced), errors highlighted by line.

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

Appearance writes `data/custom.css` (atomic, 64 KiB max, event recorded, no
revision). History and data can export a snapshot and clear samples and events.
Security: change password (signs out every session), sign out other sessions,
recent logins.
