# Monitoring rupta

rupta exposes Prometheus metrics at `GET /metrics` on its normal port.
The repository ships a scrape configuration, alert rules with unit tests,
a Grafana dashboard and a compose overlay that runs Prometheus and Grafana
next to rupta.

## Quick start with Docker Compose

```sh
scripts/generate-env.sh          # also sets RUPTA_METRICS_TOKEN and GRAFANA_ADMIN_PASSWORD
docker compose -f compose.yaml -f compose.monitoring.yaml up -d
```

- Grafana: <http://localhost:3000>, user `admin`, password
  `GRAFANA_ADMIN_PASSWORD` from `.env`. The dashboard *rupta overview* is
  in the folder *rupta*.
- Prometheus: <http://localhost:9090> (bound to localhost only), with the
  rules of `deploy/prometheus/rupta-alerts.yml` loaded. Alerts are
  evaluated but go nowhere until you add an Alertmanager
  (`alerting:` in `deploy/prometheus/prometheus.yml`).

The overlay pins `prom/prometheus:v3.16.0` and `grafana/grafana:13.3.1`,
passes `RUPTA_METRICS_TOKEN` to rupta and the same value to Prometheus as
a file (a compose secret), and installs Grafana's Infinity data source for
[annotations](#annotations-in-grafana).

## Prometheus

`deploy/prometheus/prometheus.yml` scrapes `rupta:8080/metrics` every 30
seconds. With `RUPTA_METRICS_TOKEN` set, `/metrics` answers `401` without
`Authorization: Bearer <token>`; the configuration reads the token from
`rupta-metrics-token` next to it (`authorization.credentials_file`). In
another setup, keep the `job_name: rupta` (the rules use `up{job="rupta"}`)
and adjust the target:

```yaml
scrape_configs:
  - job_name: rupta
    authorization:
      type: Bearer
      credentials_file: /etc/prometheus/rupta-metrics-token
    static_configs:
      - targets: ["errors.internal.example.com:8080"]
```

Check changes with `scripts/check-monitoring.sh`, which runs `promtool
check config`, `promtool check rules` and `promtool test rules
deploy/prometheus/rupta-alerts.test.yml` (it downloads promtool 3.16.0
when none is installed; CI runs it).

## Alert rules

`deploy/prometheus/rupta-alerts.yml`:

| Alert | Fires when | Severity |
| --- | --- | --- |
| `RuptaDown` | the scrape fails for 2 minutes | critical |
| `RuptaNotReady` | `rupta_ready == 0` for 2 minutes: the database does not answer or the instance is fenced | critical |
| `RuptaStorageFenced` | `rupta_storage_fenced == 1`: another instance took over the bucket | critical |
| `RuptaRemoteLagGrowing` | with an object store, `rupta_storage_unshipped_lsn` is above 0 and rising for 10 minutes | warning |
| `RuptaIngestQueueNearCapacity` | the ingest queue is more than 80% full for 5 minutes | warning |
| `RuptaHighRejectionRate` | more than 10% of envelope items are rate limited, invalid or dropped for a full queue, for 15 minutes (with at least 0.1 items/s) | warning |
| `RuptaNotificationFailures` | an alert notification failed in the last 30 minutes (per channel kind) | warning |
| `RuptaIntegrationSyncFailures` | more than 3 failed comments or transitions to a tracker in an hour | warning |
| `RuptaHighP99Latency` | the 99th percentile response time exceeds 2 seconds for 10 minutes (web app and live log tail excluded) | warning |
| `RuptaHighServerErrorRate` | more than 5% of responses are 5xx for 10 minutes | warning |

The thresholds are starting points. `RuptaHighRejectionRate` also fires
when SDKs simply send more than the project rate limits allow; raise the
limits or the threshold if that is expected.

## Grafana

`deploy/grafana/rupta-overview.json` is an importable dashboard
(*Dashboards → New → Import → Upload*). It has two variables:
`DS_PROMETHEUS` (the Prometheus data source; panels use
`${DS_PROMETHEUS}`) and `instance`. Panels:

- status: up, ready, fenced, unresolved issues, ingest queue fill, uptime;
- ingestion: items per second by outcome, error events per project (top
  10), new issues per project, issues by status, queue depth, batch
  commit time;
- HTTP: p50/p95/p99 response time, 5xx and 4xx share, requests by status,
  the ten slowest routes (p95);
- storage: object store lag, database and log size, page cache hit ratio,
  resident memory;
- notifications by channel and result, ticket sync by tracker, direction
  and result.

Provisioning example (used by the overlay):
`deploy/grafana/provisioning/datasources/rupta.yml` (Prometheus with uid
`prometheus`, and the optional Infinity data source) and
`deploy/grafana/provisioning/dashboards/rupta.yml` (loads dashboards from
`/var/lib/grafana/dashboards/rupta`).

## Annotations in Grafana

`GET /api/v1/orgs/{org}/annotations?from=&to=&project=&kind=` returns
releases, deploys and hours with many new issues as a JSON list, newest
first, at most 500:

```json
[
  {"time": 1794650400000, "time_end": 1794651000000, "kind": "deploy",
   "title": "Deploy 1.4.0 to production", "text": "1.4.0 deployed to production",
   "tags": ["deploy", "production"], "project": null,
   "url": "https://errors.example.com/organizations/acme/releases/1.4.0"},
  {"time": 1794650000000, "time_end": null, "kind": "release",
   "title": "Release 1.4.0", "text": "Release 1.4.0 (abc123)", "tags": ["release", "web"],
   "project": null, "url": "..."},
  {"time": 1794636000000, "time_end": 1794639600000, "kind": "new_issues",
   "title": "8 new issues in api", "text": "...", "tags": ["new_issues", "api"],
   "project": "api", "url": "..."}
]
```

- `from`, `to`: Unix milliseconds (what Grafana's `${__from}` and
  `${__to}` give), Unix seconds or RFC 3339; default the last 24 hours, at
  most 90 days.
- `project`: slugs or ids, comma-separated; `kind`: `release`, `deploy`,
  `new_issues`, comma-separated.
- `time` and `time_end` are Unix milliseconds; deploys with a start time
  and new-issue hours are regions.
- A **new-issue spike** is an hour of a project with at least 5 new issues
  and at least three times the project's hourly average over the requested
  range (rounded up). It is a simple rule, not anomaly detection.
- Authentication is an API token: create an organization token with the
  scope `org:read` (organization settings → API tokens).

With the [Infinity](https://grafana.com/grafana/plugins/yesoreyeram-infinity-datasource/)
data source (`yesoreyeram-infinity-datasource`, installed by the overlay):

1. Data source: authentication *Bearer token* with the rupta token, and
   `http://rupta:8080` (or your `RUPTA_PUBLIC_URL`) under *Allowed hosts*.
   The overlay provisions this as the data source *rupta* from
   `RUPTA_GRAFANA_TOKEN`.
2. Dashboard settings → *Annotations* → *New*: data source *rupta*, type
   *JSON*, source *URL*, method GET, URL
   `http://rupta:8080/api/v1/orgs/acme/annotations?from=${__from}&to=${__to}`.
3. Under *Columns* add `time` and `time_end` with the column type for
   Unix milliseconds (*Timestamp (UNIX ms)*), and `title`, `text` and
   `tags` as strings; then map the annotation fields: *Time* = `time`,
   *End time* = `time_end`, *Title* = `title`, *Text* = `text`, *Tags* =
   `tags`. Add `&project=web` to the URL to limit it to projects.

Any other JSON annotation source that can send a bearer token works the
same way.

## Metrics reference

Counters only grow while the process runs and start at 0 after a
restart. Labels are in braces.

### Requests

| Metric | Type | Description |
| --- | --- | --- |
| `rupta_http_requests_total{method, route, status}` | counter | Requests by method, route template (`/api/v1/issues/{id}`; `unmatched` for the web app and unknown paths) and status class (`2xx`, `4xx`, …). |
| `rupta_http_request_duration_seconds{method, route, status}` | histogram | Time until the response headers are sent. Buckets 5 ms to 30 s. For the live log tail (`/api/v1/orgs/{org}/logs/stream`) this is the time to open the stream. |

### Ingestion

| Metric | Type | Description |
| --- | --- | --- |
| `rupta_ingest_events_total{category, outcome}` | counter | Envelope items by data category (`error`, `transaction`, `session`, `attachment`, …) and outcome (`accepted`, `filtered`, `rate_limited`, `invalid`, `dropped_unsupported`, `queue_full`, `client_discard`). |
| `rupta_ingest_queue_depth`, `rupta_ingest_queue_capacity` | gauge | Envelopes waiting for a worker, and the queue size (`RUPTA_INGEST_QUEUE`). |
| `rupta_ingest_batches_total`, `rupta_ingest_batch_errors_total` | counter | Batches committed and batches that failed. |
| `rupta_ingest_batch_seconds` | histogram | Time to write and commit one batch. |
| `rupta_project_events_total{org, project}` | counter | Error events stored (grouped into issues) per project. |
| `rupta_project_new_issues_total{org, project}` | counter | New issues per project. |

Per-project labels use the organization and project slugs. To keep the
number of series bounded, only the first `RUPTA_METRICS_PROJECT_LABELS`
(default 50) projects that receive events after the start get their own
labels; all others are added up as `org="other", project="other"`. The
assignment never changes while the process runs, so no series jumps; after
a restart it can differ. These are not necessarily the busiest projects.
A project renamed while running keeps counting under its new slug; a
deleted one shows as `org="unknown", project="id-<id>"`.

### Issues

| Metric | Type | Description |
| --- | --- | --- |
| `rupta_issues{status}` | gauge | Issues of the whole instance per status (`unresolved`, `resolved`, `ignored`), from one `GROUP BY` query cached for 30 seconds. |

### Alerts, notifications and integrations

| Metric | Type | Description |
| --- | --- | --- |
| `rupta_alerts_fired_total` | counter | Alert rules that fired. |
| `rupta_alert_notifications_total{channel, result}` | counter | Deliveries by channel kind (`email`, `slack`, `teams`, `webhook`, `pagerduty`, `opsgenie`) and result (`success`, `failure`); includes the resolve events sent to PagerDuty and Opsgenie. |
| `rupta_notifications_sent_total`, `rupta_notification_failures_total` | counter | All notifications (alerts and workflow emails), kept from earlier versions. |
| `rupta_integration_requests_total{kind, operation, result}` | counter | Calls to trackers: operation `validate`, `targets`, `create`, `get`, `comment`, `transition`; result `success` or the failure reason (`network`, `blocked_host`, `unauthorized`, `not_found`, `rate_limited`, `error_response`, `invalid`). |
| `rupta_integration_webhooks_total{kind, result}` | counter | Incoming tracker webhooks: `accepted`, `duplicate`, `unauthorized`, `invalid`. |
| `rupta_integration_sync_total{kind, direction, result}` | counter | Status sync: `incoming` with `applied` (the issue changed), `echo` (within the quiet period) or `updated` (only the link changed); `outgoing` with `success` or `failure`. |

### Accounts

`rupta_logins_total`, `rupta_login_failures_total`,
`rupta_login_lockouts_total`, `rupta_sso_logins_total`,
`rupta_sso_failures_total` (counters), `rupta_artifact_bundles_total`
(source map bundles assembled).

### Storage

| Metric | Type | Description |
| --- | --- | --- |
| `rupta_ready` | gauge | 1 when the database answers and the instance is the writer (what `/readyz` checks). |
| `rupta_storage_remote_configured` | gauge | 1 with `RUPTA_STORAGE_URL`. |
| `rupta_storage_fenced` | gauge | 1 when another writer took over the bucket (0 without a bucket). |
| `rupta_storage_unshipped_lsn` | gauge | Remote lag: log records committed locally but not yet in the bucket. It is a count of log positions, not seconds. |
| `rupta_storage_local_lsn`, `rupta_storage_shipped_lsn` | gauge | The two log positions it is computed from. |
| `rupta_storage_remote_wal_bytes` | gauge | Bytes of log objects in the bucket. |
| `rupta_storage_database_bytes` | gauge | Size of the database pages (page count × 8 KiB). |
| `rupta_storage_wal_bytes` | gauge | Size of the local write-ahead log. |
| `rupta_storage_cached_pages` | gauge | Pages in the page cache (`RUPTA_CACHE_MB`). |
| `rupta_storage_cache_hits_total`, `rupta_storage_cache_misses_total` | counter | Page reads served from the cache and from disk. |
| `rupta_storage_cache_hit_ratio` | gauge | Hits ÷ reads since the start (use the counters for a recent ratio). |
| `rupta_storage_fsyncs_total`, `rupta_storage_checkpoints_total` | counter | fsync calls and checkpoints. |

### Process and build

| Metric | Type | Description |
| --- | --- | --- |
| `rupta_info{version}`, `rupta_build_info{version, os, arch}` | gauge | Always 1. |
| `rupta_uptime_seconds` | gauge | Seconds since the server started. |
| `process_start_time_seconds` | gauge | Start time (Unix seconds). |
| `process_resident_memory_bytes`, `process_virtual_memory_bytes` | gauge | From `/proc/self/status` (Linux only). |
| `process_open_fds`, `process_max_fds` | gauge | From `/proc/self/fd` and `/proc/self/limits` (Linux only). |
| `process_cpu_seconds_total` | counter | User and system CPU time (Linux only; assumes 100 clock ticks per second, the Linux default). |

## Limits

- One instance, one set of metrics: there is no aggregation across
  instances (rupta runs as a single writer).
- The latency histogram measures until the response headers; streamed
  bodies (file downloads, the live log tail) are not timed to the end.
- Route labels come from the router: requests that match no route (the
  web app's pages and assets, unknown paths) share `route="unmatched"`.
- Counters for categories and outcomes that never occurred are not
  printed; dashboards show them as no data rather than 0.
