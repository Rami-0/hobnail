# Containers (Docker discovery)

Every container on the host is discovered from the Docker socket (mounted
read-only; the app only ever sends `GET`), grouped by compose project, with no
config at all. The Docker events stream keeps the list current within ~0.3 s.

- State, health, uptime, published ports, compose project/service, image.
- Live CPU and memory from cgroup v2 files (no Docker stats API calls); history
  from the database.
- Links: ports on `0.0.0.0` use the address you opened the page on; ports on
  specific IPs prefer a non-loopback one; loopback-only shows "local";
  database/mail ports are plain badges. Logs and metrics are in the app (the
  container's Logs and Metrics tabs); there are no links to Dozzle or Beszel.
- Logs are one click from every place a container is named: "Logs" in the
  table row (first action), a logs icon on each sidebar item (and on a stack,
  for compose projects), on Overview's "Needs attention", "Top CPU" and
  "Recent events" rows, on Host, History and Alerts rows, and in Settings ›
  Containers and groups. The container page's Logs tab is the viewer.
- Why a container is red is one click from Overview. Admin/Owner can see
  probe diagnostics and recent health checks; Viewers never receive them.
  Since-times are clamped to the current container start.
- Overrides (config or Settings → Containers and groups): name, group, icon,
  url, description, hide, order; `[docker] exclude` globs; `[[groups]]`.
  `size` is still accepted and offered ("Card size") but has no effect: there
  is no card grid.
- Labels on containers: `hobnail.hide`, `.name`, `.group`, `.icon`, `.url`
  (the older `rust-homepage.*` labels still work; `hobnail.*` wins).

The React inventory is `/app/containers`; data requires login and respects
account scopes. Scoped accounts see only containers targeted by granted boards.
Discovery does not populate new boards automatically. Inspect environment
values and process commands require Admin/Owner.
