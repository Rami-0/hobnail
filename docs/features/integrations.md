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
another board. Widget settings are shared wherever that instance's widget is
placed. To use different credentials, URLs or widget options, create another
instance. Remove a widget from a board in its board editor. Deleting an
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

Hobnail stores a bearer credential in SQLite alongside the exact origin
(scheme, hostname and port). Same-origin path changes keep the binding.
For an origin change, enter a replacement or explicitly clear the existing
credential. Forms indicate when a credential is saved and always leave the
password input blank on reopen. API responses provide status and binding,
never the credential. Protect the data volume and backups, which hold
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
Multiple viewers share the same fetch and cached output. Closing the last
subscription stops polling and cancels its in-flight fetch. Hobnail allows four
provider requests at once and five seconds per request, with a 128 KiB response
cap. Widget refresh defaults to 60 seconds and can range from 15 seconds to
one hour. A failed refresh keeps the last good data with its stale timestamp
and error. Output shares the 32 KiB per-widget and 256 KiB aggregate cache with
file-configured widgets. Browser background tabs release their subscriptions
after the normal live-stream delay.

## How to add a provider

Use [Generic HTTP](../../src/providers/generic.rs) as the reference. In a new
module under `src/providers/`, declare one static `Provider` with its metadata,
connection fields, test function and widget descriptors. Add it to `REGISTRY`
in `src/providers/mod.rs`. Document the actual destinations in `contacts` and
set `public_internet` if the provider intrinsically requires a public service.

Use the shared field kinds for connection and widget options. The base URL,
enabled flag and optional bearer credential use the existing connection
storage. Keep sensitive values in the write-only credential field and keep
widget options free of secrets. Use the bound client through `Target` for
outbound calls and return a typed `Fault` with actionable wording. Never put
upstream response text or credentials in errors or logs. A widget returns a
`StructuredBody` with display data. Choose among a stat, stat grid, status list,
item list, meter, table or unavailable message. The React client renders these
shapes from the schema and registry, so a provider needs no React code.
Exercise the test and widget fetches against a local fake service.

See [Boards](boards.md), [Login and security](login.md) and [API](../api.md).
