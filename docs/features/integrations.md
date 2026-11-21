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

Generic HTTP reads a JSON endpoint. In **JSON values**, enter up to 16 JSON
paths, one per line. Paths can address nested fields (`stats.users`) and array
elements (`items[0].title`). Use `$` for the entire document or leave the paths
blank for a short JSON preview. This provider is intended for arbitrary JSON
endpoints, where field names vary by service.

Existing HTTP integrations keep their IDs, configuration and credentials
through the upgrade. Existing integration links and TOML widgets retain their
layout and settings. Open a saved connection to add a JSON values widget.

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

Declare any number of secret fields alongside the URL, enabled flag and
provider options. Settings and Guided Setup render the same schema. Keep
sensitive values out of ordinary options. The Test and Fetch functions receive
a `Conn`; Fetch also receives the effective widget options, with the refresh
interval handled by the scheduler.

Use `Conn::request(Step)` for each HTTP step. Use the empty path for the exact
configured endpoint, or append a provider-relative path to the configured
prefix. Pass query parameters separately. Resolve stored credentials by
schema field ID for Bearer, Basic, custom headers, a custom Authorization
prefix, query parameters, or JSON and form bodies. Request only the response
headers you need. Handle status codes and parse bounded response data locally.
Use `Conn::session` or `Conn::session_header` to turn session material into an
opaque value and `retry_session` for a single 401, 403 or 409 retry. A 409 can
carry a replacement session in `Fault::session_conflict`.

Return a typed `Fault`. Keep raw responses, credentials and sessions out of
errors and logs. A widget returns a `StructuredBody` with display data. Choose
among a stat, stat grid, status list, item list, meter, table or unavailable
message. The React client renders these shapes from the schema and registry.
Exercise connection tests and fetches against a local fake service.

See [Boards](boards.md), [Login and security](login.md) and [API](../api.md).
