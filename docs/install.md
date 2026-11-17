# Install, run and redeploy

Hobnail runs as one container next to the ones it watches. You need Docker with
Compose.

## First run

```sh
git clone https://github.com/Rami-0/hobnail.git
cd hobnail
docker build -t hobnail:latest .
docker compose -f deploy/compose.yaml up -d
```

Open <http://127.0.0.1:3200>. With no configuration it lists every container on
the host, grouped by Compose project, with host stats alongside. Then
[set the password](features/login.md) so you can change settings from the
browser.

## Where things live

Everything Hobnail stores is in the `data/` folder next to the compose file.

| File | What it is |
|---|---|
| `config.toml` | Optional overrides and additions, hot-reloaded: [configuration.md](configuration.md) |
| `hobnail.db` | History: [features/history.md](features/history.md) |
| `custom.css`, `icons/` | Theming and icons: [configuration.md](configuration.md#icons-and-theming) |
| `secrets/`, `../secrets.env` | Tokens for notification channels: [features/notifications.md](features/notifications.md) |

## Build and redeploy

No Rust toolchain is needed on the host; everything builds in Docker.

```sh
scripts/deploy.sh        # build (fmt + clippy + tests gate the build), deploy, health-check
scripts/measure.sh       # RAM / image size / cold start / page load, as markdown
scripts/cargo.sh test    # run cargo in the dev container (also: clippy, fmt, build)
```

`deploy.sh` tags the image `hobnail:latest` and `hobnail:<git sha>`,
copies `deploy/compose.yaml` to the stack folder (`STACK`, default
`/opt/stacks/hobnail`), runs `docker compose up -d`, waits for the healthcheck
and curls every published address.
Open pages notice the new build and reload themselves. The foot of the sidebar
shows the version and the build SHA (hover for the build time).

Manual equivalent:

```sh
docker build --build-arg RH_GIT_SHA=$(git rev-parse --short HEAD) \
             --build-arg RH_BUILD_TIME=$(date -u +%FT%RZ) -t hobnail:latest .
docker compose -f deploy/compose.yaml up -d
```

What the container gets: the Docker socket **read-only** (the app only sends
`GET` requests), its own `./data` folder, and read-only `/proc` and `/sys` for
host stats. It runs as uid 1000 with group 982 (docker) for the socket, a
read-only root filesystem, no capabilities. Set `DOCKER_GID` to your host's
docker group id (`getent group docker`) if it is not 982; `group_add` uses it.

The port is published on `127.0.0.1` only. To open it to your network, add
your own addresses, never `0.0.0.0` unless the host is firewalled:

```yaml
    ports:
      - 127.0.0.1:3200:3200
      - 192.168.1.10:3200:3200     # LAN
      - 100.64.0.10:3200:3200      # VPN, e.g. Tailscale
    environment:
      RH_HOSTNAME: homelab                   # name shown on the page
      RH_ALLOWED_HOSTS: homelab.*.ts.net     # host names the page may be opened by
```

`deploy.sh` also applies a `deploy/compose.local.yaml` if you create one, so
those lines can live outside git. Consider a socket proxy (e.g.
`tecnativa/docker-socket-proxy` with only `CONTAINERS=1 EVENTS=1`) and point
`RH_DOCKER_SOCKET` at it if you want to restrict the API further.

## Command line

CLI (inside the container: `docker exec hobnail /hobnail <cmd>`):

| Command | What it does |
|---|---|
| `serve` | run the dashboard (default) |
| `check-config [path]` | validate a config file, print errors with line/column |
| `notify-test <channel>` | send a test message to a configured channel |
| `set-password` | set or change the login password (TTY without echo, or stdin) |
| `mock-receiver [addr]` | local fake webhook/Slack/Telegram endpoint for testing |
| `healthcheck` | used by the Docker healthcheck |
| `version` | print version, git SHA and build time |

## Environment variables

A restart is needed after changing these: `RH_LISTEN` (default `127.0.0.1:3200`,
comma-separated; the compose file sets `0.0.0.0:3200` inside the container),
`RH_DATA_DIR` (`/data` in the image; `./data` outside it), `RH_CONFIG`
(`$RH_DATA_DIR/config.toml`), `RH_DB` (`$RH_DATA_DIR/hobnail.db`),
`RH_DOCKER_SOCKET` (`/var/run/docker.sock`), `RH_HOST_PROC`/`RH_HOST_SYS`
(`/host/proc`, `/host/sys` in the image; `/proc`, `/sys` outside it),
`RH_HOSTNAME` (default: the host's name from `RH_HOST_PROC`),
`RH_ALLOWED_HOSTS` (extra host names allowed in the `Host` header, globs,
comma-separated; IPs, `localhost` and `RH_HOSTNAME` always work: this blocks
DNS rebinding).
