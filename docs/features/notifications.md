# Notifications

Channels: generic webhook, Slack, Telegram. Rules: container (stopped, exit
non-zero, unhealthy, restart loop), HTTP check, host thresholds, widget values.
Dedupe (one message per incident), `for` flap guard, cooldown, recovery
message, retries with a persisted queue, channel health on the page and in
Settings → Notifications (with **Send test**).

Secrets live in `/opt/stacks/hobnail/secrets.env`; the config and the app
only hold environment variable **names** (Settings shows set / not set, never
the value). From the app you can add Slack, Telegram (default API host) and
webhook channels whose URL comes from an env var; a custom Telegram `api_base`
or webhook secret headers can only be set in the file (bound to their
destination). Fired / resolved / failed deliveries are in the history database
and on the Alerts page.

Reference: README "Notifications".
