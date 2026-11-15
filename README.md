<p align="center">
  <img src="docs/brand/social-preview.png" alt="Hobnail: a tiny home for your homelab" width="720">
</p>

# Hobnail

**A tiny home for your homelab.**

Hobnail is a home-server dashboard written in Rust: one static binary in a
~10 MB `scratch` image, idling at about 1 MiB of RAM. It discovers every Docker
container by itself, shows host stats and their history, tails logs, pulls data
from any JSON API, sends notifications when something breaks and recovers, and
can be configured from the browser behind a login.

## Quick start

You need Docker with Compose.

```sh
git clone https://github.com/Rami-0/hobnail.git
cd hobnail
docker build -t hobnail:latest .
docker compose -f deploy/compose.yaml up -d
```

Open <http://127.0.0.1:3200>. With no configuration it lists every container on
the host, grouped by Compose project. To change settings from the browser, set
a password once:

```sh
docker exec -it hobnail /hobnail set-password
```

## Documentation

| Topic | Read |
|---|---|
| Install, publish on your network, redeploy, CLI, environment variables | [docs/install.md](docs/install.md) |
| `config.toml`: page, groups, containers, labels, icons, theming | [docs/configuration.md](docs/configuration.md) |
| Login and what it protects | [docs/features/login.md](docs/features/login.md) |
| The interface, keyboard shortcuts, themes | [docs/features/ui.md](docs/features/ui.md) |
| Container discovery | [docs/features/containers.md](docs/features/containers.md) |
| Logs | [docs/features/logs.md](docs/features/logs.md) |
| Host page | [docs/features/host.md](docs/features/host.md) |
| History and its JSON API | [docs/features/history.md](docs/features/history.md) |
| Widgets | [docs/features/widgets.md](docs/features/widgets.md) |
| Notifications and alert rules | [docs/features/notifications.md](docs/features/notifications.md) |
| Settings in the app | [docs/features/settings.md](docs/features/settings.md) |
| Spec, design and measurements | [GOALS.md](GOALS.md), [docs/DESIGN.md](docs/DESIGN.md), [docs/measurements.md](docs/measurements.md) |
| Brand | [docs/BRANDING.md](docs/BRANDING.md) |

Each topic has one page. The README only points to them.
