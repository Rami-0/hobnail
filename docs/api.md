# HTTP API

The Rust access table (`src/api/access.rs`) declares the v1 routes below.
GET also accepts HEAD. Public means no session; Viewer means any signed-in
account; Admin includes Owner; Owner manages accounts and invitations. All
application data requires login. Public shell/assets, health, session probes
and credential entry are exceptions. Writes require matching Origin (or
Referer) and Host; authenticated writes also require `X-RH-CSRF`.

Scopes further restrict these roles: only granted boards/integrations are
visible; containers come from granted boards. Scoped accounts cannot read
host-wide pages or administrative configuration. Viewer responses omit
credentials, integration paths, environment values and administrative diagnostics.
See [login](features/login.md), [boards](features/boards.md) and
[integrations](features/integrations.md). JSON responses are `no-store`.
Buffered API `application/json` bodies from 1,025 to 262,144 bytes carry
`Vary: Accept-Encoding` and use gzip when the request accepts it. An
explicit `gzip;q=0` selects identity. Larger JSON responses use identity.
Event streams, log downloads, and static assets bypass runtime gzip.

The session cookie uses `HttpOnly; SameSite=Strict; Path=/` with a seven-day
sliding expiry. Issuing, renewing or clearing it adds `Secure` when a
[trusted proxy](configuration.md#incoming-reverse-proxy) reports HTTPS.
Plain HTTP uses the same session flow without the flag.

Credential-entry throttling returns `429 Too Many Requests` with
`Retry-After` in seconds. The per-IP limiter uses the TCP peer by default and
honours `X-Forwarded-For` only from configured trusted proxies. The global
failed-attempt limit also applies. The server runs one credential operation
at a time. Up to eight additional login, onboarding, invitation acceptance,
or password-change requests can wait up to five seconds for it. A full queue
or expired wait returns `429 Too Many Requests` with `Retry-After: 1`.

## Version 1

| Method | Path | Access | Purpose |
|---|---|---|---|
| GET | `/api/v1/logs/picker` | Viewer | Logs picker. |
| GET | `/api/v1/settings` | Viewer | Dashboard settings. |
| PATCH | `/api/v1/settings` | Admin | Dashboard settings. |
| GET | `/api/v1/icons/<name>` | Viewer | Dashboard icon. |
| GET | `/api/v1/boards` | Viewer | Boards. |
| POST | `/api/v1/boards` | Admin | Board create. |
| POST | `/api/v1/boards/test-link` | Unscoped Admin or Owner | Test a service URL entered in the board editor using the connection policy. |
| GET | `/api/v1/boards/<id>` | Viewer | Board. |
| PUT | `/api/v1/boards/<id>` | Admin | Board save. |
| PATCH | `/api/v1/boards/<id>` | Admin | Board update. |
| DELETE | `/api/v1/boards/<id>` | Admin | Board delete. |
| GET | `/api/v1/boards/<id>/suggestions` | Admin | Board suggestions, capped at 256. |
| GET | `/api/v1/boards/<id>/draft` | Admin | Board draft. |
| PUT | `/api/v1/boards/<id>/draft` | Admin | Board draft save. |
| DELETE | `/api/v1/boards/<id>/draft` | Admin | Board draft delete. |
| GET | `/api/v1/providers` | Viewer | Implemented providers and schemas |
| POST | `/api/v1/providers/test` | Admin | Test draft connection fields |
| GET | `/api/v1/providers/widgets` | Admin | Read widget options for an integration |
| POST | `/api/v1/providers/install` | Admin | Save an instance and add selected widgets |
| GET | `/api/v1/integrations` | Viewer | Integrations. |
| POST | `/api/v1/integrations` | Admin | Integration create. |
| GET | `/api/v1/integrations/<id>` | Viewer | Integration. |
| PUT | `/api/v1/integrations/<id>` | Admin | Integration update. |
| DELETE | `/api/v1/integrations/<id>` | Admin | Integration delete. |
| PUT | `/api/v1/integrations/<id>/secret` | Admin | Integration set secret. |
| DELETE | `/api/v1/integrations/<id>/secret` | Admin | Integration clear secret. |
| POST | `/api/v1/integrations/<id>/test` | Admin | Integration test. |
| GET | `/api/v1/session` | Public | Session. |
| GET | `/api/v1/bootstrap` | Viewer | Bootstrap. |
| GET | `/api/v1/containers` | Viewer | Containers. |
| GET | `/api/v1/containers/<name>` | Viewer | Container. |
| GET | `/api/v1/containers/<name>/metrics` | Viewer | Container metrics. |
| GET | `/api/v1/containers/<name>/inspect` | Viewer | Container inspect. |
| GET | `/api/v1/containers/<name>/events` | Viewer | Container events. |
| GET | `/api/v1/containers/<name>/env` | Admin | Container env. |
| GET | `/api/v1/containers/<name>/top` | Admin | Container top. |
| GET | `/api/v1/alarm` | Viewer | Alarm. |
| GET | `/api/v1/strips` | Viewer | Strips. |
| GET | `/api/v1/history` | Viewer | History. |
| GET | `/api/v1/bookmarks` | Viewer | Bookmarks. |
| GET | `/api/v1/widgets` | Viewer | Widgets. |
| GET | `/api/v1/alerts` | Viewer | Alerts. |
| GET | `/api/v1/overview` | Viewer | Overview. |
| GET | `/api/v1/host` | Viewer | Host. |
| GET | `/api/v1/host/history` | Viewer | Host history. |
| GET | `/api/v1/stream` | Viewer | Stream. |
| GET | `/api/v1/onboarding/draft` | Owner | Read the guided setup draft or its completed board ID. |
| PUT | `/api/v1/onboarding/draft` | Owner | Save draft fields and write-only credential changes. |
| GET | `/api/v1/onboarding/discovery` | Owner | Read at most 256 service suggestions and source availability. |
| POST | `/api/v1/onboarding/test` | Owner | Test an included item from the saved draft. |
| POST | `/api/v1/onboarding/commit` | Owner | Create the first board and mark the draft complete atomically. |
| POST | `/api/v1/onboarding/owner` | Public | Onboard. |
| POST | `/api/v1/invites/accept` | Public | Invite accept. |
| GET | `/api/v1/invites/validate` | Public | Invite validate. |
| GET | `/api/v1/invites` | Owner | Invites. |
| POST | `/api/v1/invites` | Owner | Invite create. |
| DELETE | `/api/v1/invites/<id>` | Owner | Invite revoke. |
| GET | `/api/v1/users` | Owner | Users. |
| PATCH | `/api/v1/users/<id>/role` | Owner | User role. |
| PATCH | `/api/v1/users/<id>/disabled` | Owner | User disabled. |
| PUT | `/api/v1/users/<id>/scopes` | Owner | User scopes. |
| POST | `/api/v1/users/<id>/sign-out` | Owner | User sign out. |
| DELETE | `/api/v1/users/<id>` | Owner | User delete. |
| POST | `/api/v1/account/board-tour` | Viewer | Record completion or dismissal of the board tour for the signed-in account. |
| POST | `/api/v1/account/password` | Viewer | Password. |
| GET | `/api/v1/account/sessions` | Viewer | Sessions. |
| DELETE | `/api/v1/account/sessions/<id>` | Viewer | Session revoke. |
| DELETE | `/api/v1/account/sessions` | Viewer | Sessions revoke. |

### Board freshness and tour

Provider catalog and installation:

- `GET /api/v1/providers` returns implemented provider descriptors, typed
  connection fields and per-widget option schemas. The `public_internet` and
  `contacts` fields describe the provider's outbound behavior.
- `POST /api/v1/providers/test` accepts `{integration, integration_id?}`.
  The draft connection uses the integration input below. Supplying an existing
  ID lets the test reuse its saved credentials for the same origin. The
  response has `ok`, `class`, `message`, `latency_ms` and a `fingerprint` when a
  failed TLS handshake presented a leaf certificate. The response never
  includes upstream data. The test does not write the draft or record saved
  health evidence.
- `POST /api/v1/providers/install` accepts
  `{integration, integration_id?, board_id?, widgets?}`. Each selected
  widget is `{id, placement?, options}`. Use the widget descriptor ID for
  `id` and the widget's saved placement ID when editing it. A nonempty
  selection requires a board. Connection and board updates commit together.
  The selected board's placement gets the options and refresh interval. An
  empty selection saves the connection alone.
- `GET /api/v1/providers/widgets?integration=<id>&board=<id>` returns a map
  of widget descriptor IDs to arrays of placements `{id, options}`. Scoped
  admins can test or update a granted integration by supplying its ID. Reading
  or saving widget placements also requires that board's grant. Installing or
  draft testing a new integration requires unscoped Admin or Owner access.
- Integration input uses `name`, `kind` (the provider ID), `base_url`,
  `enabled`, and optional `icon`, `options`, `fingerprint`, and
  `credentials`. Each credential field ID maps to a replacement string or
  null to clear it. Omit a field to keep its saved value for the same origin.
  Admin details expose a map of saved field IDs to bound origins. The legacy
  Generic HTTP `secret` and `clear_secret` inputs still address its bearer
  field. `has_secret` indicates whether any field is saved.
  `fingerprint` is 64 hexadecimal digits for an HTTPS URL.
- Add `board=<id>` to a stream that subscribes to the `widget` topic to
  receive provider widget updates for that board. The caller must be able
  to read it. Placements with identical fetch inputs share the result. Provider widget views supply a discriminated
  `structured` display body alongside the existing status and freshness
  fields.

Widget reads, including board resolution and widget stream events, return
`status: "stale"` when the latest refresh fails after a successful result.
`body` and `values` retain that result, `last_good_at` gives its Unix timestamp,
`fetched_at` gives the latest attempt, and `error` explains the failure. An
initial failure has `status: "error"`, a null body and empty values.

Integration reads, including board resolution, expose `enabled` and a nullable
`check`. The check contains the latest manual verdict and the timestamp and
latency of the last successful test if there was one. Connection changes and
server restart clear the in-memory test evidence. The test POST still returns
the single attempt's verdict.

Signed-in sessions return `board_tour_pending: true` until the account dismisses
or completes its tour. POST `/api/v1/account/board-tour` with the session and CSRF
token to clear the flag. The write is idempotent and returns 204.

### Guided setup

Browser owner creation initializes one setup draft for the new owner. A signed-in
owner with unfinished setup gets `setup_pending: true` in the session response.
Existing owners and owners created through host recovery keep their current
landing page.

- Read `/onboarding/draft` for the current revision, document, effective theme,
  and any staged credential origins. A completed draft carries its `board_id`.
- PUT the current `revision`, the complete `document` and a list of `credentials`
  changes to `/onboarding/draft`. Item IDs are 32 lowercase hexadecimal digits.
  The document contains the current step, an optional theme and the included or
  excluded items. Provider targets include `kind: "provider"`, the registry
  ID in `provider`, plus the same `base_url`, `enabled`, `fingerprint` and
  `options` as Settings. Each credential change supplies `item_id`, schema
  `field`, `base_url` and a write-only `secret`. Omit the change to keep that
  field, or send null to clear it. Draft responses return saved field origins
  under `credential_bindings`.
  Setup writes accept up to 2 MiB and 512 items. Item names and sections
  accept up to 256 UTF-8 bytes, URLs up to 2,048 bytes, and icon names up to
  96 bytes. The legacy discovery source field accepts up to 4,096 bytes.
- GET `/onboarding/discovery` for Docker availability, config status and the
  regular board suggestions. Docker failure leaves the other sources usable.
- POST `revision` and `item_id` to `/onboarding/test` to test a saved, included
  item. It uses the existing connection policy, rate limit and verdict format.
  A test verdict does not affect whether setup can be committed. Link tests from
  setup and the board editor use the same outbound policy. Test URLs must use
  HTTP or HTTPS and omit query strings and fragments.
- POST `revision` to `/onboarding/commit` from Review. A single transaction creates
  the board and any new integrations, sets the default board and optional theme,
  clears staged credentials and records the resulting `board_id`. Repeated
  commits return that ID. A stale save, test, or first commit returns 409.
- Dashboard settings accept `default_board`, `default_theme`, and `provider_internet`,
  in a PATCH. Null clears the corresponding setting. A cleared theme falls
  back to `[page].theme` in the configuration file.

## Kept legacy routes

These use the same guards. Scoped accounts can use logout, granted container
history/logs and icons; other legacy reads and all legacy Settings are refused.

| Method | Path | Access | Purpose |
|---|---|---|---|
| POST | `/api/login` | Public | Sign in with username and password. |
| POST | `/api/logout` | Viewer | Revoke this session. |
| GET | `/api/history/system` | Viewer | Host metric series. |
| GET | `/api/history/container/<name>` | Viewer | Visible container metric series. |
| GET | `/api/events` | Viewer | Event history; auth events require Admin. |
| GET | `/api/logs/stream` | Viewer | Bounded live or historical log SSE. |
| GET | `/api/logs/download` | Viewer | Download logs as text. |
| GET | `/api/logs/levels` | Viewer | Recorded log-level counts. |
| GET | `/api/logs/status` | Viewer | Background-tail status. |
| GET | `/api/settings/config` | Admin | Read config. |
| POST | `/api/settings/config` | Admin | Update or execute config. |
| POST | `/api/settings/config/validate` | Admin | Update or execute validate. |
| GET | `/api/settings/revisions` | Admin | Read revisions. |
| GET | `/api/settings/revision/<id>` | Admin | Read <id>. |
| GET | `/api/settings/revision/<id>/diff` | Admin | Read diff. |
| POST | `/api/settings/revision/<id>/restore` | Admin | Update or execute restore. |
| GET | `/api/settings/general` | Admin | Read general. |
| POST | `/api/settings/general` | Admin | Update or execute general. |
| GET | `/api/settings/containers` | Admin | Read containers. |
| POST | `/api/settings/docker` | Admin | Update or execute docker. |
| POST | `/api/settings/groups` | Admin | Update or execute groups. |
| POST | `/api/settings/containers/override` | Admin | Update or execute override. |
| GET | `/api/settings/bookmarks` | Admin | Read bookmarks. |
| POST | `/api/settings/bookmarks` | Admin | Update or execute bookmarks. |
| GET | `/api/settings/widgets` | Admin | Read widgets. |
| POST | `/api/settings/widgets` | Admin | Update or execute widgets. |
| POST | `/api/settings/widgets/preview` | Admin | Update or execute preview. |
| POST | `/api/settings/system` | Admin | Update or execute system. |
| POST | `/api/settings/undo-delete` | Admin | Update or execute undo-delete. |
| GET | `/api/settings/notifications` | Admin | Read notifications. |
| POST | `/api/settings/channels` | Admin | Update or execute channels. |
| POST | `/api/settings/channels/test` | Admin | Update or execute test. |
| POST | `/api/settings/alerts` | Admin | Update or execute alerts. |
| GET | `/api/settings/appearance` | Admin | Read appearance. |
| POST | `/api/settings/appearance` | Admin | Update or execute appearance. |
| GET | `/api/settings/history` | Admin | Read history. |
| POST | `/api/settings/history` | Admin | Update or execute history. |
| POST | `/api/settings/history/clear` | Admin | Update or execute clear. |
| GET | `/api/settings/history/export` | Admin | Read export. |
| GET | `/api/settings/security` | Admin | Read security. |

Public GET/HEAD resources: `/app`, `/app/*`, `/assets/*`, `/healthz`,
`/manifest.webmanifest`, `/favicon.ico`, `/icon.svg`, `/icon-192.png`,
`/icon-512.png`, `/apple-touch-icon.png`. `/custom.css` and `/icons/*`
require Viewer. Hashed assets are immutable; the app document is `no-cache`
without a session (authenticated responses are `no-store`).

Retired UI and fragment URLs have no compatibility handlers. `/` returns
404 with a link to `/app`, without redirecting.
