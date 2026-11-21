# Integrations

Open **Settings → Integrations → Add integration**, or choose **Add
integration** on a board. Search the catalog by service, category or
description. The catalog lists registered providers with working connection
tests and widgets. With `[icons] remote` enabled, Hobnail uses its existing
dashboard-icons cache. Otherwise the catalog shows monograms.

Choose a provider, give the instance a name, and enter its connection fields.
Use **Test connection** to test from the Hobnail server. The test reports
problems with DNS, connectivity, TLS, credentials, timeout or the response
format. You can correct inputs and retry in the same dialog or save the
connection to test later. A saved link or untested connection has no health
evidence.

After a successful test, choose a board and review the recommended widgets.
Each selected widget has its own options and refresh interval. Use **Edit** on
a saved integration to test changes, configure its widgets, or place them on
another board. Each placement stores its own widget options and refresh
interval. Placements share the saved connection. Open boards with identical
fetch options share one cached result, even when their refresh intervals
differ. Create a separate integration for different URLs or credentials.
Remove a widget from a board in its board editor. Deleting an
integration deletes its credentials and widgets and leaves missing targets in
existing layouts.

## Generic HTTP

Generic HTTP connection tests and **HTTP status** widgets check the HTTP status.
Any 2xx response succeeds, including 204 and plain-text 200. **JSON values**
requires a JSON response. Enter up to 16 JSON
paths, one per line. Paths can address nested fields (`stats.users`) and array
elements (`items[0].title`). With no saved credential, use `$` for the entire
document or leave the paths blank for a short preview. Connections with a
saved credential require specific paths. A previously configured preview
stays unavailable until you choose specific paths or clear the credential.
Hobnail redacts credentials and session values from provider output before
caching or streaming it.

Existing HTTP integrations keep their IDs, configuration and credentials
through the upgrade. Existing integration links and TOML widgets retain their
layout and settings. Open a saved connection to add a JSON values widget.

## Pi-hole

Enter the service root URL with any reverse-proxy prefix. Leave off the
`/admin` and `/api` suffixes. Select the installed API version:

- **v6** uses an application password or the web password. Prefer an
  application password when the web login uses two-factor authentication.
  Leave the credential blank if Pi-hole has authentication disabled.
  Hobnail obtains a session with `POST /api/auth` and sends its SID in
  `X-FTL-SID`. Saved integrations reuse the session and renew it once if
  rejected. Draft tests attempt to close their temporary session.
- **v5** uses the API token from Pi-hole when authentication is enabled.
  Hobnail calls `GET /admin/api.php` with `summaryRaw` and, when set, the
  encoded `auth` query parameter. The token can appear in Pi-hole or proxy
  access logs that record query strings. Prefer v6, which sends its session
  credential in a header.

The **DNS activity** widget shows queries, blocked queries, percentage
blocked and blocking status. The v6 widget reads `/api/stats/summary`
and `/api/dns/blocking`. The v5 widget reads all four values from the
legacy summary.

Verified against the public [authentication documentation](https://docs.pi-hole.net/api/auth/),
the upstream [v6 summary](https://github.com/pi-hole/FTL/blob/master/src/api/docs/content/specs/stats.yaml)
and [blocking](https://github.com/pi-hole/FTL/blob/master/src/api/docs/content/specs/dns.yaml)
schema files, and the tagged [v5.21 summary](https://github.com/pi-hole/web/blob/v5.21/api_FTL.php)
and [token handling](https://github.com/pi-hole/web/blob/v5.21/scripts/pi-hole/php/password.php)
source. The v6 summary schema gives totals without specifying a calendar-day
window, so the widget labels that count **Queries**. Request flow and response
parsing were exercised against fakes. Compatibility across older minor
releases and live reverse-proxy configurations remains unverified.

## Credentials and transport

Hobnail stores credentials in SQLite by integration and schema field, each
with its exact origin (scheme, hostname and port). Same-origin path changes
keep the bindings. On an origin change, replace or clear each saved field.
Forms show the saved origin and reopen with blank password inputs. API
responses expose each field's presence and binding. An omitted credential
field keeps its value, a string replaces it, and null clears it. Protect the data volume and backups, which hold
credentials in plaintext. HTTP sends credentials unencrypted.

By default HTTPS verifies the peer against public certificate roots and checks
the hostname. To use a self-signed service, compare the fingerprint presented
by the failed test with a SHA-256 fingerprint obtained directly from the
service. Confirm it in the dialog, test again, and save. You can also enter a
known fingerprint manually. When pinned, the exact leaf certificate must
match and the peer must prove possession of its private key. A new certificate
will require a new pin. Pins are stored per integration and the editor clears
the draft pin when the URL's origin changes. Only confirm a fingerprint you
can verify independently.

Provider requests stop at redirects. Hobnail resolves the hostname once,
checks the resulting addresses and connects using those same addresses.
Private LAN destinations pass the default address policy. Loopback, link-local,
metadata and other reserved addresses are blocked unless explicitly permitted
by the server operator. With a nonempty `[http] allow_hosts`, only matching
hosts can be used. The operator is trusting those hosts' DNS answers for the
address-policy exemption. See [Outgoing HTTP](../configuration.md#outgoing-http).

## Internet access and polling

**Settings → Integrations → Provider internet access** starts off. The section
states what each provider contacts. A provider declared as public is disabled
while the switch is off. Generic HTTP accepts LAN destinations by default and
checks every resolved address before an HTTP connection. Public destinations
require the switch even if the hostname is in `allow_hosts`. Hostname
resolution still uses the server's DNS. The switch is for catalog providers,
independent of file-configured widgets or dashboard-icons.

Provider widgets require access to both the board and the integration. A board
viewer without the integration grant sees a neutral **Widget unavailable**
placeholder. That viewer's live stream does not receive widget data or start
polling. Removing the integration grant also removes access on open streams.
An admin can add a provider widget only when granted access to its integration
and the board.

Provider widgets refresh only while a live board subscription includes them.
Multiple viewers share a fetch with the same effective inputs. Closing the
last subscription stops polling and cancels its in-flight fetch. Hobnail
allows four provider operations at once. Each operation has five seconds
total, including logins, DNS, TLS and retries. Each step caps the response
body at 128 KiB and the request body at 16 KiB. Changing the outbound
policy, service URL, credentials or TLS pin cancels affected operations
and closes their upstream sockets. Widget refresh defaults to 60 seconds and can range from 15 seconds to
one hour. A failed refresh keeps the last good data with its stale timestamp
and error. Output shares the 32 KiB per-widget and 256 KiB aggregate cache with
file-configured widgets, with up to 128 provider fetch slots. Browser background
tabs release their subscriptions after the normal live-stream delay.

Providers may cache up to 16 sessions, each at most 1 KiB (16 KiB of session
material plus keys and metadata). This budget keeps process-local session
reuse small within the 20 MiB application budget. A session belongs to
one integration, canonical origin and revision. Credential and pin changes
rotate that revision. Edits, deletion and changes to provider internet
access evict affected sessions. A rejected session can be renewed and retried
once within the operation deadline.

## How to add a provider

Use [Generic HTTP](../../src/providers/generic.rs) as the reference. In a new
module under `src/providers/`, declare one static `Provider` with its metadata,
connection fields, test function and widget descriptors. Add it to `REGISTRY`
in `src/providers/mod.rs`. Document the actual destinations in `contacts` and
set `public_internet` if the provider intrinsically requires a public service.

Declare write-only secret fields alongside the URL, enabled flag and provider
options. Settings and Guided Setup render the same schema. Keep sensitive
values out of ordinary options. Test and Fetch receive a `Conn` for the whole
operation. Fetch also receives the effective widget options, with the refresh
interval handled by the scheduler.

Use `Conn::request(Step)` for HTTP. An empty path uses the exact saved endpoint.
A provider-relative path appends to the saved prefix. Supply query parameters
separately and select only the response headers you need. Use
`TargetHeader::Origin` and `TargetHeader::Referer` in `Step::target_headers`
for CSRF checks that require the saved origin or URL.

| Pattern | Recipe |
| --- | --- |
| API-key header | Pass `Auth::Header("x-api-key", Input::Credential("api_key"))` to the step. |
| Bearer | Pass `Auth::Bearer(Input::Credential("token"))`. Use `Auth::Authorization` for a service-specific authorization prefix. |
| Basic | Pass the username and password fields as separate `Input::Credential` values to `Auth::Basic`. |
| Query key | Add the field to `Step::query` as `Input::Credential`. The upstream and its proxies may record the query in their access logs. |
| Form login and cookie | POST a `Body::Form` containing the credential fields and select `set-cookie` in `response_headers`. After a successful login, pass the expected cookie name to `Conn::session_cookie`. Use `Auth::Cookie(&session)` on later steps and add typed CSRF headers when required. |
| 409 session challenge | Call `retry_session(|| async { Ok(None) }, request)`. Send the real RPC with the optional cached session. Select the challenge header and return `Fault::session_conflict(conn.session_header(&response, header)?)` on 409. The helper stores the challenge and retries the RPC once. |
| Token exchange | Send the login step using `Body::Json` or `Body::Form`, handle errors, and turn the response token into an opaque session with `Conn::session`. Return `Some(session)` from the login closure in `retry_session`, then use `Input::Session` with the service's auth header or Bearer scheme. |
| Credential-free probe | Call `dns_lookup`, `tcp_probe`, `tls_leaf`, or `http_status` for the saved target. Each uses the same destination policy, request permit, deadline, and revision as HTTP steps. |

The system resolver exposes up to 64 A/AAAA addresses, including results from
the local hosts database. Other DNS record types require a dedicated resolver.
TCP connect reports reachability and connect time. Use it in place of ICMP
ping, which is outside this contract because it can require extra privileges.
The HTTP status probe gets the status and latency without waiting for the
response body or following redirects. The TLS probe performs only a TLS
handshake. It verifies the handshake signatures and reports the leaf subject,
issuer, validity in Unix seconds, SANs, SHA-256 fingerprint, and certificate
verification verdict even when normal trust or pin verification rejects the
leaf. A certificate inspection does not relax verification on HTTP requests.

Return a typed `Fault`. Keep raw responses, credentials and sessions out of
errors and logs. A widget returns a `StructuredBody` with display data. Choose
among a stat, stat grid, status list, item list, meter, table or unavailable
message. The React client renders these shapes from the schema and registry.
The shared output boundary redacts credentials and known sessions, including
textual escapes and standard or URL-safe Base64. Redact JSON before truncating
or formatting it so an encoded credential cannot survive as a partial display
value. The redactor checks up to 128 KiB per string, 32 distinct secret values
up to 16 KiB apiece, and three layers of percent, JSON and common HTML escapes.
It withholds a string or link that exceeds the applicable bounds.
Exercise connection tests and fetches against a local fake service.

See [Boards](boards.md), [Login and security](login.md) and [API](../api.md).
