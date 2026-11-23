# Integrations

Open **Settings → Integrations → Add integration**, or choose **Add
integration** on a board. Search the catalog by service, category or
description. The catalog lists registered providers with working connection
tests and widgets: each card shows the service's logo on a faint tint of its
brand colour, its category and how many widgets it offers. The logos are
built into Hobnail (see [Icons](#icons)), so they show without internet
access.

Choose a provider to open its settings page: the logo and description, a
**Setup guide** link to the service's own documentation, the connection
fields with a hint on where to find each credential, and only the fields
that apply to the chosen API version (the Pi-hole v5 token appears only for
v5). Beside the form, **Widgets you can add** previews every widget. Give
the instance a name and enter its connection fields.
Use **Test connection** to test from the Hobnail server. The test reports
problems with DNS, connectivity, TLS, credentials, timeout or the response
format. You can correct inputs and retry in the same dialog or save the
connection to test later. A saved link or untested connection has no health
evidence.

After a successful test, choose a board. The **Add to board** gallery draws
each widget with the real renderer from example data (marked "Example"),
with its name, description, default board size (columns × rows) and a
**Recommended** badge on the one most people want; that one starts switched
on. Each selected widget has its own options and refresh interval. Use **Edit** on
a saved integration to test changes, configure its widgets, or place them on
another board. Each placement stores its own widget options and refresh
interval. Placements share the saved connection. Open boards with identical
fetch options share one cached result, even when their refresh intervals
differ. Create a separate integration for different URLs or credentials.
Remove a widget from a board in its board editor, or switch it off in the
integration's **Choose widgets** dialog (the removal and any additions are one
board revision). Deleting an
integration deletes its credentials and widgets and leaves missing targets in
existing layouts.

## Accepted limitations

These are accepted pending owner sign-off at merge.

- **No provider has been checked against a real instance.** Every provider
  is tested against fake servers built from the public documentation and
  source named in its "Verified from" line; "UNVERIFIED" lines list what
  those sources could not settle. Check each provider against your own
  service before relying on it.
- **Reachability uses TCP connect, not ICMP ping.** Ping would need extra
  container privileges; TCP connect reports reachability and connect time.
- **The GitHub releases watcher compares against a version you enter.** It
  does not read running container image tags.
- **TrueNAS 26 is not supported.** It removed the REST API; the provider
  is built from the TrueNAS SCALE 24.10 REST v2.0 API (25.x, where REST is
  deprecated, is unverified).
- **Duplicati shows no per-job result**, because its backup metadata has no
  authoritative per-run result.
- **Nextcloud shows activity counts only**, not an event feed.
- **Credentials are stored in plaintext in SQLite** (unchanged from before
  this release). They are write-only through the API and bound to their
  origin; protect the data volume and its backups.
- **Public services need Provider internet access**, which is off by default;
  public providers also have a ten-minute refresh floor.
- **Embed and extension widgets are not built** and are out of scope for
  this release.

## Icons

Hobnail ships optimised SVG logos for every provider and for common
container images (PostgreSQL, Redis, nginx, Grafana and others), about 94 KB
in all, and serves them from `/api/v1/icons/<name>` with a 30-day cache. They
need no internet access, so they show with the default `[icons] remote =
false`; other dashboard-icons names still need `[icons] remote = true`. A
container without an icon picks a built-in logo from its image name (for
example `pihole/pihole` or `jc21/nginx-proxy-manager`). Each logo sits on a
plate tinted with the brand colour, and single-colour dark logos (GitHub,
Tailscale) invert on dark themes. Widgets of a provider use its colour for
their bars, glyphs, sparklines and the rule under the tile header.

Logos come from [homarr-labs/dashboard-icons](https://github.com/homarr-labs/dashboard-icons)
(Apache License 2.0); brandless providers (Generic HTTP, Bookmarks, Clock,
Markdown note, Reachability, Weather, RSS, SSL expiry) use
[Lucide](https://lucide.dev) glyphs (ISC). Product names and logos are
trademarks of their owners, used for identification only. Sources and
licences are listed in [`src/http/brand/NOTICE.md`](../../src/http/brand/NOTICE.md).

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

The form shows only the credential of the selected version.

Widgets (4x2 / 4x4 grid units by default):

- **DNS activity** (recommended): stat tiles for queries, blocked queries,
  the blocked share and blocking status (green when enabled, amber when
  disabled). On v6 the queries tile carries a 24-hour sparkline: Hobnail
  reads `GET /api/history` (10-minute slots) and sums them into hourly
  points. The graph is optional: a 404 or an oversized response drops only
  the sparkline. v6 also reads `/api/stats/summary` and `/api/dns/blocking`;
  v5 reads all four values from the legacy summary and has no sparkline.
- **Top domains**: the ten most queried domains as ranked rows with their
  query count and a bar relative to the top entry. **Show blocked domains**
  lists the ten most blocked instead.
- **Top blocked domains**: the same list for blocked domains, marked red.
- **Top clients**: the ten clients that sent the most queries, the host
  name as the title and the address as secondary text.

The lists: v6 reads `/api/stats/top_domains` (with
`blocked=true` for blocked domains) and `/api/stats/top_clients`, both
with `count=10` and the session header. v5 reads `topItems=10` (its
`top_queries` or `top_ads` map) or `getQuerySources=10` (`top_sources`,
keyed `hostname|ip`) from `/admin/api.php` with the token. The v5 maps
arrive as JSON objects, so Hobnail ranks them by count, then name. Both
versions return an empty array or 401 without a valid credential, which
Hobnail reports as an authentication failure.

Verified against the public [authentication documentation](https://docs.pi-hole.net/api/auth/),
the upstream [v6 summary, top domains and top clients](https://github.com/pi-hole/FTL/blob/master/src/api/docs/content/specs/stats.yaml)
(paths in [main.yaml](https://github.com/pi-hole/FTL/blob/master/src/api/docs/content/specs/main.yaml))
[blocking](https://github.com/pi-hole/FTL/blob/master/src/api/docs/content/specs/dns.yaml)
and [activity graph](https://github.com/pi-hole/FTL/blob/master/src/api/docs/content/specs/history.yaml)
schema files, and the tagged [v5.21 summary, `topItems` and `getQuerySources`](https://github.com/pi-hole/web/blob/v5.21/api_FTL.php)
(authorization in [api.php](https://github.com/pi-hole/web/blob/v5.21/api.php))
and [token handling](https://github.com/pi-hole/web/blob/v5.21/scripts/pi-hole/php/password.php)
source. The v6 summary schema gives totals without specifying a calendar-day
window, so the widget labels that count **Queries**. Request flow and response
parsing were exercised against fakes. Compatibility across older minor
releases and live reverse-proxy configurations remains unverified.
UNVERIFIED: that every v6 release serves `/api/history` (handled as optional).

## Uptime Kuma

Enter the service URL and the slug from a published status page, such as
`home` from `/status/home`. Use letters, digits and hyphens for the slug.
The status page grants read access without credentials.

Widgets:

- **Monitor status** (recommended, 4x4): one row per monitor with a status
  dot (up, down, pending, maintenance or unknown from the latest
  heartbeat), the state and latest response time, the upstream 24-hour
  uptime as the value and the last 30 heartbeats as status bars. It shows
  up to 24 monitors and the current pinned incident when present.
- **Status summary** (4x2): tiles counting monitors up, down and
  pending/other, and their average 24-hour uptime.

Both read the same two public routes. Kuma's
public JSON routes supply these together. Its documented Prometheus metrics
expose monitor state and response time, but do not include the status page's
uptime or pinned incident.

The heartbeat route has no size option and returns the last 50 (1.23) or
100 (2.x) beats of every public monitor, which passes 128 KiB at about 20
monitors. Hobnail streams it and keeps only the latest status, ping, the
last 30 statuses and the 24-hour uptime of the monitors it displays. A monitor missing from the response
shows Unknown while the other rows stay current. Only a response over
16 MiB leaves every row at "Heartbeat unavailable".

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
the key only in the `Authorization` header using the MediaBrowser scheme.

| Widget | Shows | Source |
|---|---|---|
| **Now playing** (recommended, id `streams`) | Up to ten sessions with a current item: title (`Series · SxxEyy` for episodes), user and device, playback progress from `PlayState.PositionTicks` / `RunTimeTicks`, time left or "Paused". | `GET /Sessions` |
| **Library counts** | Tiles for movies, series, episodes, albums, songs and books, skipping empty kinds. | `GET /Items/Counts` |
| **Latest added** | Ten recently created items: title, type with the episode title or album artist, production year. | `GET /Items?recursive=true&includeItemTypes=Movie,Episode,MusicAlbum,Audio,Book&sortBy=DateCreated&sortOrder=Descending&limit=10` |

The unpaged `/Sessions` response can exceed the size limit, leaving Now
playing unavailable while other widgets refresh. The server-wide `/Items`
query works with an application key alone; `/Items/Latest` needs a user ID.

Verified from:

- [Jellyfin 10.11.10 OpenAPI](https://api.jellyfin.org/openapi/stable/jellyfin-openapi-10.11.10.json) and the [current stable OpenAPI](https://api.jellyfin.org/openapi/jellyfin-openapi-stable.json) (SessionInfoDto `UserName`, `Client`, `DeviceName`, `NowPlayingItem`, `PlayState`; PlayerStateInfo `PositionTicks`, `IsPaused`; BaseItemDto `SeriesName`, `AlbumArtist`, `IndexNumber`, `ParentIndexNumber`, `RunTimeTicks`, `ProductionYear`)
- [Authorization parser and API key lookup](https://github.com/jellyfin/jellyfin/blob/v10.11.10/Jellyfin.Server.Implementations/Security/AuthorizationContext.cs)
- [Sessions](https://github.com/jellyfin/jellyfin/blob/v10.11.10/Jellyfin.Api/Controllers/SessionController.cs), [server-wide item queries](https://github.com/jellyfin/jellyfin/blob/v10.11.10/Jellyfin.Api/Controllers/ItemsController.cs) and [user-specific latest media](https://github.com/jellyfin/jellyfin/blob/v10.11.10/Jellyfin.Api/Controllers/UserLibraryController.cs)

UNVERIFIED: compatibility outside the inspected releases and live proxies.

## qBittorrent

Enter the WebUI URL, username and password. qBittorrent exposes its WebUI
account without separate read-only scopes. Hobnail only logs in and reads
transfer data. Keep the session cookie name at `SID` for a default 5.0.5
WebUI. If your WebUI uses a custom or port-qualified name, enter the exact
configured name, such as `QBT_SID_8080`. Keep WebUI CSRF protection enabled.
Every request after login carries only the `SID` cookie and the bound
target's `Origin` and `Referer`; the session is renewed once on a 403.

**Torrent activity** (recommended, 4×2) shows tiles for download and upload
rates, the WebUI's active and seeding counts, queued uploads and downloads,
and free space on the default save path. A torrent can appear in more than
one count. Active and seeding counts stream the WebUI's filtered
`torrents/info` lists and count their entries without keeping them. Queued
and free-space figures stream the initial `sync/maindata` snapshot, keeping
only a queued counter and the `server_state.free_space_on_disk` number.
qBittorrent has no queued filter and reports free space only in that
snapshot, which lists every torrent in full (over 128 KiB at about 100
torrents). Each streamed response may carry up to 16 MiB, enough for
roughly 10,000 torrents. A larger list shows Unavailable for that count,
and transfer speeds remain visible. All four requests share the 5-second
provider deadline, so a slow WebUI with a very large library can time out.

**Torrents** (4×4) lists up to ten torrents from
`GET /api/v2/torrents/info?filter=<show>&sort=dlspeed&reverse=true&limit=10`
(sorted by `upspeed` for the seeding filter), fastest first. Choose Active
(default), Downloading, Seeding, Completed or All. Each row shows the name,
a progress bar, a readable state with a dot (green while transferring,
amber when stalled, red for an error or missing files, none when stopped,
queued or checking), the size, the time left while downloading, and the
current download or upload speed. `limit` keeps the response small.

Verified from:

- [WebUI API v2 documentation for qBittorrent 5.0](https://github.com/qbittorrent/qBittorrent/wiki/WebUI-API-(qBittorrent-5.0))
  (`torrents/info` parameters `filter`, `sort`, `reverse`, `limit`; fields
  `name`, `progress`, `dlspeed`, `upspeed`, `eta`, `state`, `size`; the
  state values)
- [5.0.5 login result](https://github.com/qbittorrent/qBittorrent/blob/release-5.0.5/src/webui/api/authcontroller.cpp) and [session cookies, CSRF and expired sessions](https://github.com/qbittorrent/qBittorrent/blob/release-5.0.5/src/webui/webapplication.cpp)
- [Sync snapshot and free space](https://github.com/qbittorrent/qBittorrent/blob/release-5.0.5/src/webui/api/synccontroller.cpp)
- [`torrents/info` filters, `limit` and `offset`](https://github.com/qbittorrent/qBittorrent/blob/master/src/webui/api/torrentscontroller.cpp)
  ([5.0.x](https://github.com/qbittorrent/qBittorrent/blob/v5_0_x/src/webui/api/torrentscontroller.cpp)).
  Released branches reset an `offset` past the end to 0, so paging cannot
  find the end of a list. Hobnail streams the unpaged list for counts.

UNVERIFIED: Compatibility outside the inspected version and live proxies;
the `8640000` "unknown ETA" value (from qBittorrent clients, not the wiki);
`stoppedUP`/`stoppedDL` and `forcedMetaDL` as the 5.x names of the paused
and forced-metadata states.

## Sonarr

Enter the service URL and the API key from Settings → General → Security.
Sonarr's instance API key is required and has no read-only scope. Hobnail
sends it only in `X-Api-Key` and uses only Sonarr API v3 GET routes.

| Widget | Shows | Source |
|---|---|---|
| **Episode calendar** (recommended, id `upcoming`) | A 7-day agenda from the start of today (UTC): series, `SxxEyy · episode title`, air time; downloaded episodes are marked. Up to 20 entries. | `GET api/v3/calendar?start&end&includeSeries=true` |
| **Sonarr overview** | Tiles: queue total, monitored missing episodes, health (OK, or the issue count, red when any issue is an error). | `api/v3/queue/status`, `api/v3/wanted/missing?page=1&pageSize=1&monitored=true`, `api/v3/health` |
| **Download queue** | Up to ten queue rows: series, `SxxEyy · title`, progress from `size`/`sizeleft`, time left while downloading or the queue status; tracked-download warnings and errors colour the dot. | `api/v3/queue?page=1&pageSize=10&includeSeries=true&includeEpisode=true` |
| **Missing episodes** | The ten most recently aired monitored episodes without a file, with the air date. | `api/v3/wanted/missing?…&sortKey=episodes.airDateUtc&sortDirection=descending&includeSeries=true` |

An oversized calendar, queue or missing response marks only that widget
unavailable; an oversized health response leaves the other tiles.

Verified from:

- [Sonarr 4.0.17.2952 API v3 OpenAPI](https://github.com/Sonarr/Sonarr/blob/v4.0.17.2952/src/Sonarr.Api.V3/openapi.json) (QueueResource `size`, `sizeleft`, `timeleft`, `status`, `trackedDownloadStatus`; EpisodeResource; queue and missing parameters)
- [Queue status](https://github.com/Sonarr/Sonarr/blob/v4.0.17.2952/src/Sonarr.Api.V3/Queue/QueueStatusController.cs), [missing episodes](https://github.com/Sonarr/Sonarr/blob/v4.0.17.2952/src/Sonarr.Api.V3/Wanted/MissingController.cs) and [calendar](https://github.com/Sonarr/Sonarr/blob/v4.0.17.2952/src/Sonarr.Api.V3/Calendar/CalendarController.cs)
- [Wanted sort key](https://github.com/Sonarr/Sonarr/blob/v4.0.17.2952/frontend/src/Store/Actions/wantedActions.js) (`episodes.airDateUtc`)

UNVERIFIED: the `timeleft` text format `[d.]hh:mm:ss` (a .NET TimeSpan;
anything else falls back to the status), compatibility outside the
inspected release and live proxies.

## Radarr

Enter the service URL and the API key from Settings → General → Security.
Radarr's instance API key is required and has no read-only scope. Hobnail
sends it only in `X-Api-Key` and uses only Radarr API v3 GET routes.

| Widget | Shows | Source |
|---|---|---|
| **Release calendar** (recommended, id `upcoming`) | A 7-day agenda of monitored movies, at the first cinema, digital or physical release inside the window, with the release kind and year; downloaded movies are marked. | `GET api/v3/calendar?start&end` |
| **Radarr overview** | Tiles: queue total, monitored missing movies, health. | `api/v3/queue/status`, `api/v3/wanted/missing?page=1&pageSize=1&monitored=true`, `api/v3/health` |
| **Download queue** | Up to ten queue rows: movie, year, progress, time left or status, tracked-download problems. | `api/v3/queue?page=1&pageSize=10&includeMovie=true` |
| **Missing movies** | Ten monitored movies without a file, by sort title, with year and release status. | `api/v3/wanted/missing?…&sortKey=movieMetadata.sortTitle&sortDirection=ascending` |

Verified from:

- [Radarr API v3 OpenAPI (develop)](https://github.com/Radarr/Radarr/blob/develop/src/Radarr.Api.V3/openapi.json) (QueueResource, MovieResource `inCinemas`, `digitalRelease`, `physicalRelease`, `hasFile`, `includeMovie`)
- [Radarr API v3 OpenAPI at 25a2124](https://github.com/Radarr/Radarr/blob/25a2124d7d5bb10aae6b89726577303f4bd60f75/src/Radarr.Api.V3/openapi.json), [queue status](https://github.com/Radarr/Radarr/blob/25a2124d7d5bb10aae6b89726577303f4bd60f75/src/Radarr.Api.V3/Queue/QueueStatusController.cs), [missing movies](https://github.com/Radarr/Radarr/blob/25a2124d7d5bb10aae6b89726577303f4bd60f75/src/Radarr.Api.V3/Wanted/MissingController.cs), [calendar](https://github.com/Radarr/Radarr/blob/25a2124d7d5bb10aae6b89726577303f4bd60f75/src/Radarr.Api.V3/Calendar/CalendarController.cs) and [release-window query](https://github.com/Radarr/Radarr/blob/25a2124d7d5bb10aae6b89726577303f4bd60f75/src/NzbDrone.Core/Movies/MovieRepository.cs)
- [Wanted sort key](https://github.com/Radarr/Radarr/blob/develop/frontend/src/Store/Actions/wantedActions.js) (`movieMetadata.sortTitle`)

UNVERIFIED: the serialised MovieStatusType values (`tba`, `announced`,
`inCinemas`, `released`; others show "Unknown"), the `timeleft` format,
compatibility outside the inspected revisions and live proxies.

## Prowlarr

Enter the service URL and the API key from Settings → General → Security.
Prowlarr's instance API key is required and has no read-only scope.
Hobnail sends it only in `X-Api-Key` and uses only Prowlarr API v1 GET
routes.

| Widget | Shows | Source |
|---|---|---|
| **Indexer activity** (recommended, id `indexers`) | Tiles: temporarily blocked indexers, and queries, grabs and failed grabs over the last seven days. | `GET api/v1/indexerstatus`, `api/v1/indexerstats?startDate&endDate` |
| **Indexer status** | The ten busiest indexers this week, blocked ones first with their unblock time, then grabs and average response time (or failed queries), coloured down / warn / up. | the same two routes |

The status endpoint reports temporary blocks due to upstream failures. The
grab count includes failed grabs. The large `/api/v1/indexer` list is not
read; indexers without statistics in the window are not listed.

Verified from:

- [Prowlarr API v1 OpenAPI](https://github.com/Prowlarr/Prowlarr/blob/46c468c443ff3ebb86fc99347d3a15885d41d1e1/src/Prowlarr.Api.V1/openapi.json) and [develop](https://github.com/Prowlarr/Prowlarr/blob/develop/src/Prowlarr.Api.V1/openapi.json) (IndexerStatistics `numberOfQueries`, `numberOfGrabs`, `numberOfFailedQueries`, `numberOfFailedGrabs`, `averageResponseTime`)
- [Indexer status](https://github.com/Prowlarr/Prowlarr/blob/46c468c443ff3ebb86fc99347d3a15885d41d1e1/src/Prowlarr.Api.V1/Indexers/IndexerStatusController.cs) and [blocked-provider filter](https://github.com/Prowlarr/Prowlarr/blob/46c468c443ff3ebb86fc99347d3a15885d41d1e1/src/NzbDrone.Core/ThingiProvider/Status/ProviderStatusServiceBase.cs)
- [Indexer stats](https://github.com/Prowlarr/Prowlarr/blob/46c468c443ff3ebb86fc99347d3a15885d41d1e1/src/Prowlarr.Api.V1/Indexers/IndexerStatsController.cs) and [grab aggregation](https://github.com/Prowlarr/Prowlarr/blob/46c468c443ff3ebb86fc99347d3a15885d41d1e1/src/NzbDrone.Core/IndexerStats/IndexerStatisticsService.cs)

UNVERIFIED: `averageResponseTime` is in milliseconds (the spec gives no
unit); compatibility outside the inspected revisions and live proxies.

## Jellyseerr / Overseerr

Enter the Jellyseerr or Overseerr service URL and the instance API key
from Settings → General. Both use API v1 and accept the key in
`X-Api-Key`. The shared key acts as the original administrator and has
no read-only scope. Hobnail sends it only in that header.

| Widget | Shows | Source |
|---|---|---|
| **Media requests** (recommended, id `requests`) | Tiles: pending (a warning while anything waits), approved, processing and available requests. | `GET api/v1/request/count` |
| **Reported issues** | Tiles: open and closed issues, and all issues by type (video, audio, subtitles, other). | `GET api/v1/issue/count` |

A recent-requests list is not offered: request objects carry only TMDB ids,
and resolving each title would take a large details request per row.

Verified from:

- [Jellyseerr 2.7.3 OpenAPI](https://github.com/Fallenbagel/jellyseerr/blob/v2.7.3/jellyseerr-api.yml), [request counts](https://github.com/Fallenbagel/jellyseerr/blob/v2.7.3/server/routes/request.ts) and [key authentication](https://github.com/Fallenbagel/jellyseerr/blob/v2.7.3/server/middleware/auth.ts)
- [Jellyseerr develop API spec](https://github.com/Fallenbagel/jellyseerr/blob/develop/seerr-api.yml) (`/request/count` fields including `processing`, `available`; `/issue/count` `open`, `closed`, `video`, `audio`, `subtitles`, `others`; MediaRequest without a title)
- [Overseerr 1.35.0 OpenAPI](https://github.com/sct/overseerr/blob/v1.35.0/overseerr-api.yml), [request counts](https://github.com/sct/overseerr/blob/v1.35.0/server/routes/request.ts) and [key authentication](https://github.com/sct/overseerr/blob/v1.35.0/server/middleware/auth.ts)

UNVERIFIED: `/issue/count` on Overseerr releases before issues existed
(they answer 404, shown as a status error); compatibility outside the
inspected releases and live proxies.

## AdGuard Home

Enter the Web UI URL and an AdGuard Home account username and password
(the web interface login from `AdGuardHome.yaml` → `users`). The control
API uses HTTP Basic authentication and has no read-only account scope.

Widgets (all read `GET /control/stats/config` and, when statistics are on,
`GET /control/stats`, whose one response carries the counters, the
per-time-unit series and the top lists):

- **DNS protection** (recommended, 4x2): stat tiles for queries over the
  configured statistics interval with a sparkline from `dns_queries`,
  filtering-rule blocks, their share of all queries (undefined with zero
  queries), the average processing time (`avg_processing_time`, seconds,
  shown in ms) and the protection setting from `GET /control/status`.
  With statistics off it shows "Statistics: Off" and protection only.
- **Top queried domains**, **Top blocked domains**, **Top clients** (4x4):
  the first ten entries of `top_queried_domains`, `top_blocked_domains` and
  `top_clients` as ranked rows with counts and bars relative to the top
  entry. With statistics off they say so.

Verified from:

- [Upstream OpenAPI for v0.107.73](https://github.com/AdguardTeam/AdGuardHome/blob/v0.107.73/openapi/openapi.yaml)
  (`Stats`, `TopArrayEntry` one-key objects, `GetStatsConfigResponse`)

UNVERIFIED: Other versions and live reverse proxies. `TopArrayEntry` is
loosely typed (`additionalProperties: number`); Hobnail reads each entry's
first key.

## Technitium DNS

Enter the web console URL and an API token created for a user with
Dashboard View, Zones View and View on each zone you want counted. Hobnail sends the token in the form
body of each read request.

The connection test verifies zone list access. Dashboard View permission
is checked when the widget refreshes.

Widgets (all read `POST /api/dashboard/stats/get`, the last hour by
default):

- **DNS activity** (recommended, 4x2): stat tiles for queries in the last
  hour with a per-minute sparkline (the "Total" dataset of
  `mainChartData`), blocked and cached answers, clients, and the total
  number of zones visible to that user (`POST /api/zones/list`, one zone
  per page, total only).
- **Top domains**, **Top blocked domains** (4x4): `topDomains` and
  `topBlockedDomains` as ranked rows with hit counts and bars.
- **Top clients** (4x4): `topClients` with the host name as the title and
  the address as secondary text.

Verified against Technitium DNS Server v14.3.0.

Verified from:

- [Upstream v14.3.0 API reference](https://github.com/TechnitiumSoftware/DnsServer/blob/v14.3.0/APIDOCS.md) ("Get Stats", "List Zones")

UNVERIFIED: Older pagination contracts, cluster aggregation and live reverse proxies.

## Tailscale

Enable Provider internet access in Settings → Integrations. Leave the API
URL at https://api.tailscale.com and enter your tailnet (or `-` for the
token’s tailnet) and an API access token. A device-read Bearer token with
`devices:core:read` is sufficient. Replace the token when it expires.
Hobnail sends it only to the origin it was saved for.

Widgets (each reads the API v2 device list once and refreshes at
intervals of ten minutes or longer):

- **Tailnet devices** (recommended, 4x4): up to twelve devices, online
  first, each with an online/offline dot, its first Tailscale address and
  operating system, and "Online" or a relative last-seen time.
- **Tailnet summary** (4x2): tiles for devices online and offline, keys
  expiring within 30 days and clients with an update available.
- **Expiring device keys** (4x3): up to ten device keys expiring in the
  next 30 days, earliest first, with "in N days" (red within a week).

Verified from:

- [Upstream authentication reference](https://tailscale.com/docs/reference/tailscale-api)
- [Upstream device-read scope](https://tailscale.com/docs/reference/trust-credentials)
- [Upstream device list client](https://github.com/tailscale/tailscale-client-go-v2/blob/f9e92a86ee85097dbabd255384403cb32b94f7c0/devices.go)
- [Upstream OpenAPI](https://api.tailscale.com/api/v2?outputOpenapiSchema=true) (`Device`: `addresses`, `os`, `updateAvailable`, `connectedToControl`, `lastSeen`)

UNVERIFIED: OAuth token renewal, live responses and large tailnets. Use the default `-` for account aliases containing `@`.

## WireGuard (wg-easy)

Enter the wg-easy Web UI URL and password. For v15 also enter the username
of the account whose peers you want to see. A regular v15 user can see
their assigned peers. An administrator can see all peers. v14 exposes the
shared Web UI password and has no narrower scope.

Hobnail exchanges credentials for a session cookie. It uses the v14
password-only login with `connect.sid`, or the v15 username/password login
with `wg-easy`, then reads `GET /api/wireguard/client` (v14) or
`GET /api/client` (v15).

Widgets:

- **WireGuard peers** (recommended, 4x4): up to ten peers, connected ones
  first. A peer counts as connected when its latest handshake is under
  three minutes old (WireGuard renews sessions every two minutes under
  traffic). Each row shows the relative handshake time, received and sent
  bytes, and "Disabled" for switched-off peers.
- **WireGuard summary** (4x2): tiles for peers, connected peers and total
  data received and sent.

Verified from:

- [Upstream v14 session routes](https://github.com/wg-easy/wg-easy/blob/v14/src/lib/Server.js)
- [Upstream v14 peer metrics](https://github.com/wg-easy/wg-easy/blob/v14/src/lib/WireGuard.js) (`enabled`, `latestHandshakeAt`, `transferRx`, `transferTx`)
- [Upstream v15.1.0 session and client routes](https://github.com/wg-easy/wg-easy/tree/v15.1.0/src/server/api)
- [Upstream v15.1.0 cookie and peer metrics](https://github.com/wg-easy/wg-easy/tree/v15.1.0/src/server/utils) (`clientStatus.ts` merges handshake and transfer into each client)

UNVERIFIED: v14 without a password, v15 accounts with two-factor authentication, other releases and live instances; that v15 clients carry `enabled` (read as enabled when absent).

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
the username and password for HTTP Basic authentication; they are sent only
in the `Authorization` header. Glances does not expose a plugin-specific
account scope. Hobnail uses the REST API v4 and falls back to v3 if the v4
CPU route (`/api/4/cpu`, also the connection test) returns 404.

- **Host resources** (recommended, 4×3): meters for CPU, memory and swap
  (when the host has swap) and the three fullest filesystems, with
  used-of-total detail (`cpu`, `mem`, `memswap`, `fs/top/10`).
- **CPU, memory and load** (4×2): tiles for CPU and memory use with a
  sparkline of their last 24 history points, and the 1, 5 and 15-minute
  load averages with the core count, amber when the 5-minute load exceeds
  it (`cpu`, `mem`, `load`, `cpu/total/history/24`,
  `mem/percent/history/24`). A server that keeps no history, or refuses
  the history request, shows the tiles without a trend line.
- **Top processes** (4×4): the ten busiest processes with PID, user, CPU
  and memory share (`processlist/top/10`).
- **Sensors** (4×3): up to ten temperature, fan and battery readings with
  units, and a dot from Glances' own warning and critical thresholds when
  it reports them (`sensors/top/10`).

Missing optional plugins (404, or v4's "Unknown plugin" 400) and oversized
responses show as Unavailable or are left out.

Verified from:

- [Upstream v4.3.3 REST API](https://github.com/nicolargo/glances/blob/v4.3.3/docs/api.rst)
  (shapes of `fs`, `load`, `memswap`, `processlist`, `sensors`; `/top/<n>`;
  `/<plugin>/<field>/history/<n>`)
- [Upstream v4 REST API](https://glances.readthedocs.io/en/latest/api/restful.html)
- [Upstream v3.4.0 REST API](https://glances.readthedocs.io/en/v3.4.0/api.html)

UNVERIFIED: v5 (which wraps plugin data differently), platform-specific
plugins and live reverse proxies; v3 history and process list (v3 has no
`/top`, so a large v3 process list shows as too large); that v4 lists
processes busiest first (Hobnail sorts the ten it receives by CPU); sensor
unit codes other than `C` and `R`.

## Plex

Enter your Plex Media Server URL and a Plex access token for that server
(in Plex Web: an item's ⋯ menu → Get Info → View XML, then the
`X-Plex-Token` value in the address). An owner token is needed for full
session visibility. Hobnail sends the token only in `X-Plex-Token` and
requests JSON.

| Widget | Shows | Source |
|---|---|---|
| **Now playing** (recommended, id `streams`) | Up to ten sessions: title (`Show · SxxEyy`, `Artist · Track`), user and player, progress from `viewOffset` / `duration`, time left, or Paused / Buffering. | `GET /status/sessions` |
| **Library counts** | A tile per library (up to six) with its item count and a glyph for its type (movie, show, artist, photo), from first-page count metadata. | `GET /library/sections`, `GET /library/sections/<key>/all` (page size 1) |
| **Recently added** | Ten items: title, the episode title, album artist or library, and the year. | `GET /library/recentlyAdded` (page size 10) |

An oversized response leaves independent data available and marks the
affected widget or count unavailable.

Verified from:

- [Upstream Media Server API reference](https://developer.plex.tv/pms/) (session and recently-added response samples: `grandparentTitle`, `parentIndex`, `index`, `duration`, `viewOffset`, `User.title`, `Player.title`/`product`/`state`, `librarySectionTitle`, `year`; library section `type`)
- [Upstream Media Server diagnostic trace for the global recently-added route](https://forums.plex.tv/t/library-scan-not-recognizing-video-files-after-update/898898)

UNVERIFIED: live token privilege combinations; `Player.state` values other
than playing, paused and buffering (shown as playing); the pagination of
the global recently-added route.

## Immich

Enter your Immich server URL and an API key from an administrator account
(Account Settings → API Keys). Grant only `server.statistics`,
`server.storage` and `job.read`. Hobnail sends the key only in the
`x-api-key` header.

The connection test verifies storage access. Library statistics and job
permissions are checked when a widget refreshes.

| Widget | Shows | Source |
|---|---|---|
| **Immich overview** (recommended) | Tiles: photos, videos, disk used "of" the disk size, and job activity (active, else waiting, else idle) with failed jobs as a warning. | `GET /api/server/storage`, `/api/server/statistics`, `/api/jobs` |
| **Storage by user** | A meter for the server disk, then up to eight users by library size: against their quota when one is set, otherwise their share of all media. | `/api/server/storage`, `/api/server/statistics` (`usage`, `usageByUser`) |
| **Background jobs** | The ten busiest job queues with active, waiting and failed counts; failures mark the row. | `/api/jobs` |

If a response exceeds the cap, the other endpoints can still supply their
data. A server that answers 404 for the deprecated `/api/jobs` route shows
the overview's jobs tile as unavailable and the jobs widget as "no job
queue route".

Verified from:

- [Upstream v2.5.6 OpenAPI schemas and permissions](https://github.com/immich-app/immich/blob/v2.5.6/open-api/immich-openapi-specs.json)
- [Upstream main OpenAPI (3.4.0-rc.0)](https://github.com/immich-app/immich/blob/main/open-api/immich-openapi-specs.json) (ServerStatsResponseDto `usage`, `usageByUser`; UsageByUserDto `userName`, `usage`, `quotaSizeInBytes`; QueueStatisticsDto `active`, `waiting`, `failed`)

UNVERIFIED: releases without the legacy GET /api/jobs endpoint and live
reverse proxies; queue names are shown by splitting their camelCase keys.

## Transmission

Enter the Web UI origin, including any reverse-proxy prefix before
`/transmission/rpc`. Supply the RPC username and password if the daemon
uses HTTP Basic authentication; they are sent only in the `Authorization`
header. The RPC credential has no read-only scope. Hobnail sends the real
RPC on a cold connection and retries once with the session ID supplied by
an HTTP 409 response (`X-Transmission-Session-Id`).

- **Torrent activity** (recommended, 4×2): tiles for global download and
  upload speed and the active, paused and total torrent counts from
  `session-stats` (paused and total are left out if the daemon omits them).
- **Torrents** (4×4): `torrent-get` with the fields `name`, `status`,
  `percentDone`, `rateDownload`, `rateUpload`, `eta`, `totalSize` and
  `error`, for the `recently-active` subset (default; torrents that just
  stopped are dropped) or for all torrents. The ten fastest show as rows
  with a progress bar, state (green while downloading or seeding, red on a
  torrent error), size, time left while downloading, and the current
  speed. A list over the response cap shows "Torrent list too large".

Verified from:

- [Upstream 4.0.6 RPC specification](https://github.com/transmission/transmission/blob/4.0.6/docs/rpc-spec.md)
  (torrent fields, the status 0–6 table, `recently-active`,
  `pausedTorrentCount` and `torrentCount`)
- [Upstream 4.0.6 statistics and torrent selection implementation](https://github.com/transmission/transmission/blob/4.0.6/libtransmission/rpcimpl.cc)

UNVERIFIED: Nonstandard RPC path suffixes, releases that remove legacy RPC
and live reverse proxies; the `eta` values -1 (not available) and -2
(unknown) beyond the `transmission.h` comments.

## SABnzbd

Enter the SABnzbd Web UI URL and the full API Key from Config → General.
The NZB Key lacks queue access. Hobnail uses only the queue and history
read calls and sends the key in the form body (`apikey`), never in a header
or the URL.

- **Download queue** (recommended, 4×2): tiles for download speed (green
  while downloading, amber with "Paused" when the queue is paused), the
  total number of queued jobs, data left with the estimated time left, and
  free space on the incomplete-download drive. It requests only one queue
  entry and reads the summary (`mode=queue&limit=1`).
- **Queued jobs** (4×4): the next ten jobs with a progress bar, state,
  category, size and time left (`mode=queue&limit=10`).
- **Recent history** (4×4): the last ten finished jobs with state (green
  when completed, red with the failure message when failed, amber while
  post-processing), category and size (`mode=history&limit=10`).

A wrong or missing key is reported as an authentication failure, whether
SABnzbd answers 403 with a plain message (4.5.x) or a JSON
`{"status":false,"error":"API Key Incorrect"}` report. Any other JSON
error report is an unexpected response.

Verified from:

- [Upstream queue API reference](https://sabnzbd.org/wiki/advanced/api)
- [JSON error reports (`report`)](https://github.com/sabnzbd/sabnzbd/blob/develop/sabnzbd/api.py)
- [Upstream v4.5.5 queue response, slot fields and units](https://github.com/sabnzbd/sabnzbd/blob/4.5.5/sabnzbd/api.py)
- [Upstream v4.5.5 history fields](https://github.com/sabnzbd/sabnzbd/blob/4.5.5/sabnzbd/database.py)
- [Upstream v4.5.5 API key validation](https://github.com/sabnzbd/sabnzbd/blob/4.5.5/sabnzbd/interface.py)

UNVERIFIED: Releases outside v4.5.5 and live reverse proxies; the full set
of post-processing status strings (shown as in progress).

## Home Assistant

Enter the Home Assistant URL and a long-lived access token from your profile
(Profile → Security → Long-lived access tokens). Use an account that can read
the selected entities. The connection test checks `GET /api/` with the token
in the Bearer header.

Both widgets take up to 16 distinct entity IDs such as `sensor.temperature`,
one per line, and read each with `GET /api/states/<entity_id>`. Missing and
oversized entities appear as unavailable while other readings remain
visible; a state answering for a different entity is refused.

- **Entity states** (recommended, 4x2): one tile per entity, named by its
  `friendly_name` (else the ID), with the state and any unit of measurement,
  a glyph from the entity domain (lock, media player, person, camera ...)
  and a highlight when on or unavailable.
- **Entity list** (4x4): the same entities as rows with on/off dots, the
  entity ID as secondary text, led by the on / off / unavailable counts.

Verified from: [Home Assistant REST API](https://contributors.home-assistant.io/core/internals/rest-api/),
[developer REST API reference](https://developers.home-assistant.io/docs/api/rest/)
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
GUI API access. Hobnail sends it only in `X-API-Key` and checks
`GET /rest/system/status`.

- **Synchronization** (recommended, 4×2): tiles for online peer devices
  and folders in sync (green when all, amber when some, red when none). It
  checks the first ten configured folders in ID order and notes how many
  are configured when there are more. A folder is locally in sync when
  idle with no pending changes or errors; paused folders count as checked
  but not in sync. It keeps the device count if the folder list is
  oversized, and the folder count if the device list is oversized
  (`/rest/system/connections`, `/rest/config/folders`,
  `/rest/db/status?folder=<id>`).
- **Folders** (4×4): the first ten folders with their state as a dot
  (in sync green; changes pending or working amber; error red; paused or
  unavailable plain), file count, a completion bar from `globalBytes` and
  `needBytes`, and the data left or the folder size.
- **Devices** (4×3): up to ten remote devices, connected first, named from
  `/rest/config/devices` (else the first block of the device ID), with the
  address and client version while connected, red when disconnected and
  plain when paused.

Verified from: [REST API](https://docs.syncthing.net/dev/rest.html),
[`/rest/system/connections`](https://github.com/syncthing/docs/blob/main/rest/system-connections-get.rst),
[v2.0.11 API source](https://github.com/syncthing/syncthing/blob/v2.0.11/lib/api/api.go)
and [folder summary](https://github.com/syncthing/syncthing/blob/v2.0.11/lib/model/folder_summary.go).

UNVERIFIED: older releases and remote-device folder completion; the
`/rest/config/devices` fields beyond `deviceID` and `name`.

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
the monitored nodes, `VM.Audit` on the monitored guests and
`Datastore.Audit` on the storages to show. A propagated `PVEAuditor` grant
on `/` covers the cluster. Privilege-separated tokens also require the
underlying user to have the corresponding grants. For self-signed TLS,
check the server's SHA-256 fingerprint and confirm the matching leaf pin in
Hobnail. Requests carry `Authorization: PVEAPIToken=<id>=<secret>` only.

The connection test reads `/api2/json/version`. Every widget reads one
filtered `/api2/json/cluster/resources?type=…` list, which shows only what
the token may audit. A list over the response cap shows a message instead
of failing the widget.

- **Cluster overview** (recommended, 4×2): tiles for online nodes (green
  when all are online, amber when some, red when none), running VMs and
  running containers out of all of each, excluding templates
  (`type=node`, `type=vm`).
- **Node resources** (4×3): CPU (with the CPU count), memory and root disk
  meters for up to four nodes, by name; an offline node shows as Offline
  (`type=node`: `cpu`, `maxcpu`, `mem`, `maxmem`, `disk`, `maxdisk`).
- **Virtual machines and containers** (4×4): up to ten guests, running
  first then by ID, with ID and name, VM or container, node, uptime or
  state, and CPU and memory while running. Templates are left out. Choose
  all guests or only running ones (`type=vm`: `vmid`, `name`, `type`,
  `node`, `status`, `uptime`, `cpu`, `mem`, `template`).
- **Storage** (4×3): used space of up to eight storages, fullest first,
  labelled with their node unless shared; a shared storage is listed once;
  a storage whose status is not `available` is marked Unavailable
  (`type=storage`: `storage`, `node`, `status`, `disk`, `maxdisk`,
  `shared`).

Verified from: [Proxmox VE token and role docs](https://pve.proxmox.com/pve-docs/chapter-pveum.html),
[API schema (`/cluster/resources` return fields and type filter)](https://pve.proxmox.com/pve-docs/api-viewer/apidoc.js),
[version API](https://github.com/proxmox/pve-manager/blob/d147600a85186460650b5c45c453948230e6a0fb/PVE/API2.pm)
and [cluster API source](https://github.com/proxmox/pve-manager/blob/d147600a85186460650b5c45c453948230e6a0fb/PVE/API2/Cluster.pm).

UNVERIFIED: live clusters and releases with different resource schemas;
status strings other than `available` for storages and `running`/`stopped`
for guests (the schema types `status` as a free string).

## TrueNAS SCALE

Enter the TrueNAS SCALE URL and API key. The key needs read access to
`disk.query`, `pool.query`, `alert.list` and `smart.test.results`. On
24.10, a standard UI-generated legacy API key has broad appliance access.
Hobnail sends it only as a Bearer token and tests a one-entry disk query.
Widgets refresh at most once a minute (default every five minutes).

- **Storage health** (recommended, 4×4): rows for the non-dismissed alert
  count (green at zero, amber otherwise), up to ten pools with a status dot
  (ONLINE green, DEGRADED amber, FAULTED, OFFLINE, UNAVAIL or REMOVED red),
  free space and a used-space bar, and up to ten disks with their latest
  reported SMART self-test (passed green, failed red, aborted amber).
  Missing results stay unknown. The provider reads the test log and does
  not start a test. Oversized lists leave independent rows visible. If the
  SMART results route answers 404, each disk reads "SMART results
  unavailable" and the other rows stay visible.
- **Pool usage** (4×2): used space of up to ten pools as meters, from
  `pool.query` `allocated` and `size`; pools without a known size are left
  out.
- **Alerts** (4×3): up to ten non-dismissed alerts, most severe first,
  with their formatted text and level (critical and above red, warning
  amber).

Endpoints: `GET /api/v2.0/alert/list`, `/pool?limit=11&sort=name`,
`/disk?limit=11&sort=name`, `/smart/test/results?limit=10&sort=disk`.

Verified from [24.10.2 REST v2.0](https://www.truenas.com/docs/scale/24.10/api/scale_rest_api.html),
[Bearer auth source](https://github.com/truenas/middleware/blob/TS-24.10.2/src/middlewared/middlewared/restful.py),
[SMART results source](https://github.com/truenas/middleware/blob/TS-24.10.2/src/middlewared/middlewared/plugins/smart.py),
[pool fields](https://github.com/truenas/middleware/blob/TS-24.10.2/src/middlewared/middlewared/plugins/pool_/pool.py),
[alert fields](https://github.com/truenas/middleware/blob/TS-24.10.2/src/middlewared/middlewared/plugins/alert.py)
and [alert levels](https://github.com/truenas/middleware/blob/TS-24.10.2/src/middlewared/middlewared/alert/base.py),
and [TrueNAS 26 API removal notes](https://www.truenas.com/docs/scale/26/gettingstarted/versionnotes/).

The REST API is deprecated in 25.04 and 25.10, and removed in TrueNAS 26.
TrueNAS 26 requires JSON-RPC over WebSocket, which the provider connection
contract does not support.

UNVERIFIED: live appliances, 25.x REST compatibility and disk health
beyond the latest SMART self-test result; whether an alert's `formatted`
text can carry markup (shown as plain text).

## Gitea / Forgejo

Enter the forge URL and a personal access token with `read:repository`,
`read:user` and access to the selected repos. Hobnail sends `Authorization: token …`.
The connection test checks `/api/v1/version`, then a one-item
`/api/v1/user/repos` request to verify the token.

**Repository activity** counts repositories visible to the token and shows
separate issue and PR counts for up to eight configured `owner/repository`
names. It reads the most recent Actions run on Gitea 1.26 and Forgejo 15.
If the run endpoint is absent, it reads the latest Actions task when
available (including on Gitea 1.24), and labels it as a task. A repository
that answers 403, 404, 410 or 451 shows "Repository unavailable" without
failing the other rows.

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

Each repository takes three sequential requests, each on a new connection,
and the whole refresh must finish within the 5-second provider deadline.
After about 4.25 seconds Hobnail stops starting requests and shows the rows
fetched so far. The remaining rows read "Not checked this refresh (time
limit)" until a later refresh reaches them. If not even the first repository
completes, the widget reports a timeout. Rows are fetched in configured
order, so on a slow link the last repositories can stay unchecked. List
fewer repositories, or the most important ones first.

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
The UTF-8 reader keeps at most the first 128 KiB and stops downloading
there. For a longer feed it shows the entries that end within that prefix and
ignores the rest, so a feed with very long entries can show fewer than 20.
If no entry ends within 128 KiB, the widget reports an unexpected response.
XML depth is capped at 24. Each title, date and summary is read whole, up
to 16 KiB, redacted, and only then shortened for display, so a cut never
leaves the start of a credential; a longer field is not shown at all. An
attribute value over 1 KiB, such as an inline image, is skipped rather than
rejecting the feed. It supports CDATA, predefined XML entities and numeric
references. DTDs and external entities are rejected.

RSS descriptions and Atom summaries marked `type="html"` are HTML, often
entity-escaped. Hobnail shows their text: it drops tags, comments and
script or style content, and decodes common HTML entities. Atom text
summaries are shown as written. Summaries never render as HTML, and the
visible summary is capped at 240 bytes after markup is removed (from up to
2 KiB of markup per entry).

Verified from: [RSS 2.0](https://www.rssboard.org/rss-specification),
[Atom 1.0](https://www.rfc-editor.org/rfc/rfc4287.html) (including
[text constructs](https://www.rfc-editor.org/rfc/rfc4287.html#section-3.1)),
[RSS best practices on HTML in descriptions](https://www.rssboard.org/rss-profile#element-channel-item-description) and
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
The refresh interval is one hour. Like repository activity, the watcher
returns the repositories checked within the provider deadline and marks the
rest "Not checked this refresh (time limit)". A repository that answers
403, 404, 410 or 451 (other than a rate limit) shows "Latest release
unavailable" without failing the other rows.

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
the draft pin when the URL's origin changes. Adding or changing the pin of an
integration with saved credentials requires entering them again (or clearing
them) in the same save, as for an origin change, so they never follow a new
key unconfirmed. Removing the pin keeps them. Only confirm a fingerprint you
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
checks every resolved address before an HTTP connection. Private, loopback,
link-local and unique-local addresses count as LAN destinations, as does the
100.64.0.0/10 shared range that Tailscale uses for its nodes (except the
100.100.100.200 metadata address). Public destinations require the switch even
if the hostname is in `allow_hosts`. Hostname
resolution still uses the server's DNS. The switch is for catalog providers,
independent of file-configured widgets or dashboard-icons.

Provider widgets require access to both the board and the integration. A board
viewer without the integration grant sees a neutral **Widget unavailable**
placeholder. That viewer's live stream does not receive widget data or start
polling. An integration tile is likewise shown as **Integration unavailable**,
without its integration ID or custom title; a scoped editor saving the board
keeps the tile as it was. Removing the integration grant also removes access on open streams.
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
have a ten-minute minimum. So does any widget whose last fetch resolved its
destination to a public address, such as an RSS feed, Generic HTTP check,
SSL expiry or reachability check aimed at an internet host: after the first
such fetch it refreshes at most every ten minutes, whatever interval was
chosen. The widget options still show the chosen interval. The floor is
re-evaluated on each fetch, so moving the URL to a LAN host drops it after
the next fetch. A destination that fails before its name resolves (for
example a DNS error) is not known to be public and retries on its normal
interval. A failed refresh keeps the last good data with its
stale timestamp and error. After a change to an integration's URL,
credentials or TLS pin, the internet switch or `[http] allow_hosts`, its
widgets keep showing their last data, marked stale, and refetch on the next
tick; public-internet providers and widgets whose last fetch reached a
public address wait for their minimum instead, to protect the service's
quota. Saving other settings or editing `config.toml` leaves
provider widget data in place. Output shares the 32 KiB per-widget and 256 KiB aggregate cache with
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
| Streamed JSON | For a response that can exceed the 128 KiB step cap (a full torrent list, every heartbeat), call `Conn::scan(step, sink)` and feed the sink to `json_scan::Scanner` with a visitor that keeps only bounded state, such as a counter. Chunks are dropped after the visitor sees them, so memory stays at one chunk (under 64 KiB, measured) plus the fixed-size scanner. A streamed body may carry up to 16 MiB before it fails as too large, and the operation deadline still applies. Non-2xx responses are collected as usual so their status can be mapped. |
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

Give the provider a brand `color` (`#rrggbb`, the logo's main colour) and an
`icon` slug; add the logo to `src/http/brand/` (optimised, under 10 KiB, with
its source and licence in `NOTICE.md`) and to `BRANDS` in
`src/http/icons.rs` with the same colour. A field can declare `when` to apply
only for one value of another field, such as an API version.

Each widget declares a `description`, `default_size` and `min_size` in board
grid units (12 columns, rows of about 80 px; summaries are usually 4 × 2 and
lists 4 × 4) and a `sample`: a small, static, fictional `StructuredBody` in
JSON for the gallery, in the same shape the widget returns. Mark exactly one
widget per provider `recommended`. The registry test checks all of these.

Return a typed `Fault`. Keep raw responses, credentials and sessions out of
errors and logs. A widget returns a `StructuredBody` with display data. Choose
among a stat, stat grid, status list, item list, meter, table, clock, rich text
or unavailable message, or the richer meters (labelled bars), tiles (a value
with a generic glyph, a delta, a tone and an optional sparkline series), rows
(status dot, secondary text, trailing value, progress and recent status
bars) and agenda (dated entries the client groups by day). The React client
renders every shape generically, with no code per provider.
The shared output boundary redacts credentials and known sessions, including
textual escapes and standard or URL-safe Base64. Redact JSON before truncating
or formatting it so an encoded credential cannot survive as a partial display
value. The redactor checks up to 128 KiB per string, 32 distinct secret values
up to 16 KiB apiece, and three layers of percent, JSON and common HTML escapes.
It withholds a string or link that exceeds the applicable bounds.
Matching is exact, so the redactor does not detect every echo of a secret. It
misses a secret whose letter case was changed or that was re-encoded in hex.
It also misses a secret Base64-encoded inside a larger blob, unless the secret
starts the encoded data on a 3-byte boundary, because the surrounding bytes
change its encoding. A provider must not display fields that can carry such
echoes.
Every saved credential field is a redaction needle, including usernames
stored as secrets (qBittorrent, AdGuard Home, wg-easy, and the optional
HTTP Basic users of Transmission, Glances and Nextcloud). A short or common username such as `admin` is therefore
replaced with `<redacted>` wherever it appears in display text, for
example inside a torrent, client or monitor name. This fails safe: labels
can be mangled, but no credential is shown. Use a distinctive username if
the mangling gets in the way.
A provider that splits, joins, strips markup from, decodes or folds upstream
text before display can reshape a secret so the final pass misses it. Build
such text with `OutputRedactor::transformed`, which withholds the whole
value when any source matches and redacts the result before any
truncation. Pi-hole client labels and feed titles, dates and summaries use it.
Exercise connection tests and fetches against a local fake service.

See [Boards](boards.md), [Login and security](login.md) and [API](../api.md).
