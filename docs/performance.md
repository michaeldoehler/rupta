# Performance: transactions, traces and spans

rupta stores the transactions and spans that the Sentry SDKs
send when tracing is enabled (`tracesSampleRate`, `traces_sample_rate`) and
answers questions about them: which transactions are slow or failing, what
happened inside one request (the trace), and which operations take the
time (the span explorer).

## What is stored

| Source | Stored as |
| --- | --- |
| `transaction` items (PHP, Python, older JavaScript SDKs) | one row in `transactions`; its root span and every span of its `spans` array in `spans` |
| `span` items (current JavaScript SDKs stream spans in `{"items": [...]}` containers) | every span in `spans`; segment spans (the root of a local span tree) also in `transactions` |

Both tables are columnar, sorted by project and time.

- `transactions`: trace id, span id, parent span id, name, operation,
  status, start, end, duration, environment, release, platform, SDK, a
  hash of the user, span count, tags, measurements, the web vitals LCP,
  FCP, CLS, INP and TTFB as columns, and the profile links
  (`contexts.profile.profile_id`, `profiler_id`).
- `spans`: trace id, span id, parent span id, segment id (the span id of
  the transaction the span belongs to), whether it is a segment, the
  transaction name, operation, description, status, start, duration,
  environment, release, platform, SDK, `data` (span data or flattened
  span attributes, at most 16 KiB of JSON per span), tags, and web vitals
  reported on the span (`browser.web_vital.<name>.value` attributes, or a
  transaction's measurements on its root span).

Error events keep the trace and span id of their trace context in
`events.trace_id` and `events.span_id`, so a trace shows its errors.

Limits: at most 2,000 spans per transaction or span item (more are counted
as `invalid` with reason `span_limit_or_missing_id`), descriptions up to
2,000 characters, at most 100 attributes per streamed span.

## Sampling and switches

Per project (`PATCH /api/v1/projects/{org}/{project}` with `settings`):

- `sampling.transactions` (0 to 1, default 1): the share of transactions
  kept, with their spans. `sampling.spans` does the same for standalone
  `span` items. The decision is made from the trace id, so a trace is kept
  or dropped as a whole across services that share the setting. Dropped
  items are counted as outcome `filtered`, reason `sample_rate`.
- `data_types.traces` (default `true`): `false` drops transactions and
  spans (outcome `filtered`, reason `disabled`).

Server-side sampling applies after the SDK's own sampling; rupta does not
extrapolate counts, so `count` and `tpm` describe what was stored.

## Retention

`settings.retention.spans` (days, default 30) applies to `transactions`
and `spans`. See [ingestion](ingestion.md#retention).

## API

All endpoints take the shared scope parameters (`project`, `environment`,
`period` or `start`/`end`, see [the API](api.md)); the full schemas are in
`docs/api/openapi.json` (tag `performance`).

### `GET /api/v1/orgs/{org}/performance/transactions`

Transactions grouped by name and operation over the range:

```json
{"data": [{"name": "GET /orders", "op": "http.server", "count": 1520, "tpm": 1.05,
           "p50": 42.0, "p75": 80.1, "p95": 310.4, "p99": 905.2, "avg": 71.3,
           "failure_rate": 0.02, "apdex": 0.93, "users": 211}],
 "next_cursor": null}
```

Durations are milliseconds. `failure_rate` is the share of transactions
whose status is not `ok`, `cancelled` or `unknown`. `apdex` uses T =
300 ms: satisfied up to 300 ms, tolerating up to 1,200 ms. `users` is an
estimate of distinct users. `sort` is `tpm` (default), `count`, `p50`,
`p75`, `p95`, `p99`, `avg`, `failure_rate`, `apdex`, `users` or `name`,
descending; prefix `-` for ascending. `query` filters: free text matches
the name; keys `op`, `status`, `release`, `platform`, `sdk`, `trace`,
`duration` (milliseconds, with comparisons); other keys match tags.

### `GET /api/v1/orgs/{org}/performance/transactions/summary?name=`

One transaction name: `stats` (as above, several operations merged), a
time series `series: [{ts, count, p50, p95}]` with `interval` seconds,
a duration `histogram` of 20 buckets from 0 to the 99th percentile
(`[{start, end, count}]`), the ten `slowest` transactions with trace,
span and profile ids, `related_issues` (issues with error events in this
transaction in the range, most events first) and `web_vitals`:
`{lcp, fcp, cls, inp, ttfb}` each `{p75, count}` (milliseconds; CLS
unitless).

### `GET /api/v1/orgs/{org}/traces/{trace_id}`

Everything stored for one trace in the organization's projects, without
a time range:

- `transactions`: the trace's transactions (with project slug, profile
  links);
- `spans`: the span tree. Each node has `span_id`, `parent_span_id`,
  `segment_id`, `is_segment`, `project_id`, `transaction`, `op`,
  `description`, `status`, `start_timestamp`, `end_timestamp`,
  `duration_ms`, `data`, `tags` and `children` (by start time). Roots are
  spans whose parent is not in the trace (the trace's entry point, and
  spans whose parent service sent nothing). At most 10,000 spans are
  loaded (`truncated: true` beyond); the tree is at most 256 levels deep.
- `errors`: error events of the trace `{event_id, issue_id, project_id,
  title, level, span_id, timestamp}`;
- `logs_count`: logs with this trace id (list them with
  `GET /orgs/{org}/logs?query=trace:<id>`);
- `start`, `end`, `duration_ms`, `span_count`.

A trace id with nothing stored answers `404`.

### `GET /api/v1/orgs/{org}/spans`

The span explorer. `group_by` is `op` (default), `description`,
`transaction` or `none`. Grouped: `groups: [{group, count, p50, p95, avg,
sum_duration_ms}]`, sorted by `sort` (`sum` default, `count`, `p50`,
`p95`, `avg`). `group_by=none` returns the matching spans newest first in
`spans`. `query` keys: `op`, `description` (substring), `transaction`,
`status`, `release`, `trace`, `platform`, `duration`; other keys match
span data; free text matches the description. `limit` 1 to 100.

## Performance of the queries

Measured with `cargo run --release -p rupta-api --example
telemetry_bench` through the HTTP API on one project with 100,000
transactions and 1,000,000 spans over 7 days (4 shared vCPUs, local disk,
warm cache; full table in `benchmarks/telemetry-2026-11-14.txt`):

| Query | Median |
| --- | ---: |
| transactions, 24 hours | 5 ms |
| transactions, 7 days | 15 ms |
| transaction summary, 7 days | 119 ms |
| trace by id | 10 ms |
| spans grouped by op, 7 days | 13 ms |

Aggregations run in squad's column engine. The summary is slower because
its histogram groups by an expression, which the row engine evaluates; it
reads only the rows of one transaction name.

## Limits

- Percentiles are estimates from a quantile sketch; exact percentiles
  (`percentile_cont`) are not available in squad.
- Transactions and spans have no inbound filters or rate limits of their
  own beyond the project rate limit for `transaction` items.
- Span links, span events and OpenTelemetry (`otel_span`) items are not
  stored.
- The trace view joins nothing across organizations.
