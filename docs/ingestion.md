# Ingestion

How events get from an SDK into the database: the pipeline, durability,
backpressure and rate limits, how issues are updated, and retention.
Which endpoints and item types exist is described in
[SDK compatibility](sdk-compatibility.md).

## Pipeline

```text
HTTP handler ──try_send──► bounded queue ──► workers ──► batch writer ──► squad
 key check, limits,         RUPTA_INGEST_     normalize,    one transaction
 decompress, parse          QUEUE envelopes   filter,       per 50 ms or
                            (429 when full)   symbolicate,  1,000 events
                                              scrub, group
```

1. **HTTP handler** (async, never blocks on the database): authenticates
   the DSN key against an in-memory cache of keys and projects (reloaded
   when the configuration changes, and at most once a second when an
   unknown key appears), checks the origin, size and rate limits,
   decompresses, parses the envelope, assigns an event id when the SDK sent
   none, and puts the envelope on the queue.
2. **Workers** (`RUPTA_INGEST_WORKERS` threads, default one per core) take
   whole envelopes and handle every item (one dispatch point per item
   type): events are normalized, checked against the project's inbound
   filters, symbolicated, scrubbed, grouped, assigned to an issue (from a
   shared cache of fingerprint → issue; new issues are inserted right away
   with the project's next short id), serialized and compressed (zstd);
   attachments are written to the object store. The result is rows for the
   writer.
3. **Batch writer** collects processed envelopes for up to 50 ms or 1,000
   events (earlier when everything in flight has been collected) and writes
   them in one transaction: `events` (columnar append), `event_payloads`,
   `transactions`, `spans`, `logs` and `metrics` (columnar appends),
   profile and replay rows, issue counters and state changes, releases, session
   counts, outcomes, feedback, attachment rows and check-ins. After the
   commit it publishes issue signals (`Ingest::subscribe`, used by the
   alert engine): `New`, `Regressed`, `Reappeared` and an aggregated `Seen`
   per issue and batch. Session users go to `session_users` (see [SDK
   compatibility](sdk-compatibility.md#sessions)). `Escalating` comes from
   the escalation check (below).

An event id that is already stored (an SDK retrying) is skipped.

## Durability

`RUPTA_DURABILITY` decides when the SDK gets its answer:

| Mode | Acknowledged | If the process dies | Cost |
| --- | --- | --- | --- |
| `fast` (default) | when the envelope is queued | envelopes still in the queue or the current batch (up to `RUPTA_INGEST_QUEUE` envelopes) are lost; the SDK will not resend them | none |
| `safe` | after the batch is committed | nothing acknowledged is lost | each request waits for the next commit (one fsync, plus the upload to the object store with `RUPTA_REMOTE_SYNC=sync`) |

On a regular shutdown (SIGTERM) the server stops accepting events and
writes everything queued before it exits (bounded by
`RUPTA_SHUTDOWN_TIMEOUT_SECS`), in both modes.

## Rate limits and backpressure

| Limit | Applies to | Answer |
| --- | --- | --- |
| Project `rate_limit_per_min` (project settings) | events, CSP reports and transactions per minute | `429`, `Retry-After: <seconds to the next minute>`, `X-Sentry-Rate-Limits: <seconds>:error;default;security;transaction:project` |
| Key `rate_limit_per_min` (per DSN key) | the same | the same with scope `key` |
| Queue full | everything | `429`, `Retry-After: 1`, `X-Sentry-Rate-Limits: 1::organization` |

Limits count in fixed one-minute windows, in memory (a restart resets
them). Sessions, client reports, feedback and check-ins are not limited.
An envelope that hits a limit is rejected as a whole. The SDKs honor
`X-Sentry-Rate-Limits` and stop sending the named categories for the given
time, recording what they dropped in later client reports.

The queue holds `RUPTA_INGEST_QUEUE` envelopes (default 100,000; at about
5 KB per event up to roughly 500 MB of memory). HTTP handlers never wait
for space: a full queue is answered immediately with 429. Workers wait when
the writer falls behind, so a slow disk fills the queue and turns into 429s
rather than into memory growth.

## Outcomes

Every item is counted in `outcomes` (per project, hour, data category,
outcome and reason) and in the `rupta_ingest_events_total{category,outcome}`
metric:

| Outcome | Meaning |
| --- | --- |
| `accepted` | stored |
| `filtered` | dropped by an inbound filter (reason: `browser-extensions`, `localhost`, `legacy-browsers`, `web-crawlers`, `error-message`, `release-version`; attachments of filtered events: `event_filtered`) |
| `rate_limited` | rejected by a rate limit (reason `project` or `key`) or a full queue (reason `queue_full`; metric outcome `queue_full`) |
| `invalid` | malformed or too large (reason: `invalid_json`, `too_large`, `invalid_session`, `invalid_check_in`, `missing_event_id`, ...) |
| `dropped_unsupported` | acknowledged but not stored (`replay_video`, `metric_meta`, `otel_span`, CSP reports of other types, unknown items) |
| `client_discard` | dropped by the SDK itself, from client reports, with the SDK's reason |

Requests rejected before the project is known (bad key, malformed body)
are not counted.

## Issues

- **New issue:** created by the first event with a new fingerprint, status
  `unresolved`, substatus `new`, short id from `projects.next_short_id`;
  `first_release` is the event's release, and the release's `new_issues`
  counter in `release_projects` goes up.
- **Counters:** per batch, `times_seen` grows by the issue's events,
  `first_seen`/`last_seen` widen, `last_release` follows the newest event.
- **Regression:** an event for a `resolved` issue sets it to `unresolved` /
  `regressed`, clears the resolution, adds `set_regression` to the issue's
  activity and publishes `Regressed`. Which events count depends on how the
  issue was resolved ("first seen" of a release is `releases.created_at`,
  the time rupta first saw it from an event, a session or the upload CLI; a
  release seen for the first time in the same batch counts as first seen
  now):
  - resolved without a release: every event;
  - resolved in a release `V` (`resolved_in_release`, `resolved_in_mode`
    `release`): events of `V` itself, of releases first seen after `V`, and
    events without a release. Events of releases first seen before `V`
    keep it resolved. While `V` has not been seen at all, only events of
    `V` regress the issue;
  - resolved in the next release (`resolved_in_mode` `next_release`; the
    API records the project's latest release at that moment as
    `resolved_in_release`): only events of a release other than the
    recorded one that was first seen after the resolution. Events of the
    recorded release, of older releases and events without a release keep
    it resolved.
- **Ignored issues** reappear (`unresolved` / `ongoing`, activity
  `set_unresolved` with the reason, signal `Reappeared`) when
  `ignore_until` has passed, `times_seen` reaches `ignore_count` (an
  absolute threshold the API sets to `times_seen` at ignore time plus N),
  or the user condition is met: without a window `users_seen` reaches
  `ignore_user_count` (absolute, `users_seen` at ignore time plus N); with
  `ignore_user_window` the issue's events of the last that many minutes
  come from at least `ignore_user_count` distinct users (counted in
  `events` when an event arrives). Issues ignored forever stay ignored;
  issues ignored with substatus `archived_until_escalating` come back when
  they escalate (below).
- **Users:** `users_seen` counts distinct user identities (id, else e-mail,
  else username, else IP address, hashed). The writer keeps up to 10,000
  user hashes per issue (for up to 50,000 issues) in memory and loads them
  from `events` after a restart. Beyond 10,000 users per issue new users
  are no longer counted, so `users_seen` is a lower bound there; exact
  numbers can be computed from `events.user_hash`.
- **Feedback** is linked to the issue of the event it refers to; feedback
  that arrives before its event is linked when the event is written (while
  the process runs).

Releases are created on first sight (from events or sessions) with their
first and last event times.

## Escalating issues

A background check (every five minutes, `rupta_ingest::escalation`) sets
substatuses that depend on time rather than on a single event:

- **Escalating:** an unresolved issue with substatus `ongoing`, or an
  issue ignored with substatus `archived_until_escalating`, becomes
  `unresolved` / `escalating` when its events in the last hour are at
  least 50 and more than 10 times its hourly average over the 7 days
  before that hour (events in those 168 hours / 168; younger issues are
  averaged over the full 168 hours too). The average is stored in
  `escalation_baseline`, the time in `escalated_at`, activity
  `set_escalating` (with `events_last_hour`, `hourly_average`) is recorded
  and the signal `Escalating` goes to the alert engine (rule condition
  `escalating`).
- **Back to ongoing:** an escalating issue that escalated at least 24
  hours ago returns to `ongoing` (activity `set_ongoing`) when each hour of
  the last 24 had fewer than twice the baseline events (and at least: no
  events). Issues set to `escalating` through the API have no baseline and
  return after a day without events.
- **New to ongoing:** issues stay `new` for 7 days after their first
  event; the check then moves them to `ongoing`. Only `ongoing` issues can
  escalate, so an issue's first week never escalates (its first hours
  would always look like a spike).

Counts use the events' own timestamps, as the issue statistics do, and
the `events` table, so events removed by retention or deleted with their
issue no longer count. The check runs in the server process; when it was
not running (for example during a restart), the next run looks at the
last hour only.

## Throughput

Measured with `cargo run --release -p rupta-ingest --example ingest_bench
-- 100000 64`: the ingest router on a local port with an on-disk database,
64 concurrent HTTP clients in the same process, realistic JavaScript error
events of about 5 KB (12 frames, 10 breadcrumbs, contexts, request, user,
tags) over 50 issues. Machine: 4 vCPU, shared with two other build jobs
(load average around 3.5 before the run), so the numbers are noisy. Raw
output: `benchmarks/ingest-2026-11-14.txt`.

| Durability | Acknowledged | Stored (until everything was committed) | Ack latency p50 / p99 |
| --- | --- | --- | --- |
| `fast` | 39,600 events/s | 10,300 events/s | 1.3 ms / 9.3 ms |
| `safe` | 4,100 events/s | 4,100 events/s | 13.4 ms / 22.7 ms |

In `fast` mode the HTTP side accepts about 40,000 events per second; what
the server can store continuously on this machine is about 10,000 per
second, and a burst larger than the queue turns into 429s at that point.
Per event on one core: JSON parsing 27 µs, normalization 19 µs, scrubbing
12 µs, grouping 3 µs, serialization 14 µs, compression 12 µs; the batch
writer needs about 25 ms per 1,000 events in isolation and about three
times as much while sharing the cores with workers, HTTP and the database's
background compaction. The 20,000 stored events per second aimed for were
not reached on this machine. In `safe` mode throughput is bound by commits
(one fsync per batch) and the number of concurrent clients.

`RUPTA_INGEST_WORKERS` adds processing threads; the writer is a single
thread by default (`IngestConfig::writers`; two writers were not faster on
four cores).

## Retention

`rupta_ingest::retention::spawn` runs five minutes after start and then
every 24 hours. For each project it deletes data older than the project's
`retention_days` or `RUPTA_RETENTION_DAYS` (default 90):

- `events` (columnar), one `DELETE` per day from the oldest event up to
  the cutoff, so each statement rewrites a bounded part of the segments;
- `event_payloads` (by `day`), `attachments` (objects first, then rows; an
  object that cannot be deleted keeps its row and is retried next run),
  `session_counts`, `outcomes`, check-ins;
- issues whose `last_seen` is before the cutoff, with their activity.

Telemetry has its own retention per project (`settings.retention`, in
days): `spans` for transactions and spans (default 30), `logs` (30),
`metrics` (90), `profiles` (30) and `replays` (30). Columnar tables are
deleted a day at a time like events; profile and replay documents are
deleted by day prefix in the object store (`profiles/{project}/{day}/`,
`replays/{project}/{day}/`) before their rows. Transactions follow the
`spans` retention, not `retention_days`.

A columnar `DELETE` rewrites every segment that contains matching rows.
Measured (`cargo test --release -p rupta-ingest --test retention --
--ignored --nocapture`): in a table of 500,000 events over 10 days, after
compaction, deleting the oldest day (50,000 events) took 1.7 s and a day in
the middle 0.9 s, on the same shared machine. Ingestion continues during
retention; deletes and compaction take turns on the table's segment lock.

## Metrics

Appended to `/metrics`:

| Metric | Type |
| --- | --- |
| `rupta_ingest_queue_depth`, `rupta_ingest_queue_capacity` | gauge |
| `rupta_ingest_events_total{category,outcome}` | counter |
| `rupta_ingest_batches_total`, `rupta_ingest_batch_errors_total` | counter |
| `rupta_ingest_batch_seconds` | histogram of batch write and commit time |

## Limits

- One server writes the database; ingestion does not scale out to several
  processes.
- Rate limits and session transition state are in memory.
- A batch that fails to commit (other than for duplicate event ids or
  serialization conflicts, which are retried) is dropped and logged; with
  durability `safe` the SDKs receive 503 and retry.
