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
- Passwords are salted argon2id hashes in SQLite: 19,456 KiB memory, two passes,
  one lane. Each hash temporarily allocates about 19 MiB. A shared argon2 lock
  bounds concurrency; a simultaneous credential request can return 429.
- Sessions use 32 random bytes; only their SHA-256 is stored. The cookie is
  `HttpOnly; SameSite=Strict; Path=/; Max-Age=604800`. Database expiry and cookie
  slide on authenticated responses. The cookie has no `Secure` flag with the
  plain-HTTP default. Use an encrypted access path for credentials.
- Failed logins use per-peer-IP and global exponential back-off (1, 2, 4 …
  60 seconds; five global failures are free). The limiter uses the connection
  peer, so users behind one proxy share its IP. Password changes have separate
  per-account back-off. The client displays the retry countdown.
- Viewers never receive configuration, secrets, integration URL paths, container
  environment values, process commands or probe diagnostics. Authentication
  events are administrative data. Streams recheck session validity and grants.
- Command widgets remain file-only. Config secret references stay destination
  bound; integration credentials are write-only and origin-bound in SQLite.

**Recovery and upgrades**

`hobnail set-password [username]` is optional recovery on the host (TTY without
echo, or stdin), not first-run setup. It recovers the owner and revokes sessions.
A database migrated from the legacy shared-password model is locked against
browser ownership claims: the old password is discarded and host recovery is
required. Keep a pre-upgrade database copy; migrations only move forward.
