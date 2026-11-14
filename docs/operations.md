# Operations

Running rupta day to day: monitoring, backups and restore, upgrades and
troubleshooting.

## Health and monitoring

| Endpoint | Meaning |
| --- | --- |
| `GET /healthz` | Liveness: the process answers. Always 200 while it runs. |
| `GET /readyz` | Readiness: 200 when the database answers and this instance is still the writer of its bucket; 503 otherwise. The body says which check failed and how far the bucket is behind (`local_lsn`, `shipped_lsn`). |
| `GET /metrics` | Prometheus text format. Protect it with `RUPTA_METRICS_TOKEN`. The [monitoring](monitoring.md) guide lists every metric and ships alert rules and a Grafana dashboard. |

Metrics worth alerting on:

| Metric | Alert when |
| --- | --- |
| `rupta_storage_fenced` | `== 1`: another instance took over the database; this one refuses writes. |
| `rupta_storage_unshipped_lsn` | grows for minutes: the bucket is unreachable; with `RUPTA_REMOTE_SYNC=sync` writes fail meanwhile, with `async` they queue. |
| `rupta_ingest_queue_depth` | stays near `RUPTA_INGEST_QUEUE`: processing cannot keep up; SDKs receive 429. |
| `rupta_ingest_events_total{outcome="rate_limited"}` | rises: projects or keys hit their rate limits. |
| `rupta_ready` | `== 0`: what `/readyz` reports as not ready. |
| `rupta_alert_notifications_total{result="failure"}` | rises: a channel does not deliver. |

`deploy/prometheus/rupta-alerts.yml` has these as Prometheus rules; see
[monitoring](monitoring.md#alert-rules).

Logs go to standard output, as JSON with `RUPTA_LOG_FORMAT=json` (the
default in the container image). Every request carries an `x-request-id`.

## Backups

With a bucket configured, the bucket **is** the backup of the working copy:
every write is stored there (immediately with `RUPTA_REMOTE_SYNC=sync`), and
a lost volume is restored from it automatically. What remains is protecting
the bucket itself:

- Run Garage with `replication_factor = 3` across failure domains.
- For protection against operator error or a compromised key, replicate the
  bucket to a second location with versioning, or copy it periodically
  (`rclone sync`, `aws s3 sync`) to storage the rupta key cannot write.

Without a bucket, back up the `/data` volume while rupta is stopped, or from
a filesystem snapshot. Copying the files of a running instance can produce
an inconsistent copy.

## Restore

- **Lost volume, bucket intact:** start rupta with the same
  `RUPTA_STORAGE_URL` and an empty volume. It downloads the newest snapshot
  and the log written after it, then serves as before.
- **Move to another host:** stop the old instance, start the new one with
  the same bucket settings. If both run at the same time, the newer one
  takes over and the older one is fenced.
- **From a bucket copy:** point `RUPTA_STORAGE_URL` at the copy (or copy it
  back) and start with an empty volume.

Test the restore regularly: start a second instance against a copy of the
bucket on another prefix and check that it opens.

## Upgrades

1. Read the changelog for the target version.
2. Pull the new image and redeploy. With Swarm's `stop-first` the old
   container stops (finishing queued events), the new one starts, applies
   schema migrations and serves. Expect a short gap in which SDKs retry.
3. Migrations are applied automatically and are not reversible. To roll
   back across a migration, restore the bucket from before the upgrade.

Before 1.0, minor versions (0.1 → 0.2) may change the storage format; the
changelog says when, and how to migrate.

## Data retention

Events, attachments and statistics older than `RUPTA_RETENTION_DAYS`
(default 90, or a project's own setting) are deleted by a daily job, and
so are issues last seen before that cutoff, with their activity.
Transactions and spans, logs, metrics, profiles and replays have their own
retention per project (30 days by default, metrics 90). See
[ingestion](ingestion.md#retention).

## Troubleshooting

| Symptom | Check |
| --- | --- |
| SDK events don't arrive | Is the DSN's host the `RUPTA_PUBLIC_URL`? Does the project's key exist and is it active? Browser SDKs: is the page's origin in the project's allowed origins? Look for 4xx in the rupta log. |
| Events from a script or short job don't arrive | The process exited before the SDK sent. Call `flush` (`Sentry.flush(2000)`, `sentry_sdk.flush()`, `\Sentry\flush()`) before exiting. |
| Events from another container don't arrive | `localhost` in the DSN means the application's own container. Use a host name the application can reach, and set `RUPTA_PUBLIC_URL` to it so new DSNs carry it. |
| Wrong client IP addresses on events | Behind a reverse proxy, set `RUPTA_TRUSTED_PROXIES` to the proxy's address or network (with Compose, see [which variables reach rupta](deployment.md#docker-compose-single-host)). |
| `429` responses | Project or key rate limits, or the ingest queue is full (see metrics). |
| Login loops behind a proxy | `RUPTA_PUBLIC_URL` must be the external URL; with `https://` cookies are `Secure` and need TLS at the proxy. |
| SSO error "invalid redirect" | The redirect URI at the provider must be exactly `<RUPTA_PUBLIC_URL>/auth/sso/oidc/callback`. |
| Start fails with "behind the store" or "superseded writer" | The volume belongs to an older writer; rupta replaces it from the bucket automatically. If it still fails, remove the volume. |
| `/readyz` 503 with `fenced: true` | Another instance with the same bucket prefix runs. Stop one of them. |
