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
- **Errors on the field, in plain words:** a form's 422 names the field that
  caused it ("Use a duration like 30s or 5m.", "Use a whole number.", "Use a
  link that starts with http:// or https://.") and carries no config-file line;
  the browser checks required fields, links, numbers and lengths first and
  shows every problem at once.

**Forms** (the ux-pass dialog, DESIGN.md §11.6; Bookmarks so far, the other
tabs follow):

- Add and edit open in a dialog from the row or button you clicked; the URL
  says which (`?add=bookmark`, `?edit=bookmark:2`), so a reload or a link opens
  the same dialog. Esc, ×, Cancel or the backdrop close it, and ask first when
  something was typed. Enter saves; the button shows progress; the list behind
  updates in place and the changed row flashes.
- Lists and their values are rendered by the server, for a session only.
  `GET /settings/<tab>?frag=list` serves the list again for those updates (401
  without a session). Signed out nothing of it is in the page.
- Delete asks in one confirm dialog; the message after it has **Undo**, which
  restores the revision from before the delete (refused if the file changed
  since). Up / down arrows move an entry one step, one revision per step.
- Every dialog, confirm and arrow click saves against the file version it
  was started from; nothing is refreshed under an open dialog. If the file
  changed meanwhile the save is refused and the dialog says so, with "Reload
  and keep my input": an edit then goes on only if the same entry is found
  again with all its original values, otherwise it cannot be saved (Discard).
  Session expired: sign in in a new tab, then save again; the input stays.
- **Without JavaScript** every page still shows its values, but nothing can be
  saved: writes need the `X-RH-CSRF` header, which a plain form cannot send.
  Every form is `method="post"` (so no value, a password least of all, can end
  up in a URL), its save button is disabled, and a note says why (also while
  the script is still loading, or if it failed to load). Settings pages are
  served `Cache-Control: no-store`. A plain form
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

Appearance writes `data/custom.css` (atomic, 64 KiB max, event recorded, no
revision). History and data can export a snapshot and clear samples and events.
Security: change password (signs out every session; all problems shown at
once, a wrong current password on its field, passwords never trimmed), sign out
other sessions, recent logins.
