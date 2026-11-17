# Notifications

Channels: generic webhook, Slack, Telegram. Rules: container (stopped, exit
non-zero, unhealthy, restart loop), HTTP check, host thresholds, widget values,
log lines (`log_match`, see [logs.md](logs.md)).
Dedupe (one message per incident), `for` flap guard, cooldown, recovery
message, retries with a persisted queue (three attempts in a row, then once
a minute; given up after 6 hours; at most 200 waiting), channel health on the
page and in Settings → Notifications (with **Send test**).

In Settings → Notifications, channels and rules are lists with an add / edit
dialog each (see [settings.md](settings.md)):

- **Channel:** Name, Type (Webhook, Slack, Telegram) and where the secret
  comes from: an environment variable (its name), a file in `data/secrets/`
  (its path), or, for a webhook only, a plain URL stored in the file (with a
  warning). Telegram adds the chat id; Timeout is under "More options". Values
  set in the file that the form does not handle (`chat_id_env`, `api_base`,
  headers, a body template) are listed read-only by name and kept on save;
  their values never reach the page. **Save and send test** saves, then sends
  a test to the saved channel and shows the result in the dialog. Each row
  has a one-click **Send test** too.
- **Rule:** the type chooses its condition fields; a new rule notifies every
  channel (untick to exclude); For, Cooldown and the messages are under "More
  options". The row's switch turns a rule on or off at once.
- Renaming a channel that rules send to asks first and then renames it in
  those rules too; deleting one that a rule uses says which rule.

Server behaviour behind this (`/api/settings/channels`, `/api/settings/alerts`):

- **Delete refused while used:** deleting a channel that a rule's
  `channels` still names answers HTTP 422 with a plain message ("The channel
  “x” is used by the rule “y”. …") and `used_by` (the rule names); nothing is
  written. Before, the save failed on the validator's `unknown channel` text.
- **Rename follows:** renaming a channel changes it in every rule's
  `channels` in the same revision (as before; the dialog now asks first).
- **An edit is a patch:** an edited channel or rule keeps every key the
  request leaves out; `null` removes one. So the form never sends header
  values, a body, `api_base` or `chat_id_env`, and they stay as they are.

Secrets live in `secrets.env` next to the compose file or in files under
`data/secrets/`; the config and the app only hold environment variable
**names** and file paths (Settings shows set / not set or file found / no
file, never the value). From the app you can add Slack, Telegram (default API
host) and webhook channels whose secret comes from an env var or a file, and
webhooks with a plain URL; a custom Telegram `api_base`, `chat_id_env`, webhook
headers or a body template can only be set in the file. Fired / resolved /
failed deliveries and test sends are in the history database and on the
Alerts page.

**On the Alerts page.** Firing incidents and the alert log link to their
subject (a container's page, the Host page, the widget on the Overview, or
the checked URL), name the rule that fired and show a clock time; rows about
a container have its logs from that moment. The log is the newest 50 alert
events. An incident held back by the cooldown is marked "cooldown" (it is
sent if it is still open when the cooldown ends). Signed in, the page also carries
the Notifications tab's channel and rule lists with their dialogs: "Add
channel", "Add rule", "Send test" per channel, the rule on/off switches,
edit and delete; a firing incident's rule name opens that rule's dialog and
a "Not delivered" row in the log opens its channel's dialog.
They are the same lists, dialogs and script as in Settings
(read from `/settings/notifications?frag=list`, saved through the same
routes and guards); `/alerts?add=channel`, `?edit=rule:<name>` and so on open
the dialog, or show the form without JavaScript. Signed out the page shows
only what it always did: rule and channel names, kinds, what a rule watches
and delivery state, never env-variable names, URLs of channels or form
values. The read-only rules table states thresholds with their unit
("memory above 90%") and the timing ("fires after 30 s · repeats after
15 min"). Delivery counts are notifications, not attempts: "Undelivered" is
what waits for a retry ("1 queued") or was given up, with the failed
attempts behind it on a second line; the summary reads "3 sent · 1
undelivered (29 failed attempts)". The alert log does not repeat a value the
state already says ("47.4% > 1%", not "47.4% > 1% (47.4)").

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

Alternatively use files: `token_file = "/data/secrets/telegram"` (the file's
content, trimmed; keep it to one line). Then check each channel without waiting for an incident:

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
# timeout = "10s"                # any channel type: 1s..60s (default 10s)

[channels.slack]
type = "slack"                    # incoming webhook, sends {"text": message}
webhook_url_env = "RH_SLACK_WEBHOOK_URL"   # or webhook_url_file

[channels.telegram]
type = "telegram"                 # plain text, no parse_mode; cut at 4,000 characters
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
for = "30s"                       # must hold this long before firing (default 30s for containers, else 0; at most 1d)
cooldown = "15m"                  # min time between two notified firings of rule+subject (default 15m; at most 7d)
channels = ["ops-webhook", "slack", "telegram"]   # empty: shown on the page only
# message = "🔴 {{subject}}: {{state}} ({{rule}} on {{host}}, {{time}})"
# recovery = "✅ {{subject}}: {{state}} after {{duration}} ({{rule}} on {{host}}, {{time}})"
# recovery = ""                   # no recovery messages
# enabled = false

[[alerts]]
name = "shop-up"
type = "http"
url = "http://192.168.1.10:8070/health"
interval = "60s"                  # default 60s, at least 5s
timeout = "5s"                    # default 5s; at least 1s, not longer than interval
expect_status = 200               # default: any 2xx/3xx (redirects are not followed)
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
# interval = "30s"                # default 30s, at least 5s
channels = ["telegram"]

[[alerts]]
name = "mail-backlog"
type = "widget"
widget = "mailpit"                # a [[widgets]] id
field = "unread"                  # key in the widget's extracted values
above = 1000                      # or below =, equals =, not_equals =
# on_error = true                 # also alert when the widget fails
channels = ["ops-webhook"]

[[alerts]]
name = "app-errors"
type = "log_match"                # lines in container logs, see logs.md
pattern = '(?i)\b(panic|fatal)\b'
containers = ["shop-*"]           # name globs, and/or stack = "shop"; exclude = [...]
count = 3                         # matching lines (default 1) ...
window = "5m"                     # ... within this window (10s..1d, default 5m)
channels = ["telegram"]
```

A key that does not belong to the rule's or the channel's `type` is a config
error, not ignored.

Template variables: `{{rule}}`, `{{subject}}` (container name, URL,
"disk /", "memory", "cpu", "temperature", widget id), `{{state}}`
("stopped (exit 137)", "unhealthy", "down: HTTP 502", "92.1% > 90%",
"3 matching lines in 5m", "recovered", "removed"), `{{value}}`, `{{time}}` (UTC ISO 8601),
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
