# Login and security

Application data requires login. Open `/app` on first run and set the owner
username and password in the browser (at least eight characters). The first
visitor claims ownership; publish the instance only on trusted addresses during
setup. No console step is required for a fresh database.

Sign in with username and password. The client returns to the requested `/app`
path; foreign origins, protocol-relative paths, backslashes and control
characters are refused. The application needs JavaScript.

The Owner invites Admin and Viewer accounts, assigns roles and grants board
and integration scopes in Settings → Users. An unrestricted Admin edits normal
settings; a Viewer reads data and manages their own account. Scoped Admins can
edit granted boards/integrations, but cannot create/delete them or edit global
settings. Scoped accounts see containers only through granted boards; host-wide
Overview, Host, History, Widgets, Bookmarks and Alerts reads are refused.
An empty scope grants nothing; an unrestricted account has no scope restriction.

The Account page changes a password using the current password, lists sessions,
and revokes one or all other sessions. Password changes revoke sessions,
including the current one. The Owner can disable users, change scopes/roles,
sign them out and delete invited users; the sole Owner cannot be demoted,
disabled or deleted. Invitations are single-use, expire, and can be revoked.

**How it is enforced**

- The shared route gate in `src/api/access.rs` checks roles and scopes before
  dispatch. [The API reference](../api.md) lists exceptions for credential
  entry and public shell/assets/health probes.
- Every write requires an Origin (or Referer) matching Host. Authenticated
  writes additionally require `X-RH-CSRF`. `GET /api/v1/session`, `no-store`,
  supplies the token only for a valid cookie; retrying after sign-in preserves
  unsaved input without reloading the document.
- Unknown Host names return 421. IP literals, localhost, `RH_HOSTNAME` and
  `RH_ALLOWED_HOSTS` globs are accepted; `/healthz` is exempt.
- New passwords are stored as salted Argon2id hashes in SQLite, using
  7,168 KiB, five passes and one lane. This is one of the
  [equivalent OWASP profiles](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#argon2id).
  A successful login verifies a legacy 19,456 KiB/two-pass hash with its
  original settings and then replaces it atomically. A failed upgrade write
  leaves the original hash and the new session valid. The server runs one
  credential operation at a time on a blocking worker. Eight requests can
  queue asynchronously for up to five seconds, retaining only parsed
  credentials and request state. A full queue or an expired wait returns
  429 with `Retry-After: 1`.
- Sessions use 32 random bytes; only their SHA-256 is stored. The cookie is
  `HttpOnly; SameSite=Strict; Path=/; Max-Age=604800`. Database expiry and cookie
  slide on authenticated responses. Hobnail adds `Secure` when a
  [trusted proxy](../configuration.md#incoming-reverse-proxy) reports HTTPS.
  Plain HTTP continues to work.
- Failed logins use per-client-IP and global exponential back-off (1, 2, 4 …
  60 seconds; five global failures are free). The limiter uses the connection
  peer by default. A trusted proxy can supply the client IP via
  [`X-Forwarded-For`](../configuration.md#incoming-reverse-proxy). Password
  changes have separate per-account back-off. The client displays the retry
  countdown.
- Viewers never receive configuration, secrets, integration URL paths, container
  environment values, process commands or probe diagnostics. Authentication
  events are administrative data. Streams recheck session validity and grants.
- Command widgets remain file-only. Config secret references stay destination
  bound; integration credentials are write-only and origin-bound in SQLite.
- Provider requests never follow redirects. Hobnail checks resolved
  addresses and connects to those same addresses. A hostname allow-list
  delegates trust in matching hostnames' DNS answers to the operator.
  Public provider destinations require a separate Settings opt-in.
- Provider HTTPS uses public roots by default. A per-integration pin
  verifies the exact leaf certificate and still requires a valid TLS
  handshake signature. Compare the presented SHA-256 fingerprint with
  the service certificate before confirming it. See [Integrations](integrations.md).

**Recovery and upgrades**

`hobnail set-password [username]` is optional recovery on the host (TTY without
echo, or stdin), not first-run setup. It recovers the owner and revokes sessions.
A database migrated from the legacy shared-password model is locked against
browser ownership claims: the old password is discarded and host recovery is
required. Keep a pre-upgrade database copy; migrations only move forward.

Existing 19 MiB Argon2id hashes keep their original work factor until a
successful sign-in rehashes each password at 7 MiB. Even an unsuccessful
attempt against an old hash uses the original work factor. In M5 the first
successful sign-in peaked at about 20.5 MiB.

If the server's cgroup v2 memory limit (`/sys/fs/cgroup/memory.max`) is below
36 MiB, Hobnail does not verify legacy hashes at all. Sign-in to such an
account fails exactly like a wrong password, with the same response delay,
and the server logs a one-line warning naming `hobnail reset-password`
(without the username or password). The 36 MiB threshold is the 12.45 MiB
M5 workload peak plus the 19 MiB hash, with headroom. Without a limit, or
with `max`, legacy sign-ins work and migrate as before. Disabled accounts
are always checked against a current-profile dummy record, so a disabled
legacy account never migrates by itself: run `hobnail reset-password
<username>` for it too.

Finish the migration before enforcing a hard 20 MiB container limit. Either
have every account sign in once while the container has no limit, or reset
the remaining passwords with `hobnail reset-password <username>`. The reset
preserves the existing account and revokes its sessions. It reads the new
password from the TTY without echo or from stdin, just like owner recovery.

`reset-password` and `set-password` hash the new password and open the
database themselves. `docker exec` runs them inside the server's own cgroup,
so they share its memory limit and can be OOM-killed with it. Run them with
`docker exec` only while the container has no hard limit. Once a limit is in
place, stop the server and run the command in a one-off container on the same
data volume, without a memory limit, then start the server again:

```sh
docker stop hobnail
docker run --rm -it --user 1000:1000 -v /opt/stacks/hobnail/data:/data \
  hobnail:latest reset-password <username>
docker start hobnail
```

Use your stack folder's `data` directory for the volume. Do not use
`docker compose run`: it applies the service's own memory limit.

Run `hobnail list-legacy-hashes` against the same data directory or volume.
It prints one username per line and exits successfully with no output when
migration is complete. The Owner-only `GET /api/v1/users` response also marks
each account with `legacy_hash`. The shipped Compose and deploy
configurations set no memory limit.

While legacy hashes remain, or when the server cannot tell whether any
remain, every sign-in that reaches the credential queue (wrong password,
unknown or disabled account, back-off refusal after queueing, and success)
is answered no earlier than a floor after it obtained the single credential
permit, and the permit is held until then. A queued request therefore waits
for the previous holder's deadline rather than its hash, and a client that
disconnects does not release the permit early.

The floor is an estimate of a legacy verification: twice the slowest
current-profile verification of the last two and a half to five minutes,
plus 50 ms, at least 150 ms and at most two seconds. A legacy verify costs
1.09 times a current one by memory and passes, and 1.55 times in M5.
Only current-profile verifications are measured, including the dummy
record checked for unknown and disabled accounts. Legacy verifications are
never measured, so the floor does not move in a way that depends on which
account was tried. Requests refused before hashing record nothing. When
nothing recent was measured, the latest sample holds, so the floor does not
fall below the last measured hash speed. A slow outlier ages out within five
minutes. An operation that still runs past its floor is answered two
seconds after it obtained the permit, so the size of an overrun is not
measurable.

While legacy hashes remain, the server verifies one current-profile dummy
hash at start (under the credential permit) so the floor is measured
before the first sign-in. This raised the boot peak from about 4.2 MiB to
about 8.8 MiB in M5 measurements, well under a 20 MiB limit.

In a loopback test of the pre-fix release, 30 failed attempts per profile
had medians of 68.85 ms for legacy hashes, 44.52 ms for current hashes and
51.44 ms for unknown accounts. Their maxima were 82.52, 67.69 and 106.20 ms,
respectively. Residual risk: a legacy verification slower than the
estimate (for example right after the host's load rises, before a current
verification has measured it) answers at the two-second overrun time,
which differs from the usual answer time. Past two seconds no padding is
applied. On hardware where a current verification takes about 975 ms or
more, the floor sits at two seconds and sign-ins are limited to about one
every two seconds while legacy hashes remain.

Cold SQLite reads can also raise the container's reported memory usage. The
kernel charges file-cache pages to the container that brings them into cache
and can reclaim them under memory pressure. The capped M5 cold run completed
startup, authentication with the new hash profile, and 40 full-history reads
at 20 MiB with no OOM kills. See the
[M5 memory results](../plans/react-port-phase-2-5.md#finish-hobnail-m5-server-memory).

## Guided setup

After browser owner creation, setup opens Appearance → Discover services →
Select what to include → Configure connections → Review dashboard.
Use Back, Continue or Skip to move through the optional steps. The draft lives
in SQLite, so signing in as the same owner in another browser resumes the
saved step and choices. The selected theme previews during setup and applies
as the dashboard default on creation.

Discovery suggests Docker services, bookmarks, widgets and saved integrations.
When Docker is unavailable, use the remaining suggestions or add links and HTTP
integrations manually. Include, exclude, rename and group the items before
reviewing them. Connection tests run from Hobnail. You can continue after a
failed test. Save a credential explicitly to include it with the connection.
Draft responses show the origin of each saved credential.

Creating the board saves the reviewed selection once, even if the request is
retried. The resulting Home board opens immediately. Its Add and Discover
actions and first-use tour are described in [Boards](boards.md).
