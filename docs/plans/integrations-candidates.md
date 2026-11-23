# Hobnail integration candidates (D19: choose the first providers)

**How to use this file:** tick `[x]` next to every integration you want. Use `[1]`-style
notes if you want an order, or write "first" / "later" after the name. Anything left
unticked is out of scope for the first PR. When you are done, the implementation prompt
reads this file as the provider list.

Each provider ships with: a setup form (URL plus only the credentials it needs, stored
write-only and bound to the origin, D14), a backend connection test, one or more board
widgets, docs in `docs/features/integrations.md`, and tests against a fake server.
Only real, working providers are listed in the catalog (decision doc section 4).

**Effort hint** (my estimate, not measured): **S** = one simple JSON endpoint, usually an
API key header; **M** = several endpoints or a login/token flow; **L** = session auth,
unusual protocol, or a large API surface.

**Memory note:** providers are polled only while someone has a board open, with bounded
timeouts and caches, so each adds little RAM. Measured (finish-hobnail M5 and M7): the
memory benchmark polls 13 providers against fake servers, 12 of the 27 service and 8
no-service providers plus Generic HTTP (Pi-hole, Uptime Kuma,
Jellyfin, qBittorrent, Sonarr, Radarr, Transmission, Home Assistant, Proxmox VE, Generic
HTTP, RSS / Atom, SSL expiry, reachability; 18 widgets on a board) with 25 open dashboards
and full history, and the whole container peaked at 12.45 MiB without a limit and 12.24 MiB
under a 20 MiB limit, with no errors (details and the M7 re-run:
[measurements.md](../measurements.md)). The other 23 providers share the same connection
context and caps (4 requests at once, 128 KiB per buffered response, a 5 s deadline) but
were not in the benchmark. All ticked lines are built; none is verified against a real
service instance.

---

## Already built (no need to tick)

- [x] Generic HTTP (`custom-api` style) widget: any JSON URL, mapped to fields
- [x] Docker containers, stacks, logs, host metrics, history, alerts (native, not integrations)

---

## Network and DNS

- [x] Pi-hole: queries, blocked %, top clients/domains, enable/disable status. **S**
- [x] AdGuard Home: queries, blocked %, protection status. **S**
- [x] Technitium DNS: queries, blocked, zones. **M**
- [ ] Unbound / blocky stats. **M**
- [ ] UniFi Network: clients, devices up/down, WAN throughput. **L**
- [ ] OPNsense: gateway status, interface traffic, firewall counters. **M**
- [ ] pfSense: gateway status, interface traffic. **M**
- [ ] MikroTik RouterOS: interface traffic, clients, CPU. **M**
- [ ] OpenWrt (ubus/LuCI): clients, load, WAN. **L**
- [x] Tailscale: devices online, last seen, key expiry. **S**
- [ ] Headscale: nodes, users, online state. **M**
- [x] WireGuard (wg-easy): peers, last handshake, transfer. **M**
- [x] Nginx Proxy Manager: proxy hosts, certificate expiry. **M**
- [ ] Traefik: routers, services, errors. **S**
- [ ] Caddy (admin API): upstream health. **S**
- [ ] Cloudflare: tunnel status, DNS records, analytics. **M**
- [ ] Speedtest Tracker: last download/upload/ping. **S**
- [ ] Gluetun: VPN status, public IP, region. **S**
- [ ] Authentik: users, recent logins, failed logins. **M**
- [ ] Authelia: health only. **S**
- [ ] Keycloak: realm, sessions. **L**

## Monitoring and uptime

- [x] Uptime Kuma: monitor states, uptime %, incidents. **S**
- [x] Beszel: systems with CPU, memory and disk, fleet summary. **M**
- [ ] Netdata: key metrics, alarms. **M**
- [ ] Prometheus: arbitrary instant query as a number or sparkline. **M**
- [ ] Grafana: alert state, dashboard link health. **M**
- [ ] Healthchecks.io: check states, last ping. **S**
- [ ] Gatus: endpoint results. **S**
- [x] Glances: CPU, memory, sensors, disk of another host. **S**
- [ ] Scrutiny: S.M.A.R.T. drive health. **S**
- [ ] Dozzle: link only (no API; already covered by Hobnail logs).
- [ ] ntfy: recent messages, topic stats. **S**
- [ ] Gotify: recent messages, app list. **S**

## Media: servers

- [x] Jellyfin: active streams, library counts, latest added. **S**
- [x] Plex: active streams, library counts, recently added. **M**
- [ ] Emby: active streams, library counts. **S**
- [ ] Navidrome / Subsonic API: now playing, library counts. **S**
- [ ] Audiobookshelf: libraries, listening now. **S**
- [ ] Kavita: libraries, recently added. **M**
- [ ] Komga: libraries, recently added. **S**
- [ ] Calibre-Web: book counts. **M**
- [x] Immich: photos/videos counts, storage, jobs. **S**
- [ ] PhotoPrism: counts, indexing state. **M**
- [ ] Tautulli: Plex play stats, top users. **S**

## Media: requests and automation (the "*arr" family)

- [x] Sonarr: queue, upcoming, missing, health. **S**
- [x] Radarr: queue, upcoming, missing, health. **S**
- [ ] Lidarr: queue, upcoming, wanted. **S**
- [ ] Readarr: queue, wanted. **S**
- [x] Prowlarr: indexer health, grabs. **S**
- [ ] Bazarr: missing subtitles. **S**
- [x] Jellyseerr / Overseerr: pending requests, approve count. **S**
- [ ] Tdarr: transcode queue, errors. **M**
- [ ] Huntarr / Maintainerr / Recyclarr: status only. **M**

## Downloads

- [x] qBittorrent: speeds, active/seeding/queued, free space. **M**
- [x] Transmission: speeds, active torrents. **M**
- [ ] Deluge: speeds, active torrents. **M**
- [x] SABnzbd: queue, speed, free space. **S**
- [ ] NZBGet: queue, speed. **S**
- [ ] aria2: active downloads, speed. **M**
- [ ] JDownloader (MyJDownloader): status. **L**

## Home automation and IoT

- [x] Home Assistant: chosen entity states, counts of on/off, sensor values. **M**
- [ ] Zigbee2MQTT: device list, battery low, offline. **M**
- [ ] ESPHome: node states. **M**
- [ ] Frigate: camera state, recent events. **M**
- [ ] Scrypted: camera list. **L**
- [ ] MQTT broker topic value (Mosquitto): one topic as a number or state. **M**
- [ ] Octoprint / Moonraker (3D printers): job progress, temps. **S**
- [ ] Philips Hue bridge: lights on, groups. **M**
- [ ] Shelly devices: power, state. **S**
- [ ] Tasmota: power, state. **S**

## Files, sync and backup

- [x] Nextcloud: storage, users, activity. **M**
- [x] Syncthing: folders in sync, devices online. **S**
- [ ] Filebrowser: link only (no useful API). 
- [x] Duplicati: last backup, result, size. **S**
- [ ] Restic REST server / restic exporter: last snapshot age. **M**
- [ ] Borgmatic / Borg: last archive age (via exporter). **M**
- [ ] Kopia: snapshot status. **M**
- [ ] Backrest: plans, last run. **S**
- [ ] MinIO: buckets, usage, health. **M**
- [ ] Seafile: libraries, storage. **M**

## Storage and virtualization

- [x] Proxmox VE: nodes, VMs/CTs up, CPU/RAM. **M**
- [ ] Proxmox Backup Server: last backups, datastore usage. **M**
- [x] TrueNAS SCALE: pools, alerts, disk health. **M**
- [ ] OpenMediaVault: disks, shares, services. **L**
- [ ] Synology DSM: volume usage, temperature. **L**
- [ ] Unraid: array state, disk usage, containers. **M**
- [ ] Portainer: endpoints, container counts. **S**
- [ ] Dockge / Komodo: stack list and state. **M**
- [ ] Kubernetes (read-only): pod/node counts. **L**

## Developer and self-hosted tools

- [x] Gitea / Forgejo: repos, open issues/PRs, actions status. **S**
- [x] GitHub: repo stars, open PRs/issues, workflow status. **S**
- [ ] GitLab: pipelines, open MRs. **M**
- [ ] Woodpecker / Drone CI: latest builds. **M**
- [ ] Jenkins: job states. **M**
- [ ] Vaultwarden: link only (admin API is sensitive; not recommended).
- [x] Paperless-ngx: document counts, inbox. **S**
- [ ] Mealie / Tandoor: recipes, meal plan today. **M**
- [ ] Linkwarden / Linkding / Karakeep: bookmark counts, recent. **S**
- [ ] FreshRSS / Miniflux: unread counts. **S**
- [ ] Wallabag / Readeck: unread articles. **M**
- [ ] Vikunja / Planka / Wekan: open tasks. **M**
- [ ] Actual Budget / Firefly III: balances, recent spending. **M**
- [ ] Ghost / WordPress: post counts, drafts. **M**
- [ ] Changedetection.io: changed watches. **S**
- [ ] Homebox / Grocy: stock below minimum, due chores. **M**

## Feeds, information and utilities (no self-hosted service needed)

- [x] RSS / Atom feed list. **S**
- [x] Weather (Open-Meteo, opt-in, no key). **S**
- [x] Clock and calendar (multiple time zones). **S**
- [ ] iCal / CalDAV calendar (Nextcloud, Radicale, Google link): next events. **M**
- [x] Bookmarks groups (local, no network). **S**
- [x] Markdown note (local). **S**
- [ ] iframe embed (strict CSP and sandbox). **M**
- [ ] Reddit / Hacker News top stories. **S**
- [x] GitHub releases watcher: "new version available" for the apps you run. **S**
- [ ] Docker image update awareness (opt-in registry check). **M**
- [ ] Prometheus-style `/metrics` scrape of any exporter, shown as one number. **M**
- [ ] Webhook receiver: other tools push a status or number to a widget. **M**
- [x] SSL certificate expiry for any hostname. **S**
- [x] TCP / HTTP / DNS / ping reachability check as a status tile. **S**

---

## Your extras

Add anything missing here (name, what you want to see on the board):

- [ ] 
- [ ] 

---

## Selected for the single PR

Everything ticked above is in scope. Nothing else is. The selection is deliberately
lean: well-known apps with simple, documented APIs, grouped so that each integration
*shape* is proven once (API key header, status list, media server, login/session client,
*arr pattern, feed with no service). Adding a provider later should be a small, repeatable
change that follows the pattern the first ones set.
