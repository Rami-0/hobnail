# Notifications

Channels: generic webhook, Slack, Telegram. Rules: container (stopped, exit
non-zero, unhealthy, restart loop), HTTP check, host thresholds, widget values.
Dedupe (one message per incident), `for` flap guard, cooldown, recovery
message, retries with a persisted queue, channel health on the page and in
Settings → Notifications (with **Send test**).

Secrets live in `secrets.env` next to the compose file; the config and the app
only hold environment variable **names** (Settings shows set / not set, never
the value). From the app you can add Slack, Telegram (default API host) and
webhook channels whose URL comes from an env var; a custom Telegram `api_base`
or webhook secret headers can only be set in the file (bound to their
destination). Fired / resolved / failed deliveries are in the history database
and on the Alerts page.

## Where to put real tokens

Secrets never go in `config.toml`. Put them in **`secrets.env`** next to the
compose file (mode 600, read by compose via `env_file`), then apply with
`docker compose up -d` (a restart is needed for env changes; config changes are
hot-reloaded):

```sh
# secrets.env
RH_SLACK_WEBHOOK_URL=https://hooks.slack.com/services/T000/B000/XXXXXXXX
RH_TELEGRAM_TOKEN=123456789:AA...your-bot-token
RH_WEBHOOK_URL=https://ntfy.sh/your-topic          # if the URL itself is secret
```

and reference them by name in `config.toml`:

```toml
[channels.slack]
type = "slack"
webhook_url_env = "RH_SLACK_WEBHOOK_URL"

[channels.telegram]
type = "telegram"
token_env = "RH_TELEGRAM_TOKEN"
chat_id = "123456789"        # your chat id (not a secret); or chat_id_env
```

Alternatively use files: `token_file = "/data/secrets/telegram"` (first line,
trimmed). Then check each channel without waiting for an incident:

```sh
docker exec hobnail /hobnail notify-test telegram
```

## Add a channel / rule (reference)

```toml
# ---- channels: named destinations ------------------------------------
[channels.ops-webhook]
type = "webhook"
url_env = "RH_WEBHOOK_URL"        # or url_file = "/data/secrets/webhook",
                                  # or url = "http://..." (only for URLs that are not secret)
# headers = { "X-Source" = "hobnail" }
# headers_env = { Authorization = "RH_WEBHOOK_AUTH" }   # header value from an env var
# body = '{"title":"{{title}}","text":"{{message}}"}'  # JSON template, values JSON-escaped.
#   Default body: {"kind","title","message","rule","subject","state","value","time","host"}
# timeout = "10s"

[channels.slack]
type = "slack"                    # incoming webhook, sends {"text": message}
webhook_url_env = "RH_SLACK_WEBHOOK_URL"   # or webhook_url_file

[channels.telegram]
type = "telegram"                 # plain text, no parse_mode
token_env = "RH_TELEGRAM_TOKEN"   # or token_file
chat_id = "123456"                # number or "@channel"; or chat_id_env
# api_base = "https://api.telegram.org"

# ---- rules -------------------------------------------------------------
[[alerts]]
name = "container-down"
type = "container"
containers = ["*"]                # globs on the container name (default all)
exclude = ["*-e2e-*"]
on = ["stopped", "exit_nonzero", "unhealthy", "restarting"]  # default: stopped, unhealthy, restarting
restarts = 3                      # "restarting" = at least this many dies ...
window = "10m"                    # ... within this window (1m..1h)
for = "30s"                       # must hold this long before firing (default 30s for containers, else 0)
cooldown = "15m"                  # min time between two notified firings of rule+subject (default 15m)
channels = ["ops-webhook", "slack", "telegram"]   # empty: shown on the page only
# message = "🔴 {{subject}}: {{state}} ({{rule}} on {{host}}, {{time}})"
# recovery = "✅ {{subject}}: {{state}} after {{duration}} ({{rule}} on {{host}}, {{time}})"
# recovery = ""                   # no recovery messages
# enabled = false

[[alerts]]
name = "shop-up"
type = "http"
url = "http://192.168.1.10:8070/health"
interval = "60s"
timeout = "5s"
expect_status = 200               # default: any 2xx/3xx
# contains = "pass"               # body must contain this text
failures = 2                      # consecutive failed checks before it counts as down (default 1)
channels = ["slack"]

[[alerts]]
name = "disk"
type = "host"
metric = "disk"                   # disk | memory | cpu (percent) | temp (°C)
# disk = "/"                      # disk label glob (default: every disk)
above = 90                        # or below =
for = "5m"
# interval = "30s"
channels = ["telegram"]

[[alerts]]
name = "mail-backlog"
type = "widget"
widget = "mailpit"                # a [[widgets]] id
field = "unread"                  # key in the widget's extracted values
above = 1000                      # or below =, equals =, not_equals =
# on_error = true                 # also alert when the widget fails
channels = ["ops-webhook"]
```

Template variables: `{{rule}}`, `{{subject}}` (container name, URL,
"disk /", "memory", "cpu", "temperature", widget id), `{{state}}`
("stopped (exit 137)", "unhealthy", "down: HTTP 502", "92.1% > 90%",
"recovered", "removed"), `{{value}}`, `{{time}}` (UTC ISO 8601),
`{{host}}`, `{{duration}}` (how long it was bad), `{{kind}}`
(firing / recovery / test), `{{title}}` and, in webhook bodies,
`{{message}}`.

Secrets come only from environment variables or files and are never
logged or rendered. Channels obey `[http] allow_hosts`. State lives in
`{data_dir}/alerts-state.json`. Test a channel with
`hobnail notify-test <channel>`.

## Testing without real services

`hobnail mock-receiver` is a local fake for all three channel types. The
compose file has it under the `test` profile:

```sh
docker compose --profile test up -d mock
curl -s http://127.0.0.1:3299/_received     # what it got (Telegram tokens redacted)
curl -s http://127.0.0.1:3299/_clear
```

Point channels at it with `url = "http://hobnail-mock:9999/webhook"`,
`RH_SLACK_WEBHOOK_URL=http://hobnail-mock:9999/slack/x` and
`api_base = "http://hobnail-mock:9999"` for Telegram. Any path containing
`/fail` returns HTTP 500, to see a failing channel on the page.
