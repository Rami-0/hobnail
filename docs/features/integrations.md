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

## Uptime Kuma

Enter the service URL and the slug from a published status page, such as
`home` from `/status/home`. Use letters, digits and hyphens for the slug.
The status page grants read access without credentials.

**Monitor status** shows up, down, pending, maintenance or unknown based on
the latest heartbeat, along with the upstream 24-hour uptime percentage. It
shows up to 24 monitors and the current pinned incident when present. Kuma's
public JSON routes supply these together. Its documented Prometheus metrics
expose monitor state and response time, but do not include the status page's
uptime or pinned incident.

Verified from:

- [Kuma 1.23.16 status-page router](https://github.com/louislam/uptime-kuma/blob/1.23.16/server/routers/status-page-router.js)
- [Kuma 2.0.2 status-page router](https://github.com/louislam/uptime-kuma/blob/2.0.2/server/routers/status-page-router.js)
- [Status page model](https://github.com/louislam/uptime-kuma/blob/2.0.2/server/model/status_page.js), [heartbeat model](https://github.com/louislam/uptime-kuma/blob/2.0.2/server/model/heartbeat.js) and [incident model](https://github.com/louislam/uptime-kuma/blob/2.0.2/server/model/incident.js)
- [Prometheus setup](https://github.com/louislam/uptime-kuma/wiki/Prometheus-Integration) and [exported metrics](https://github.com/louislam/uptime-kuma/blob/2.0.2/server/prometheus.js)

UNVERIFIED: Compatibility outside the inspected releases and live proxies.

## Jellyfin

Enter the service URL and a Jellyfin Dashboard API key. Create a dedicated
key in Dashboard → API Keys. Jellyfin grants application keys server-wide
access without per-route scopes. Hobnail uses read endpoints only and sends
the key in the `Authorization` header using the MediaBrowser scheme.

**Active streams** counts sessions with a current media item, including
paused playback, and lists up to ten titles and clients. The unpaged
`/Sessions` response can exceed the size limit, leaving active streams
unavailable while other widgets can still refresh. **Library counts**
shows server-wide movies, series, episodes, albums, songs and books.
**Latest added** shows up to ten recently created library entries, newest
first. Its server-wide `/Items` query works with an application key alone.
Jellyfin's user-specific `/Items/Latest` needs a user ID.

Verified from:

- [Jellyfin 10.11.10 OpenAPI](https://api.jellyfin.org/openapi/stable/jellyfin-openapi-10.11.10.json)
- [Authorization parser and API key lookup](https://github.com/jellyfin/jellyfin/blob/v10.11.10/Jellyfin.Server.Implementations/Security/AuthorizationContext.cs)
- [Sessions](https://github.com/jellyfin/jellyfin/blob/v10.11.10/Jellyfin.Api/Controllers/SessionController.cs), [server-wide item queries](https://github.com/jellyfin/jellyfin/blob/v10.11.10/Jellyfin.Api/Controllers/ItemsController.cs) and [user-specific latest media](https://github.com/jellyfin/jellyfin/blob/v10.11.10/Jellyfin.Api/Controllers/UserLibraryController.cs)

UNVERIFIED: Compatibility outside the inspected release and live proxies.

## qBittorrent

Enter the WebUI URL, username and password. qBittorrent exposes its WebUI
account without separate read-only scopes. Hobnail only logs in and reads
transfer data. Keep the session cookie name at `SID` for a default 5.0.5
WebUI. If your WebUI uses a custom or port-qualified name, enter the exact
configured name, such as `QBT_SID_8080`. Keep WebUI CSRF protection enabled.

**Torrent activity** shows download and upload rates, the WebUI's active
and seeding counts, queued uploads and downloads, and free space on the
default save path. A torrent can appear in more than one count.
Active and seeding counts use separate filtered first pages and show
Unavailable when the page fills (32 entries) or reaches the response cap.
Queued and free-space figures come from the initial sync snapshot and show
Unavailable if it exceeds 128 KiB. Transfer speeds remain visible.

Verified from:

- [WebUI API v2 documentation for qBittorrent 5.0](https://github.com/qbittorrent/qBittorrent/wiki/WebUI-API-(qBittorrent-5.0))
- [5.0.5 login result](https://github.com/qbittorrent/qBittorrent/blob/release-5.0.5/src/webui/api/authcontroller.cpp) and [session cookies, CSRF and expired sessions](https://github.com/qbittorrent/qBittorrent/blob/release-5.0.5/src/webui/webapplication.cpp)
- [Sync snapshot and free space](https://github.com/qbittorrent/qBittorrent/blob/release-5.0.5/src/webui/api/synccontroller.cpp)

UNVERIFIED: Compatibility outside the inspected version and live proxies.

## Sonarr

Enter the service URL and the API key from Settings → General. Sonarr's
instance API key is required and has no read-only scope. Hobnail sends it
in `X-Api-Key` and uses only Sonarr API v3 GET routes.

**Sonarr overview** shows the download queue total, including pending and
unknown items, the monitored missing-episode total and current health issues.
**Upcoming episodes** lists up to ten monitored airings in the next 48 hours.

Verified from:

- [Sonarr 4.0.17.2952 API v3 OpenAPI](https://github.com/Sonarr/Sonarr/blob/v4.0.17.2952/src/Sonarr.Api.V3/openapi.json)
- [Queue status](https://github.com/Sonarr/Sonarr/blob/v4.0.17.2952/src/Sonarr.Api.V3/Queue/QueueStatusController.cs), [missing episodes](https://github.com/Sonarr/Sonarr/blob/v4.0.17.2952/src/Sonarr.Api.V3/Wanted/MissingController.cs) and [calendar](https://github.com/Sonarr/Sonarr/blob/v4.0.17.2952/src/Sonarr.Api.V3/Calendar/CalendarController.cs)

UNVERIFIED: Compatibility outside the inspected release and live proxies.

## Radarr

Enter the service URL and the API key from Settings → General. Radarr's
instance API key is required and has no read-only scope. Hobnail sends it
in `X-Api-Key` and uses only Radarr API v3 GET routes.

**Radarr overview** shows the download queue total, including pending and
unknown items, the monitored missing-movie total and current health issues.
**Upcoming movies** lists up to ten monitored movies with a theatrical,
digital or physical release in the next 48 hours, in Radarr's calendar order.

Verified from:

- [Radarr API v3 OpenAPI](https://github.com/Radarr/Radarr/blob/25a2124d7d5bb10aae6b89726577303f4bd60f75/src/Radarr.Api.V3/openapi.json)
- [Queue status](https://github.com/Radarr/Radarr/blob/25a2124d7d5bb10aae6b89726577303f4bd60f75/src/Radarr.Api.V3/Queue/QueueStatusController.cs), [missing movies](https://github.com/Radarr/Radarr/blob/25a2124d7d5bb10aae6b89726577303f4bd60f75/src/Radarr.Api.V3/Wanted/MissingController.cs), [calendar](https://github.com/Radarr/Radarr/blob/25a2124d7d5bb10aae6b89726577303f4bd60f75/src/Radarr.Api.V3/Calendar/CalendarController.cs) and [release-window query](https://github.com/Radarr/Radarr/blob/25a2124d7d5bb10aae6b89726577303f4bd60f75/src/NzbDrone.Core/Movies/MovieRepository.cs)

UNVERIFIED: Compatibility outside the inspected upstream revision and live proxies.

## Prowlarr

Enter the service URL and the API key from Settings → General. Prowlarr's
instance API key is required and has no read-only scope. Hobnail sends it
in `X-Api-Key` and uses only Prowlarr API v1 GET routes.

**Indexer activity** shows the number of temporarily blocked indexers,
details for up to five of them, and total and failed grabs from the last
seven days. The status endpoint reports temporary blocks due to upstream
failures. The total grab count includes failed grabs.

Verified from:

- [Prowlarr API v1 OpenAPI](https://github.com/Prowlarr/Prowlarr/blob/46c468c443ff3ebb86fc99347d3a15885d41d1e1/src/Prowlarr.Api.V1/openapi.json)
- [Indexer status](https://github.com/Prowlarr/Prowlarr/blob/46c468c443ff3ebb86fc99347d3a15885d41d1e1/src/Prowlarr.Api.V1/Indexers/IndexerStatusController.cs) and [blocked-provider filter](https://github.com/Prowlarr/Prowlarr/blob/46c468c443ff3ebb86fc99347d3a15885d41d1e1/src/NzbDrone.Core/ThingiProvider/Status/ProviderStatusServiceBase.cs)
- [Indexer stats](https://github.com/Prowlarr/Prowlarr/blob/46c468c443ff3ebb86fc99347d3a15885d41d1e1/src/Prowlarr.Api.V1/Indexers/IndexerStatsController.cs) and [grab aggregation](https://github.com/Prowlarr/Prowlarr/blob/46c468c443ff3ebb86fc99347d3a15885d41d1e1/src/NzbDrone.Core/IndexerStats/IndexerStatisticsService.cs)

UNVERIFIED: Compatibility outside the inspected upstream revision and live proxies.

## Jellyseerr / Overseerr

Enter the Jellyseerr or Overseerr service URL and the instance API key
from Settings → General. Both use API v1 and accept the key in
`X-Api-Key`. The shared key acts as the original administrator and has
no read-only scope. Hobnail uses only the request-count endpoint.

**Media requests** shows the instance totals for pending and approved
requests. Approved includes requests still being processed and those
already available.

Verified from:

- [Jellyseerr 2.7.3 OpenAPI](https://github.com/Fallenbagel/jellyseerr/blob/v2.7.3/jellyseerr-api.yml), [request counts](https://github.com/Fallenbagel/jellyseerr/blob/v2.7.3/server/routes/request.ts) and [key authentication](https://github.com/Fallenbagel/jellyseerr/blob/v2.7.3/server/middleware/auth.ts)
- [Overseerr 1.35.0 OpenAPI](https://github.com/sct/overseerr/blob/v1.35.0/overseerr-api.yml), [request counts](https://github.com/sct/overseerr/blob/v1.35.0/server/routes/request.ts) and [key authentication](https://github.com/sct/overseerr/blob/v1.35.0/server/middleware/auth.ts)

UNVERIFIED: Compatibility outside the inspected releases and live proxies.

## AdGuard Home

Enter the Web UI URL and an AdGuard Home account username and password.
The control API uses HTTP Basic authentication and has no read-only account
scope. **DNS protection** reads the configured statistics interval and
reports filtering-rule blocks, their share of all queries, and the current
protection setting. With zero queries the percentage is undefined.

Verified from:

- [Upstream OpenAPI for v0.107.73](https://github.com/AdguardTeam/AdGuardHome/blob/v0.107.73/openapi/openapi.yaml)

UNVERIFIED: Other versions and live reverse proxies.

## Technitium DNS

Enter the web console URL and an API token created for a user with
Dashboard View, Zones View and View on each zone you want counted. Hobnail sends the token in the form
body of each read request.

The connection test verifies zone list access. Dashboard View permission
is checked when the widget refreshes.

**DNS activity** shows queries and blocks from the last hour and the total
number of zones visible to that user. Hobnail requests one zone per page
and reads only the total. Verified against Technitium DNS Server v14.3.0.

Verified from:

- [Upstream v14.3.0 API reference](https://github.com/TechnitiumSoftware/DnsServer/blob/v14.3.0/APIDOCS.md)

UNVERIFIED: Older pagination contracts, cluster aggregation and live reverse proxies.

## Tailscale

Enable Provider internet access in Settings → Integrations. Leave the API
URL at https://api.tailscale.com and enter your tailnet (or `-` for the
token’s tailnet) and an API access token. A device-read Bearer token with
`devices:core:read` is sufficient. Replace the token when it expires.
Hobnail sends it only to the origin it was saved for.

**Tailnet devices** counts devices connected to Tailscale’s control service
and shows last seen for up to ten devices. **Expiring device keys** lists
up to ten device keys expiring in the next 30 days, earliest first. The
widgets read the API v2 device list and refresh at intervals of ten minutes
or longer.

Verified from:

- [Upstream authentication reference](https://tailscale.com/docs/reference/tailscale-api)
- [Upstream device-read scope](https://tailscale.com/docs/reference/trust-credentials)
- [Upstream device list client](https://github.com/tailscale/tailscale-client-go-v2/blob/f9e92a86ee85097dbabd255384403cb32b94f7c0/devices.go)

UNVERIFIED: OAuth token renewal, live responses and large tailnets. Use the default `-` for account aliases containing `@`.

## WireGuard (wg-easy)

Enter the wg-easy Web UI URL and password. For v15 also enter the username
of the account whose peers you want to see. A regular v15 user can see
their assigned peers. An administrator can see all peers. v14 exposes the
shared Web UI password and has no narrower scope.

**WireGuard peers** shows the accessible peer count and up to ten peers
with their latest handshake and received and sent bytes. Hobnail exchanges
credentials for a session cookie. It uses the v14 password-only login with
`connect.sid`, or the v15 username/password login with `wg-easy`.

Verified from:

- [Upstream v14 session routes](https://github.com/wg-easy/wg-easy/blob/v14/src/lib/Server.js)
- [Upstream v14 peer metrics](https://github.com/wg-easy/wg-easy/blob/v14/src/lib/WireGuard.js)
- [Upstream v15.1.0 session and client routes](https://github.com/wg-easy/wg-easy/tree/v15.1.0/src/server/api)
- [Upstream v15.1.0 cookie and peer metrics](https://github.com/wg-easy/wg-easy/tree/v15.1.0/src/server/utils)

UNVERIFIED: v14 without a password, v15 accounts with two-factor authentication, other releases and live instances.

## Nginx Proxy Manager

Enter the admin UI URL, account email and password. A regular user with
View on Proxy Hosts and SSL Certificates is sufficient for the records
visible to that account. Hobnail exchanges the credentials for a Bearer
token and verifies it using the account endpoint.

The test signs in and reads the account profile. Access to the proxy host
and certificate lists is checked when the widget refreshes.

**Proxy hosts and certificates** counts visible proxy hosts and certificates
and lists up to ten certificate expiration dates. The two lists have no
documented page controls in v2.12.3. An oversized list shows Unavailable
for its own data while the other list remains usable.

Verified from:

- [Upstream v2.12.3 endpoint schemas](https://github.com/NginxProxyManager/nginx-proxy-manager/tree/v2.12.3/backend/schema/paths)
- [Upstream v2.12.3 token exchange](https://github.com/NginxProxyManager/nginx-proxy-manager/blob/v2.12.3/backend/internal/token.js)
- [Upstream v2.12.3 list permissions](https://github.com/NginxProxyManager/nginx-proxy-manager/tree/v2.12.3/backend/lib/access)

UNVERIFIED: Other releases, database-specific display formats and live reverse proxies.

## Glances

Enter the Glances web server URL. If authentication is enabled, enter both
the username and password for HTTP Basic authentication. Glances does
not expose a plugin-specific account scope.

**Host resources** shows CPU and memory usage and up to five filesystems
and five sensors. Hobnail uses the REST API v4 and falls back to v3 if
the v4 CPU route returns 404. Missing or oversized optional plugin data
shows as Unavailable.

Verified from:

- [Upstream v4 REST API](https://glances.readthedocs.io/en/latest/api/restful.html)
- [Upstream v3.4.0 REST API](https://glances.readthedocs.io/en/v3.4.0/api.html)

UNVERIFIED: v5, platform-specific plugins and live reverse proxies.

## Plex

Enter your Plex Media Server URL and a Plex access token for that server.
An owner token is needed for full session visibility. Use a token limited
to the libraries you need when only reading their data. Hobnail sends
`X-Plex-Token` and requests JSON.

**Active streams** shows the session count and up to ten titles. **Library
counts** shows the number of visible libraries and up to five library item
totals using first-page count metadata. **Recently added** shows up to ten
items. An oversized response leaves independent data available and marks
the affected widget or count unavailable.

Verified from:

- [Upstream Media Server API 1.2.3 reference](https://developer.plex.tv/pms/)
- [Upstream Media Server diagnostic trace for the global recently-added route](https://forums.plex.tv/t/library-scan-not-recognizing-video-files-after-update/898898)

UNVERIFIED: Live token privilege combinations, and the JSON response shape and
pagination of the global recently-added route.

## Immich

Enter your Immich server URL and an API key from an administrator account.
Grant only `server.statistics`, `server.storage` and `job.read`. Hobnail sends
the key in the `x-api-key` header.

The connection test verifies storage access. Library statistics and job
permissions are checked when the widget refreshes.

**Immich overview** shows instance photo and video counts, used and available
disk space, and active, waiting and failed jobs across the queues. If a
response exceeds the cap, the other endpoints can still supply their data.

Verified from:

- [Upstream v2.5.6 OpenAPI schemas and permissions](https://github.com/immich-app/immich/blob/v2.5.6/open-api/immich-openapi-specs.json)

UNVERIFIED: Releases without the legacy GET /api/jobs endpoint, which v2.5.6 marks deprecated, and live reverse proxies.

## Transmission

Enter the Web UI origin, including any reverse-proxy prefix before
`/transmission/rpc`. Supply the RPC username and password if the daemon
uses HTTP Basic authentication. The RPC credential has no read-only scope.

**Torrent activity** shows global transfer speeds and the exact active
torrent count reported by `session-stats`. When transfers are running, it lists up to ten downloading,
seeding or verifying torrents from the `recently-active` RPC subset.
If that subset exceeds the cap, speeds and the total remain available.
Hobnail sends the real RPC on a cold connection and retries once with
the session ID supplied by an HTTP 409 response.

Verified from:

- [Upstream 4.0.6 RPC specification](https://github.com/transmission/transmission/blob/4.0.6/docs/rpc-spec.md)
- [Upstream 4.0.6 statistics and torrent selection implementation](https://github.com/transmission/transmission/blob/4.0.6/libtransmission/rpcimpl.cc)

UNVERIFIED: Nonstandard RPC path suffixes, releases that remove legacy RPC and live reverse proxies.

## SABnzbd

Enter the SABnzbd Web UI URL and the full API Key from Config → General.
The NZB Key lacks queue access. Hobnail uses only the queue read call and
sends the key in the form body.

**Download queue** shows the total number of queued jobs, download speed
and free space on the incomplete-download drive. It requests only one
queue entry and reads the summary.

Verified from:

- [Upstream queue API reference](https://sabnzbd.org/wiki/advanced/api)
- [Upstream v4.5.5 queue response and units](https://github.com/sabnzbd/sabnzbd/blob/4.5.5/sabnzbd/api.py)
- [Upstream v4.5.5 API key validation](https://github.com/sabnzbd/sabnzbd/blob/4.5.5/sabnzbd/interface.py)

UNVERIFIED: Releases outside v4.5.5 and live reverse proxies.

## Home Assistant

Enter the Home Assistant URL and a long-lived access token from your profile.
Use an account that can read the selected entities. The connection test checks
`GET /api/` with the token in the Bearer header.

**Entity states** takes up to 16 distinct IDs such as `sensor.temperature`,
one per line. It reads each ID separately and shows its state, any unit of
measurement and the on/off counts across those entities. Missing and oversized
entities appear as unavailable while other readings remain visible.

Verified from: [Home Assistant REST API](https://contributors.home-assistant.io/core/internals/rest-api/)
and [Core 2025.11.0 API source](https://github.com/home-assistant/core/blob/2025.11.0/homeassistant/components/api/__init__.py).

UNVERIFIED: live installations and the permissions of custom authentication providers.

## Nextcloud

Enter the Nextcloud URL, an administrator’s username and an app password.
Enable Nextcloud’s Monitoring app (`serverinfo`). Hobnail uses HTTP Basic
auth and the OCS request header on the OCS v2 serverinfo endpoint.

**Storage and activity** shows free space as reported by Nextcloud, total
users, active users in the last 5 minutes, hour and day, and total shares.
The serverinfo response has no equivalent aggregate used-space field and
provides activity counts in place of an event feed.

Verified from: [serverinfo API and example](https://github.com/nextcloud/serverinfo)
and the [Nextcloud 32 controller](https://github.com/nextcloud/serverinfo/blob/672a40b96e9a502122e6217fcd326b72b559d651/lib/Controller/ApiController.php).

UNVERIFIED: other Nextcloud generations and storage backends, quota-derived
used space and the separate Activity app.

## Syncthing

Enter the GUI URL and the API Key from Settings → GUI. The key has Syncthing
GUI API access. Hobnail sends it in `X-API-Key` and checks
`GET /rest/system/status`.

**Synchronization** counts online peer devices and checks the first ten
configured folders in ID order. A folder is locally in sync when idle with
no pending changes or errors. Paused folders are included in the checked
count. The widget reports partial coverage and keeps device counts if the
folder list is oversized, or folder status if the device list is oversized.

Verified from: [REST API](https://docs.syncthing.net/dev/rest.html),
[v2.0.11 API source](https://github.com/syncthing/syncthing/blob/v2.0.11/lib/api/api.go)
and [folder summary](https://github.com/syncthing/syncthing/blob/v2.0.11/lib/model/folder_summary.go).

UNVERIFIED: older releases and remote-device folder completion.

## Duplicati

Enter your Duplicati 2.1 server URL and its web interface password. Hobnail
sends the password in JSON to `POST /api/v1/auth/login`, uses the returned
JWT as a Bearer token, and tests `GET /api/v1/serverstate`. Duplicati web
credentials permit server administration. The provider makes read requests
after login.

**Recent backups** reads `GET /api/v1/backups` and shows the latest recorded
completion and examined source size for up to ten jobs. An oversized backup
list is shown as unavailable. A recorded completion alone cannot establish a
job's success, warning or error result.

Verified from Duplicati 2.1.0.5 [login](https://github.com/duplicati/duplicati/blob/d41bac323831c7490b27c8ae1a8028d81a524417/Duplicati/WebserverCore/Endpoints/V1/Auth.cs),
[backups](https://github.com/duplicati/duplicati/blob/d41bac323831c7490b27c8ae1a8028d81a524417/Duplicati/WebserverCore/Endpoints/V1/Backups.cs)
and [metadata writer](https://github.com/duplicati/duplicati/blob/d41bac323831c7490b27c8ae1a8028d81a524417/Duplicati/Library/RestAPI/Runner.cs).

UNVERIFIED: job result (the backup metadata has no authoritative per-run
result), legacy 2.0 authentication and deployments using a pre-shared key.

## Proxmox VE

Enter the Proxmox VE URL (typically HTTPS on port 8006), full token ID
(`USER@REALM!TOKENID`) and token secret. Grant the token `Sys.Audit` on
the monitored nodes and `VM.Audit` on the monitored guests. A propagated
`PVEAuditor` grant on `/` covers the cluster. Privilege-separated tokens
also require the underlying user to have the corresponding grants. For
self-signed TLS, check the server's SHA-256 fingerprint and confirm the
matching leaf pin in Hobnail.

**Cluster overview** checks `/api2/json/version` and uses the filtered
`/api2/json/cluster/resources` node and VM lists. It counts online nodes and
running VMs and containers visible to the token, excludes templates, and shows
CPU and RAM for up to ten nodes. Oversized lists leave the other list visible.

Verified from: [Proxmox VE token and role docs](https://pve.proxmox.com/pve-docs/chapter-pveum.html),
[version API](https://github.com/proxmox/pve-manager/blob/d147600a85186460650b5c45c453948230e6a0fb/PVE/API2.pm)
and [cluster API source](https://github.com/proxmox/pve-manager/blob/d147600a85186460650b5c45c453948230e6a0fb/PVE/API2/Cluster.pm).

UNVERIFIED: live clusters and releases with different resource schemas.

## TrueNAS SCALE

Enter the TrueNAS SCALE URL and API key. The key needs read access to
`disk.query`, `pool.query`, `alert.list` and `smart.test.results`. On
24.10, a standard UI-generated legacy API key has broad appliance access.
Hobnail uses it as a Bearer token and tests a one-entry disk query.

**Storage health** shows non-dismissed alerts, status and free space for
up to ten pools, and the latest reported SMART self-test for up to ten
disks. Missing results stay unknown. The provider reads the test log and
does not start a test. Oversized lists leave independent metrics visible.

Verified from [24.10.2 REST v2.0](https://www.truenas.com/docs/scale/24.10/api/scale_rest_api.html),
[Bearer auth source](https://github.com/truenas/middleware/blob/TS-24.10.2/src/middlewared/middlewared/restful.py),
[SMART results source](https://github.com/truenas/middleware/blob/TS-24.10.2/src/middlewared/middlewared/plugins/smart.py)
and [TrueNAS 26 API removal notes](https://www.truenas.com/docs/scale/26/gettingstarted/versionnotes/).

The REST API is deprecated in 25.04 and 25.10, and removed in TrueNAS 26.
TrueNAS 26 requires JSON-RPC over WebSocket, which the provider connection
contract does not support.

UNVERIFIED: live appliances, 25.x REST compatibility and disk health
beyond the latest SMART self-test result.

## Gitea / Forgejo

Enter the forge URL and a personal access token with `read:repository`,
`read:user` and access to the selected repos. Hobnail sends `Authorization: token …`.
The connection test checks `/api/v1/version`, then a one-item
`/api/v1/user/repos` request to verify the token.

**Repository activity** counts repositories visible to the token and shows
separate issue and PR counts for up to eight configured `owner/repository`
names. It reads the most recent Actions run on Gitea 1.26 and Forgejo 15.
If the run endpoint is absent, it reads the latest Actions task when
available (including on Gitea 1.24), and labels it as a task.

Verified from the [Gitea 1.26 API](https://github.com/go-gitea/gitea/blob/v1.26.0/templates/swagger/v1_json.tmpl),
[Gitea 1.24 Actions API](https://docs.gitea.com/api/1.24/operations/list-action-tasks/)
and [Forgejo 15 API](https://codeberg.org/forgejo/forgejo/src/tag/v15.0.0/templates/swagger/v1_json.tmpl).

UNVERIFIED: older API releases, private installations and custom token middleware.

## GitHub

GitHub contacts only `https://api.github.com` and requires the public
internet setting, which is off by default. Leave the token blank for public
repos, or use a fine-grained token with Metadata, Pull requests and Actions
read permissions for the selected repos. Hobnail uses Bearer authentication
and tests `/user` with a token or `/rate_limit` without one.

**Repository activity** shows stars, open issues and PRs, and the latest
workflow status for up to eight configured `owner/repository` names. It
bounds the pulls and runs requests to one result each. If the PR count is
unavailable, it displays GitHub's combined issue and PR count. For public
widgets, the minimum interval is ten minutes. GitHub defaults to 30 minutes.
Anonymous requests share GitHub's 60 requests per hour per IP. Multiple
widgets or shorter intervals can exhaust that budget. Rate-limited responses
retain stale data and suppress further requests until the cooldown expires.
Anonymous and authenticated cooldowns are tracked separately in memory.

Verified from GitHub's [repositories](https://docs.github.com/en/rest/repos/repos#get-a-repository),
[pull requests](https://docs.github.com/en/rest/pulls/pulls#list-pull-requests),
[pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api),
[workflow runs](https://docs.github.com/en/rest/actions/workflow-runs#list-workflow-runs-for-a-repository)
and [rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)
REST API documentation.

UNVERIFIED: live tokens, enterprise policy behavior and concurrent changes
to issue and PR counts between the two requests.

## Paperless-ngx

Enter the Paperless-ngx URL and an API token from your Paperless account.
Grant the account view permission for the documents and inbox tags you
want counted. Hobnail sends the token in `Authorization: Token …` and
checks `GET /api/`.

**Documents and inbox** reads `GET /api/statistics/` for the visible document
total and inbox count. It reports when the account has no visible inbox tag.
The provider reads counts without listing or downloading documents.

Verified from the [Paperless-ngx REST API](https://paperless-ngx-docs.readthedocs.io/en/latest/api.html),
[v2.20.15 routes](https://github.com/paperless-ngx/paperless-ngx/blob/v2.20.15/src/paperless/urls.py)
and [statistics source](https://github.com/paperless-ngx/paperless-ngx/blob/v2.20.15/src/documents/views.py).

UNVERIFIED: other API versions, reverse-proxy auth and live permission setups.

## SSL certificate expiry

Configure one TLS hostname per integration as `https://host[:port]`. The
widget runs a TLS handshake from Hobnail and reports time remaining, expiry
urgency (14 and 7 days by default), issuer, subject, DNS/IP SANs, hostname
match and the public trust or configured pin verdict. An expired or
untrusted leaf is still displayed. LAN targets use the normal outbound policy.
Public targets require the provider internet switch.

Verified from: [RFC 9525](https://www.rfc-editor.org/rfc/rfc9525.html) and
[rustls name verification](https://docs.rs/rustls/latest/rustls/client/fn.verify_server_name.html).

UNVERIFIED: Live certificate chains and TLS endpoints.

## Reachability

Choose one TCP, HTTP or DNS check per integration. TCP connect replaces ICMP
ping, which requires additional privileges on some hosts. TCP shows connect
time, HTTP checks an inclusive status range (200–299 by default), and DNS
uses the server resolver for A/AAAA addresses with an optional expected IP.
HTTP checks stop at the first response status. Admins can probe private
hosts and ports reachable from Hobnail. See [http.allow_hosts](../configuration.md)
for explicit exceptions to HTTP destination policy. Public destinations
require the provider internet switch.

Verified from: [TCP](https://www.rfc-editor.org/rfc/rfc9293.html),
[HTTP semantics](https://www.rfc-editor.org/rfc/rfc9110.html) and
[DNS AAAA](https://www.rfc-editor.org/rfc/rfc3596.html).

UNVERIFIED: Live endpoints and system resolver configurations. The resolver
uses the system lookup directly and the fixture exercises a literal address.

## RSS / Atom

Enter the feed's HTTP(S) URL. Public addresses require the provider internet
switch. The widget displays up to 20 RSS 2.0 items or Atom 1.0 entries in feed
order, with dates as published by the feed and optional trimmed summaries.
Feed content displays as text. Only absolute HTTP(S) article links are used.
The UTF-8 reader caps input at 128 KiB, XML depth at 24, attribute values at
1 KiB and display field lengths individually. It supports CDATA, predefined
XML entities and numeric references. DTDs and external entities are rejected.

Verified from: [RSS 2.0](https://www.rssboard.org/rss-specification),
[Atom 1.0](https://www.rfc-editor.org/rfc/rfc4287.html) and
[XML 1.0](https://www.w3.org/TR/xml/).

UNVERIFIED: Live feeds. Non-UTF-8 encodings and relative article links are unsupported.

## Weather (Open-Meteo)

With provider internet access enabled, enter latitude and longitude in decimal
degrees. Hobnail contacts only `api.open-meteo.com` and uses the free API for
current temperature and conditions and today's high and low in the location's
time zone. Choose Celsius or Fahrenheit and optionally show an outlook for
three days, including today. Refresh is at least 15 minutes.

Verified from: [Open-Meteo Forecast API and WMO code table](https://open-meteo.com/en/docs).

UNVERIFIED: Live API responses, quota behavior and location coverage.

## Clock and calendar

Enter up to eight IANA zone names, one per line. Add an optional label after
`|`, for example `America/Chicago | Home`. Choose a 12- or 24-hour clock and
an optional month calendar in the first recognized zone. The clock uses the
browser's own time and zone database, pauses while the tab is hidden, and
shows minutes only with reduced motion enabled. It makes no external requests.

Verified from: [IANA Time Zone Database](https://www.iana.org/time-zones)
and [Intl.DateTimeFormat](https://tc39.es/ecma402/#datetimeformat-objects).

UNVERIFIED: Zone availability in browser builds outside the Chromium fixture.

## Bookmarks

Make one integration per group and use its name as the group label. Enter up
to 16 lines in the form `Name | https://example.com | optional-icon`. Only
absolute HTTP(S) URLs without credentials are accepted. An optional
dashboard-icons slug uses the existing opt-in icon system. The widget never
fetches or checks the saved URLs.

Verified from: [URL Standard](https://url.spec.whatwg.org/) and Hobnail's
[icon configuration](../configuration.md).

UNVERIFIED: Third-party dashboard-icons slug availability.

## Markdown note

Notes are local and limited to 8 KiB, 64 blocks and 160 inline spans or
empty list entries. Use `#` through `###` headings, blank lines between
paragraphs, and ordinary newlines for visible line breaks. Consecutive
`- item`, `+ item` or `* item` lines form a bulleted list. Use `1. item` for
numbered lists. Use `**bold**`, `*italic*` (or underscores), backticks for
inline code, triple-backtick fences for code blocks and `[label](URL)` for
absolute HTTP(S) or mailto links. Inline markup is single-line and
non-nesting. Fenced code and HTML display as literal text. Images, relative
links and other Markdown constructs are outside this subset.

Verified from: [CommonMark 0.31.2 syntax](https://spec.commonmark.org/0.31.2/).

UNVERIFIED: Other Markdown dialects and their extensions.

## GitHub releases watcher

Select the releases widget on a GitHub integration after enabling provider
internet access. Enter up to ten lines in the form `owner/repo | v1.2.3`.
Current versions are manual. The widget queries GitHub's latest published
full release for each repository, with the shared GitHub cooldown and stale
data behavior. A fine-grained token is optional for public repositories.
Use Contents read access for private repositories.

The comparison ignores a leading `v`, compares one to three numeric version
components, and ignores valid build metadata. Unknown versions, including
prerelease suffixes, are displayed beside the latest tag without a comparison.
The refresh interval is one hour.

Verified from: [latest GitHub release](https://docs.github.com/en/rest/releases/releases#get-the-latest-release),
[GitHub REST rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)
and [SemVer precedence](https://semver.org/).

UNVERIFIED: Live repositories, private repository permissions and GitHub quota behavior.

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
and closes their upstream sockets. The standard widget refresh defaults to
60 seconds and ranges from 15 seconds to one hour. Public-internet providers
have a ten-minute minimum. A failed refresh keeps the last good data with its
stale timestamp and error. Output shares the 32 KiB per-widget and 256 KiB aggregate cache with
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
options. Local providers omit the URL and credentials. Settings and Guided
Setup render the same schema. Keep sensitive
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
| Basic | Pass the username and password fields as separate `Input::Credential` values to `Auth::Basic`. For OCS APIs, set `Step::ocs_api_request` to request the fixed `OCS-APIRequest: true` header. |
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
among a stat, stat grid, status list, item list, meter, table, clock, rich text
or unavailable message. The React client renders these shapes from the schema and registry.
The shared output boundary redacts credentials and known sessions, including
textual escapes and standard or URL-safe Base64. Redact JSON before truncating
or formatting it so an encoded credential cannot survive as a partial display
value. The redactor checks up to 128 KiB per string, 32 distinct secret values
up to 16 KiB apiece, and three layers of percent, JSON and common HTML escapes.
It withholds a string or link that exceeds the applicable bounds.
Exercise connection tests and fetches against a local fake service.

See [Boards](boards.md), [Login and security](login.md) and [API](../api.md).
