# Install, run and redeploy

Hobnail runs as one container next to the ones it watches. You need Docker with
Compose. Building and deploying also requires Python 3.9 or newer, curl with
Unix socket support, and access to a local Docker daemon over a Unix socket
for the image-size gate.

## First run

```sh
git clone https://github.com/Rami-0/hobnail.git
cd hobnail
docker build -t hobnail:candidate . &&
  scripts/check-image-size.py hobnail:candidate &&
  docker tag hobnail:candidate hobnail:latest &&
  docker compose -f deploy/compose.yaml up -d
```

Open <http://127.0.0.1:3200/app> and set the owner username and password in
the browser. No console step is required for a fresh instance. The first visitor
claims it, so keep access restricted while setting up. New boards start empty;
choose their contents in Settings → Boards and edit mode. Discovery is available
on the Containers page, subject to account scopes.

Use a fresh, writable data directory for a new installation or a throwaway test.
Do not reuse a production database for experiments. Build-time Node/Rust tools
are containerized; neither runs on the host or as a production sidecar.

## Where things live

Everything Hobnail stores is in the `data/` folder next to the compose file.

| File | What it is |
|---|---|
| `config.toml` | Optional overrides and additions, hot-reloaded: [configuration.md](configuration.md) |
| `hobnail.db` | Accounts, sessions, boards, integrations, settings, drafts and history: [features/history.md](features/history.md) |
| `custom.css`, `icons/` | Theming and icons: [configuration.md](configuration.md#icons-and-theming) |
| `secrets/`, `../secrets.env` | Tokens for notification channels: [features/notifications.md](features/notifications.md) |

## Build and redeploy

No Rust or Node toolchain is needed on the host; everything builds in Docker. The build needs network access to crates.io and the npm registry.

```sh
scripts/deploy.sh        # build (fmt + clippy + tests gate the build), deploy, health-check
PORT=3395 scripts/bench-mem.sh hobnail-rp:m5 /tmp/hobnail-bench # throwaway image required
scripts/cargo.sh test    # run cargo in the dev container (also: clippy, fmt, build)
```

`deploy.sh` uses Docker BuildKit and the local Unix-socket connection
described above. It builds `hobnail:<git sha>`, checks the 13,500,000-byte
image limit, and promotes the image to `hobnail:latest` only if the gate passes.
It needs write access to the stack folder. It then copies `deploy/compose.yaml` to the stack folder (`STACK`, default
`/opt/stacks/hobnail`), runs `docker compose up -d`, waits for the healthcheck
and curls every published address.
`deploy.sh` asks for confirmation; `DEPLOY_YES=1` is the explicit unattended
opt-in. Open pages notice the new build and offer to reload. The foot of the sidebar
shows the version and the build SHA (hover for the build time).

Manual equivalent:

```sh
docker build --build-arg RH_GIT_SHA="$(git rev-parse --short HEAD)" \
             --build-arg RH_BUILD_TIME="$(date -u +%FT%RZ)" -t hobnail:candidate . &&
  scripts/check-image-size.py hobnail:candidate &&
  docker tag hobnail:candidate hobnail:latest &&
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

## Upgrading from the old UI

Before upgrading, stop Hobnail and copy the data volume, including
`hobnail.db` and any SQLite WAL sidecar, to a separate backup location.
For an online backup, use SQLite's backup command to make a consistent copy.
Keep the previous image or binary version. The provider framework upgrades
the database to schema version 14. Older builds refuse to open a database
with a newer schema version and run with database-backed features disabled.

To roll back, stop the upgraded instance and keep its data volume separately
if you may need changes made since the upgrade. Restore the pre-upgrade data
into a clean volume or directory, including any sidecars from that same
backup, then start the previous build with the restored data. The restored
database contains the state at the time of the backup.

If the database had a legacy password, browser ownership claims are locked.
Run `hobnail set-password [username]` on the host to recover the owner. Old
sessions are discarded. Fresh databases use browser setup instead.

Existing account passwords hashed with the old 19 MiB Argon2id profile keep
that work factor until the next successful sign-in for each account rehashes
its password. That first sign-in peaked at about 20.5 MiB in M5. Temporarily
raise a hard 20 MiB container memory limit for the first sign-in after
upgrading. The shipped Compose and deploy configurations set no memory limit.

Reads from a cold SQLite database can increase reported container memory
because the kernel charges file-cache pages to the container that reads
them. This cache is reclaimable under memory pressure. The capped M5 cold
run completed startup, authentication with the new hash profile, and 40
full-history reads under the 20 MiB cap with no OOM kills.
[M5 memory results](plans/react-port-phase-2-5.md#finish-hobnail-m5-server-memory)

The old UI exists on `rust-only`. Its URLs are gone; `/` returns 404 with a
link to `/app`, without redirecting. There is no RH_UI=classic fallback or
old-URL compatibility. The React application needs JavaScript.

## Command line

CLI (inside the container: `docker exec hobnail /hobnail <cmd>`):

| Command | What it does |
|---|---|
| `serve` | run the dashboard (default) |
| `check-config [path]` | validate a config file, print errors with line/column |
| `notify-test <channel>` | send a test message to a configured channel |
| `set-password [username]` | optional owner recovery (TTY without echo, or stdin); revoke sessions |
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

## Not built yet

The native systemd package, multi-architecture export target and installer
described in the planning documents are not shipped by this Dockerfile. The
container build above is the supported procedure verified in this milestone.
