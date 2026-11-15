# Containers (Docker discovery)

Every container on the host is discovered from the Docker socket (mounted
read-only; the app only ever sends `GET`), grouped by compose project, with no
config at all. The Docker events stream keeps the list current within ~0.3 s.

- State, health, uptime, published ports, compose project/service, image.
- Live CPU and memory from cgroup v2 files (no Docker stats API calls); history
  from the database.
- Links: ports on `0.0.0.0` use the address you opened the page on; ports on
  specific IPs prefer a non-loopback one; loopback-only shows "local";
  database/mail ports are plain badges. Logs (Dozzle) and metrics (Beszel)
  links are auto-detected.
- Overrides (config or Settings → Containers and groups): name, group, icon,
  url, description, hide, order, size; `[docker] exclude` globs; `[[groups]]`.
- Labels on containers: `hobnail.hide`, `.name`, `.group`, `.icon`, `.url`.
