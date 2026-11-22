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
