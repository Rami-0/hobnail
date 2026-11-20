> Historical: this file describes work before the React port. Current UI and
> installation contracts are in README.md, docs/DESIGN.md and docs/install.md.

# Goal

Build `hobnail` in this directory (`/home/dev/projects/rust-homepage`) into a
working, deployed dashboard **today**, running on this server and reachable over
Tailscale, so I can open it from my phone and laptop and watch it improve while you
work. Read `GOALS.md` first. It is the spec: targets, non-goals, milestones and the
decisions already made. Follow it. Where it is silent, use the defaults below
instead of stopping to ask.

# Scope for today

Deliver milestones **M0 to M4** from `GOALS.md`, deployed and measured. Do **M5**
(theming and polish) only if M0 to M4 are done and verified. **Do not do M6.** The
cutover from the current Homepage needs my sign-off, so leave Homepage running and
untouched.

# Where it runs and how I see it

- Deploy as a compose stack at `/opt/stacks/hobnail/compose.yaml`, the same
  layout as `/opt/stacks/dozzle` (`container_name`, `restart: unless-stopped`, a
  `./data` volume, a healthcheck).
- Port **3200** (checked free). Publish it **only** on the two addresses Dozzle uses:
  `192.168.8.200:3200` and `100.125.149.51:3200` (this machine's Tailscale IP, host
  `codexdev`). Never `0.0.0.0`.
- It must be reachable at `http://100.125.149.51:3200` from my other tailnet
  devices. Verify with `curl` from this machine against both addresses.
- **Deploy early and often.** Get a first visible page running within your first
  hour (M0), then redeploy after every milestone so I can watch progress live. Put
  the git short SHA, build time and uptime in the page footer so I can tell which
  build I am looking at.
- **Do not** use `tailscale funnel` or anything that exposes this to the public
  internet. `tailscale serve` for HTTPS on the tailnet is optional and only if it
  works without touching my other Tailscale settings. Tell me if you use it.

# Building

- There is **no Rust toolchain on the host.** Do not install one. Build with Docker
  (multi-stage `Dockerfile`, BuildKit cache mounts for the cargo registry and
  `target`, a musl static target, final stage `scratch` or distroless). Docker 29
  and buildx are installed. Pulling a `rust` image is fine.
- Run `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test` inside the
  build (or a build stage) so the same checks gate every deploy.
- Disk is fine (338 GB free), but clean up dangling images and build cache you create.

# Defaults for the open decisions

- **Auth (D3):** none in v1. Network-level only: bound to the LAN and Tailscale IPs.
- **Labels (D4):** keep it small. Support `hobnail.hide=true`,
  `hobnail.name`, `hobnail.group`, `hobnail.icon` and
  `hobnail.url` on containers. Do not copy Homepage's label set.
- **Name (D5):** Hobnail is the approved name; use `hobnail` for the binary and image.
  The existing live stack remains at `/opt/stacks/rust-homepage` until a separate cutover.
- **Alert state (D6):** a small state file under `./data`. No database.
- **Config:** TOML at `/opt/stacks/hobnail/data/config.toml`. An empty or
  missing file must still produce a useful page (see "High-level, not exhaustive" in
  `GOALS.md`). Hot reload it, and keep the last good config if an edit is invalid.

# What "done" means (all must be true, and shown with evidence)

1. **M0 to M4 work on the deployed stack**, not only in tests. Check each in a real
   browser or with `curl`, and say what you checked.
2. **Auto-discovery:** with an empty config, the page lists every container on this
   host, grouped by compose project, with running vs stopped shown correctly, health,
   published ports and working links.
3. **Custom API widget** shows live data from at least 3 services on this host (for
   example Beszel on `:8090`, Mailpit on `:8025`, Odoo on `:8070`), and one command
   widget works. A deliberately broken widget must not break the page.
4. **Notifications** work for a generic webhook, Slack and Telegram, with dedupe,
   cooldown and recovery messages. Test them against a **local mock receiver** you
   start yourself. I have not given you real Slack or Telegram credentials. Read
   channel secrets from environment variables or files, never log them, and
   document in the README exactly where I put real tokens. Do not invent tokens or
   contact real services.
5. **Measured against the targets in `GOALS.md` section 1.** Record idle RAM
   (`docker stats --no-stream`), image size, cold start and page load in
   `docs/measurements.md`, next to the baselines already in `GOALS.md`. If a target is
   missed, say so plainly and by how much, and do not quietly relax it.
6. **Docker socket is mounted read-only**, and the app never writes to Docker.
7. `README.md` explains how to run, configure, add a widget, add a channel, and
   redeploy.

# Working rules

- Keep `PROGRESS.md` in this directory: after each milestone, what works, what
  does not, what you measured, and the URL. Commit to git after each milestone with
  a clear message. Do not add a remote and do not push.
- **Do not touch** the running Homepage, Dockge, Dozzle or Beszel stacks, or any
  container you did not create. Do not stop or restart other containers. No `sudo`
  (it is not available anyway). Do not change anything outside this project and
  `/opt/stacks/hobnail`.
- Do not mount more of the host into the container than the socket (read-only),
  its own data folder, and read-only `/proc` and `/sys` for host stats.
- Prefer a smaller dependency and fewer threads when the choice is close. RAM is the
  headline goal. If a crate or design pushes idle RAM over target, measure it,
  report it, and choose the lighter option.
- Do not stop to ask me unless you are blocked on something only I can provide (a
  real credential, or a decision that changes the design). Make a reasonable
  choice, write it into `PROGRESS.md`, and keep going.
- Verify before claiming. If something is untested or only partly working, say so.

# Final report

When you stop, tell me:
- The URL to open and what I should see.
- Which of M0 to M5 are done and verified, and which are not.
- The measured numbers against the targets.
- Anything you chose for me that I should review.
- Exactly what is left before the M6 cutover.
