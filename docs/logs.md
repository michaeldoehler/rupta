# Logs

rupta stores the structured logs that the Sentry SDKs send
(`Sentry.logger.info(...)` in JavaScript, `sentry_sdk.logger` in Python
with `enable_logs=True`, `\Sentry\logger()` in PHP with `enable_logs`) and
OpenTelemetry log records, lets you search them, count them over time, and
follow new ones live.

## What is stored

| Item | Content |
| --- | --- |
| `log` | a container `{"items": [...]}` (content type `application/vnd.sentry.items.log+json`); each record has `timestamp`, `level`, `body`, `trace_id`, optional `span_id`, `severity_number` and typed `attributes` |
| `otel_log` | one OpenTelemetry log record in OTLP JSON (`timeUnixNano`, `severityNumber`, `severityText`, `body`, `attributes`, `traceId`, `spanId`) |

Each record is one row of the columnar `logs` table: id (assigned by
rupta), timestamp, received time, trace id, span id (also taken from the
`sentry.trace.parent_span_id` attribute), level, severity number, body,
environment, release, SDK and attributes.

- Typed attributes (`{"key": {"value": 42, "type": "integer"}}`) are
  stored as plain JSON values (`{"key": 42}`); OTLP values the same way.
- Levels are `trace`, `debug`, `info`, `warn`, `error` and `fatal`
  (`warning` becomes `warn`, `critical` becomes `fatal`; otherwise the
  severity number decides).
- `sentry.environment`, `sentry.release` and `sentry.sdk.name` also fill
  the environment, release and SDK columns (falling back to the envelope's
  trace header and SDK).
- Data scrubbing (project settings, on by default) applies to attribute
  keys and values and to the body: values of keys like `password` become
  `[Filtered]`, card numbers and bearer tokens in strings are replaced.

Limits: 10,000 records per item, body up to 16 KiB, 100 attributes per
record, attribute names up to 200 and string values up to 4 KiB.

## Sampling, switches, retention

- `settings.sampling.logs` (0 to 1, default 1): records with a trace id
  are kept or dropped together with their trace (by a hash of the trace
  id); records without one at random. Outcome `filtered`, reason
  `sample_rate`.
- `settings.data_types.logs = false` drops all logs (reason `disabled`).
- `settings.retention.logs` (days, default 30).

## API

Scope parameters as described in [the API](api.md); schemas under the
`logs` tag of `docs/api/openapi.json`.

### `GET /api/v1/orgs/{org}/logs`

```json
{"data": [{"id": "0195f3...", "project_id": "1", "timestamp": "2026-11-14T15:21:39.883Z",
           "received": "2026-11-14T15:21:40.102Z", "level": "info", "severity_number": 9,
           "body": "smoke node log", "trace_id": "371aa2...", "span_id": "83c6d2...",
           "environment": "smoke", "release": "smoke-node@1.0.0", "sdk": "sentry.javascript.node",
           "attributes": {"order.id": 42, "smoke.kind": "node", "sentry.sdk.version": "11.12.0"}}],
 "next_cursor": "..."}
```

`sort` is `-timestamp` (newest first, default) or `timestamp`; `limit` 1
to 1,000 (default 100). `query`:

| Term | Matches |
| --- | --- |
| free text | substring of the body, case-insensitive (`%` and `_` are removed) |
| `level:error`, `level:[warn,error]` | levels |
| `trace:<id>`, `span:<id>` | trace and span ids |
| `release:`, `sdk:` | columns |
| `severity:>=17` | severity number |
| `body:` / `message:` | substring of the body |
| any other `key:value` | the attribute `key` (exact value) |
| `!term` | negation |

### `GET /api/v1/orgs/{org}/logs/histogram`

Counts per bucket and level for the same filter:
`{"interval": 3600, "buckets": [{"ts": 1794650400, "total": 12, "counts": {"info": 10, "error": 2}}], "totals": {"info": 300, "error": 21}}`.
Every bucket of the range is present; `interval=1m|1h|1d` chooses the
bucket size.

### `GET /api/v1/orgs/{org}/logs/stream` (live tail)

Server-Sent Events (`text/event-stream`) for the same scope and `query`:

- `event: ready` once the stream is open;
- `event: logs`, `data:` a JSON array of log records (oldest first) for
  every poll that found new ones;
- `event: lagging`, `data: {"skipped": n}` when more than 500 records
  arrived within one poll; the oldest ones were skipped;
- `event: error` with `{"code", "message"}` if a poll failed;
- a comment line every 15 seconds keeps idle connections open.

Use it with `new EventSource(url)`; the session cookie authenticates it
(GET requests need no CSRF header).

Limits, deliberately simple:

- The server polls the `logs` table once per second per stream; there is
  no push from ingestion. Expect one to two seconds of delay (plus the
  batch interval of ingestion, and longer with `RUPTA_DURABILITY=fast`
  under load).
- Records are selected by the time rupta received them, looking back
  10 seconds to catch records committed out of order, and deduplicated by
  id. Records whose own timestamp is more than one hour older than the
  time they were received are not shown in the tail (they are stored and
  searchable).
- Only records received after the stream started are sent; load history
  with the list endpoint.
- A stream ends after 30 minutes; `EventSource` reconnects by itself.
- Each open stream costs one small query per second; dozens of streams
  are fine, thousands are not.

## Performance of the queries

Through the API, one project with 1,000,000 logs over 7 days (see
`benchmarks/telemetry-2026-11-14.txt`):

| Query | Median |
| --- | ---: |
| newest 100, 24 hours | 14 ms |
| `level:error`, 7 days | 5 ms |
| attribute filter (`customer.id:c42`, 1 in 1,000), 7 days | 26 ms |
| body text, 7 days | 12 ms |
| by trace id | 7 ms |
| histogram, 7 days | 8 ms |

Pages are looked for in growing windows next to the cursor (5 minutes,
1 hour, 6 hours, 1 day, 7 days, then the whole range), because sorting a
whole time range of a columnar table to return a page is slow. A filter
that matches very few records in a long range reads the range once per
window size; it stays correct but costs up to six scans.

## Limits

- No full-text index: body search is a substring scan over the selected
  time range.
- Attributes are searched by exact value; nested attribute values are
  stored as JSON but not searchable by path.
- Logs are not linked to issues automatically; the trace id connects them
  to transactions and errors.
