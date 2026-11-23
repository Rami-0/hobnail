# Vendored icons

The SVG files in this directory are built into the server binary and served
from `/api/v1/icons/<name>` and `/icons/di/<name>.svg` (see
`src/http/icons.rs`). They need no remote fetch, so they work with
`[icons] remote = false`.

## Brand logos

Source: [homarr-labs/dashboard-icons](https://github.com/homarr-labs/dashboard-icons),
`svg/<name>.svg` at commit `d4b500dc40a8e1043b1581cabb7208892cf35472`
(2026-11-16). Licence: Apache License 2.0
(<https://github.com/homarr-labs/dashboard-icons/blob/main/LICENSE>).

The files were minified (comments, editor metadata and unreferenced ids
removed, coordinates rounded relative to the view box); their artwork is
unchanged.

> All product names, trademarks, and registered trademarks are the property
> of their respective owners. Icons are used for identification purposes only
> and do not imply endorsement. (dashboard-icons README, "Legal")

Files: adguard-home, beszel, caddy, docker, dockge, dozzle, duplicati, gitea,
github, glances, grafana, home-assistant, immich, jellyfin, jellyseerr,
mailpit, mariadb, minio, mongodb, mysql, nextcloud, nginx, nginx-proxy-manager,
nodejs, odoo, paperless-ngx, pi-hole, plex, postgres, prometheus, prowlarr,
proxmox, qbittorrent, radarr, redis, sabnzbd, sonarr, syncthing, tailscale,
technitium, traefik, transmission, truenas, uptime-kuma, wireguard.

## Generic glyphs

`bookmark`, `clock`, `http`, `markdown`, `network`, `open-meteo`, `rss` and
`ssl` stand for providers without a brand logo. They are
[Lucide](https://lucide.dev) icons (`bookmark`, `clock`, `globe`, `file-text`,
`activity`, `cloud-sun`, `rss`, `shield-check`, lucide-react 1.70.0), stroked
in the provider's colour. Licence: ISC, Copyright (c) 2026 Lucide Icons and
Contributors.
