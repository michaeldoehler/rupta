# Alerts and notifications

Alert rules belong to a project and decide *when* to notify; notification
channels belong to an organization and decide *where*. Admins manage both.

## Channels

`/api/v1/orgs/{org}/channels`

| Kind | Configuration | Delivery |
| --- | --- | --- |
| `email` | `{"to": ["ops@example.com", ...]}` (1–50 addresses) | One message per recipient over SMTP (`RUPTA_SMTP_URL`) or a Camelmailer server (`RUPTA_CAMELMAILER_API_KEY`), from `RUPTA_MAIL_FROM`, plain text and HTML |
| `slack` | `{"webhook_url": "https://hooks.slack.com/services/..."}` | Slack incoming webhook, message with blocks |
| `teams` | `{"webhook_url": "https://..."}` | Microsoft Teams webhook (Workflows "post to a channel when a webhook request is received", or a legacy connector), Adaptive Card |
| `webhook` | `{"url": "https://...", "secret": "..."}` | JSON `POST`, signed when a secret is set |
| `pagerduty` | `{"routing_key": "...", "region": "us"}` | PagerDuty Events API v2: a `trigger` event per firing, `resolve` when the issue is resolved ([below](#pagerduty)) |
| `opsgenie` | `{"api_key": "...", "region": "eu"}` | Opsgenie Alert API v2: an alert per issue, closed when the issue is resolved ([below](#opsgenie)) |

`POST /api/v1/orgs/{org}/channels/{id}/test` sends a test notification and
answers `502 delivery_failed` with the receiver's error if it fails.

The webhook secret, the PagerDuty routing key and the Opsgenie API key are
stored encrypted and never returned (`has_secret: true`). Leave them out
when changing a channel to keep them; send `""` to remove a webhook
secret (PagerDuty and Opsgenie keys are required). Webhook, Slack and Teams URLs are returned as stored,
to admins only.

SMTP URL forms: `smtp://host:25` (plain), `smtp://user:pass@host:587?tls=required`
(STARTTLS), `smtps://user:pass@host:465` (TLS).

### Webhook payload

```json
{
  "action": "triggered",
  "trigger": "first_seen",
  "rule": {"id": "12", "name": "New errors"},
  "organization": "acme",
  "project": {"slug": "web", "name": "Web"},
  "issue": {"id": "1203", "short_id": "WEB-41", "title": "TypeError: x is undefined",
            "culprit": "app.js in render", "level": "error", "times_seen": 1,
            "url": "https://errors.example.com/organizations/acme/issues/1203"},
  "environment": "production",
  "triggered_at": "2026-11-14T12:00:00Z"
}
```

Headers: `X-Rupta-Timestamp` (Unix seconds) and, with a secret,
`X-Rupta-Signature: sha256=<hex>` where the hex is HMAC-SHA256 with the
secret over `{timestamp}.{raw body}`. Verify it, and reject old timestamps,
to make sure a request comes from rupta:

```python
import hmac, hashlib, time
def valid(secret, timestamp, body, signature):
    if abs(time.time() - int(timestamp)) > 300:
        return False
    mac = hmac.new(secret.encode(), f"{timestamp}.".encode() + body, hashlib.sha256).hexdigest()
    return hmac.compare_digest("sha256=" + mac, signature)
```

Any `2xx` answer counts as delivered. There are no retries: a failed
delivery is logged and counted in `rupta_notification_failures_total`.

### PagerDuty

Create a service integration of type *Events API v2* in PagerDuty
(service → *Integrations* → *Add integration* → *Events API V2*) and use
its **integration key** as `routing_key`. `region` is `us` (default,
`https://events.pagerduty.com/v2/enqueue`) or `eu`
(`https://events.eu.pagerduty.com/v2/enqueue`); `api_url` replaces the
endpoint (for proxies).

When a rule fires, rupta sends a `trigger` event with `dedup_key` = the
issue id, so all firings of one issue belong to one incident;
`payload.summary` is the headline, `source` the project slug, `severity`
follows the event level (`fatal` → `critical`, `error` → `error`,
`warning` → `warning`, others → `info`), `component` the culprit, `group`
the environment, `class` the trigger, `custom_details` short id, project,
level, event count and rule, and `links` the issue URL. When the issue is
resolved in rupta (by a person, or by a linked ticket), rupta sends a
`resolve` event with the same key. When a rule fires again later (for
example on `regression`), the new trigger opens the incident again. The channel test triggers an incident with a
test key and resolves it immediately.

### Opsgenie

Create an *API* integration in Opsgenie (team → *Integrations* → *Add
integration* → *API*) and use its **API key** as `api_key` (sent as
`Authorization: GenieKey ...`). `region` is `us` (`https://api.opsgenie.com`)
or `eu` (`https://api.eu.opsgenie.com`); `api_url` replaces the base URL.

When a rule fires, rupta creates an alert (`POST /v2/alerts`) with
`alias` = the issue id (Opsgenie de-duplicates open alerts by alias),
`message` the headline (130 characters at most), `description` the text
version, `priority` from the level (`fatal` P1, `error` P2, `warning` P3,
`info` P4, `debug` P5), `source: rupta`, the project as `entity`, tags
`rupta`, project, level and environment, and details. Resolving the issue
closes the alert (`POST /v2/alerts/{alias}/close?identifierType=alias`).

PagerDuty and Opsgenie requests go through the client used for issue
trackers ([integrations](integrations.md#requests-to-trackers)): `429` and
`503` are retried with backoff or `Retry-After`, and a custom `api_url` on
an internal address needs `RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS=true`.
Incidents opened for an issue are recorded (`channel_incidents`); a
resolve that fails is logged, counted and tried again the next time the
issue is resolved. Deleting the channel forgets its incidents without
resolving them.

## Rules

`/api/v1/projects/{org}/{project}/alert-rules`

```json
{
  "name": "New or regressed errors in production",
  "enabled": true,
  "conditions": [
    {"type": "first_seen"},
    {"type": "regression"},
    {"type": "event_frequency", "count": 100, "interval": "1h"}
  ],
  "filters": [
    {"type": "level", "level": "error", "match": "gte"},
    {"type": "tag", "key": "region", "value": "eu-west"}
  ],
  "environment": "production",
  "actions": [
    {"type": "channel", "channel_id": "3"},
    {"type": "email", "user_ids": ["7"], "team_ids": ["2"]},
    {"type": "create_ticket", "integration_id": "4", "labels": ["production"]}
  ],
  "frequency_minutes": 30
}
```

A rule fires for an issue when **any** condition holds and **all** filters
match:

| Condition | Holds when |
| --- | --- |
| `first_seen` | the issue was just created |
| `regression` | an event arrived for a resolved issue |
| `reappeared` | an ignored issue's condition (time, event count or user count) was met |
| `escalating` | the issue became `escalating`: far more events in the last hour than usual (see [ingestion](ingestion.md#escalating-issues)) |
| `event_frequency` `{count, interval}` | the issue has at least `count` events within `interval` (`1m` to `30d`) |
| `user_frequency` `{count, interval}` | the issue's events within `interval` come from at least `count` distinct users |

| Filter | Matches when |
| --- | --- |
| `level` `{level, match}` | the event level is `eq` to, or `gte` (default) the given level; order `debug < info < warning < error < fatal` |
| `environment` `{value}` | the event's environment equals `value` |
| `tag` `{key, value}` | the latest event has this tag value |

`environment` on the rule itself works like an environment filter.

### Actions

`channel` sends through a channel of the same organization;
`email` sends to the listed users and to the members of the listed teams,
as long as they are members of the organization, not disabled, and have
not turned off `notifications.issue_alerts` in their preferences
(`PATCH /api/v1/user`). Channels are not affected by user preferences.

`create_ticket` `{integration_id, target?, issue_type?, project?, labels?}`
creates a ticket in an issue tracker ([integrations](integrations.md)) the
first time the rule fires for an issue; later firings, and issues that
already have a ticket of that integration (created or linked by hand),
create none. Fields left out come from the project's defaults for the
integration; a rule without `target` is refused when the project has no
defaults. The ticket appears in the issue's links and activity, created
by nobody (`created_by: null`). Failures are logged (warning) and stored
as the integration's `last_error`.

Workflow emails are separate from rules: when SMTP is configured, users
get an email when an issue is assigned to them or to one of their teams,
and when someone comments on an issue assigned to them (or their team).
The person who made the change gets none. Each user can turn these off
with `notifications.workflow`.

`frequency_minutes` (1–43200, default 30) limits notifications to one per
rule and issue within that many minutes. Fires are recorded in
`alert_fires` and kept for 31 days; `GET
/api/v1/projects/{org}/{project}/alert-fires` lists them, newest first
(`?rule=` for one rule).

Notifications link to `{RUPTA_PUBLIC_URL}/organizations/{org}/issues/{id}`.

## How evaluation works

Ingestion publishes a signal for every new issue, regression, reappearance
and batch of events of an existing issue, and the escalation check (every
five minutes) one for every issue that starts escalating. The alert engine,
which runs in the server process, receives them and, per
signal, loads the project's enabled rules, checks conditions (frequency
conditions count rows in the `events` table) and filters, records the fire
and delivers notifications, at most 16 at a time. Ignored issues never
notify.

Limits:

- Signals live in memory. If the engine falls behind by more signals than
  the channel holds, the oldest are skipped (logged as a warning), and
  signals of events ingested while the server was down are not replayed.
- Frequency conditions are evaluated when events arrive, not on a timer: a
  rule cannot fire because events *stopped*.
- Environment and tag filters look at the environment reported with the
  signal or, failing that, at the issue's latest stored event.
- Deliveries are not queued across restarts. Email, Slack, Teams and
  webhook deliveries are not retried; PagerDuty, Opsgenie and
  `create_ticket` retry `429`/`503` up to three times.
- Webhook, Slack and Teams URLs and custom PagerDuty/Opsgenie API URLs on
  loopback, private or link-local addresses are refused unless the server
  runs with `RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS=true`
  ([security](security.md#outbound-requests)).

Counters on `/metrics`: `rupta_alerts_fired_total`,
`rupta_notifications_sent_total`, `rupta_notification_failures_total`,
and per channel kind and result `rupta_alert_notifications_total` (see
[monitoring](monitoring.md)).
