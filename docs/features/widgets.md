# Widgets

- **custom-api:** fetch JSON from any URL, pick fields with paths, show them
  with a template or rows, optional status dot. Add and edit them in
  Settings → Widgets, with a **Fetch now** preview before saving.
- **command:** run a program in the container and show its JSON. Off unless
  `enabled = true`, and **file-only**: the app can never create, change or enable
  one (it would be remote command execution in a container with the Docker
  socket). In Settings they are listed read-only with a lock.
- One failing widget never breaks the page; each has its own timeout and
  refresh; widgets are fetched only while someone is viewing (or an alert rule
  uses them).
- Header secrets (`headers_env`, `headers_file`) are set in the file on the
  host: they are bound to their URL so the app cannot redirect them.

Reference: README "Add a widget".
