# Integrations

Settings → Integrations manages generic HTTP connection entities with stable
IDs, a name, icon, base URL and enabled state in SQLite. Multiple instances are
allowed (at most 128). No service-specific providers, provider catalog or
provider widgets are built yet. An integration tile does not imply live health.

An optional bearer token entered in the browser is write-only. The database
stores it together with its normalized origin (scheme, hostname and port).
Only that origin receives it; changing the origin requires entering it again,
or clearing the old token first. Paths are not part of the binding. Secret
values never return in responses or drafts. Admins see configuration and whether
a token is set; Viewers see identity/origin, without URL paths or credential
metadata. Tokens are plaintext on the data volume: protect database backups.

Test connection sends a bounded backend request to the saved URL with the
bound credential. Disabled integrations cannot be tested. The verdict contains
latency, a class and a short message, never the upstream body or token. Classes:
success, auth (401/403), http_status, tls, timeout, dns, connect, too_large,
blocked, protocol. Any redirect stops the test and returns a non-success HTTP
status verdict; credentials are never forwarded along a redirect. A user may
make three tests in a rolling ten-second window.

`[http] allow_hosts` applies to hostname resolution. With an empty allow-list,
private LAN destinations are allowed, but loopback, link-local/metadata,
multicast, reserved IPv4 and selected translation/metadata IPv6 addresses are
blocked for integration tests. All resolved addresses are checked; connection
uses those same addresses. An explicit allow-list permits matching hosts and
is the host operator's override for these address restrictions. URLs must be
absolute HTTP(S), without user information, query or fragment. HTTP tokens
travel without TLS; the editor warns about this.

TLS uses public certificate roots only (D21). Self-signed roots, custom CA
uploads and fingerprint pinning are not built yet. Service icons use the
existing opt-in `[icons] remote` dashboard-icons cache (D20).

Only Admin/Owner can create or test entities; scoped Admins may update/test
only granted entities and cannot create/delete them. The Owner manages grants.
See [boards.md](boards.md), [login.md](login.md) and [api.md](../api.md).
