# Metrics

rupta stores the metrics the Sentry SDKs send
(`Sentry.metrics.count/gauge/distribution` in JavaScript,
`sentry_sdk.metrics.*` in Python, `\Sentry\traceMetrics()` in PHP) and the
older statsd-style metric formats, aggregates them over time and shows
them on dashboards.

## What is stored

| Item | Format | Stored |
| --- | --- | --- |
| `trace_metric` | container `{"items": [...]}` of `{timestamp, trace_id, span_id, name, type, value, unit, attributes}`; `type` is `counter`, `gauge` or `distribution` | one row per item |
| `statsd` (deprecated) | text lines `name@unit:value[:value...]\|type\|#tag:v,tag:v\|T<unix>`; types `c`, `g`, `d`, `s` (and `ms`, `h` as distributions) | counters: one row with the sum of the values; gauges (`last:min:max:sum:count`): one row with the last value; distributions: one row per value; sets: one row per member |
| `metric_buckets` (deprecated) | JSON `[{timestamp, width, name, type, value, tags}]` with names like `c:custom/name@unit` | as statsd; gauges store `last` |

Each row of the columnar `metrics` table has timestamp, received time,
name, type (`counter`, `gauge`, `distribution`, `set`), unit (`none` is
stored as no unit), value, trace and span id, environment, release and
attributes (typed attributes flattened to plain values; statsd tags as
strings). Set members are stored as numbers: integers as themselves,
other members as a 32-bit hash; `count_unique` counts distinct values.

`metric_meta` items (code locations of the deprecated format) are
acknowledged and not stored. No current SDK sends `statsd` or
`metric_buckets`; they are tested with hand-written envelopes.

Limits: 10,000 values per item, names up to 200 characters, 100
attributes per value.

## Switches and retention

`settings.data_types.metrics = false` drops metrics (outcome `filtered`,
reason `disabled`). `settings.retention.metrics` (days, default 90). There
is no server-side sampling of metrics.

## API

Scope parameters as in [the API](api.md); schemas under the `metrics` tag.

### `GET /api/v1/orgs/{org}/metrics/names`

`{"data": [{"name": "http.latency", "type": "distribution", "unit": "millisecond", "last_seen": "...", "count": 3320}]}`,
by name, for the range (default 7 days).

### `GET /api/v1/orgs/{org}/metrics/query`

Parameters: `name` (required), `op`, `group_by`, `interval`, `query`.

- `op`: `sum`, `avg`, `min`, `max`, `count`, `count_unique`, `p50`, `p75`,
  `p95`, `p99`. Default by type: `sum` for counters, `avg` for gauges,
  `p95` for distributions, `count_unique` for sets.
- `group_by`: an attribute name, or `environment` / `release`; one series
  per value, the 10 largest by total (`truncated: true` if there were
  more).
- `query`: `key:value` on attributes, `release:`, `trace:`, `value:>n`.

```json
{"name": "http.latency", "op": "p95", "type": "distribution", "unit": "millisecond",
 "interval": 3600, "truncated": false,
 "series": [{"group": {"route": "/checkout"}, "total": 412.5,
             "points": [[1794646800, 380.2], [1794650400, null]]}]}
```

`points` has one entry per bucket; buckets without values are `null`.
`group` is `null` without `group_by`. A name without data answers an
empty `series`.

### Dashboards

`GET/POST /api/v1/orgs/{org}/dashboards`, `GET/PATCH/DELETE
/api/v1/orgs/{org}/dashboards/{id}`. Viewers read, members and above
create, change and delete; changes are written to the audit log.

```json
{"id": "3", "title": "Checkout", "description": null, "created_by": "1",
 "created_at": "...", "updated_at": "...",
 "widgets": [{"id": "w1", "title": "Requests", "type": "metric", "chart": "line",
              "query": {"name": "http.requests", "op": "sum", "group_by": "route"},
              "layout": {"x": 0, "y": 0, "w": 6, "h": 4}}]}
```

A widget's `type` is `metric`, `transactions`, `logs` or `issues`, its
`chart` `line`, `area`, `bar`, `table` or `number`; `query` holds the
parameters of the matching query endpoint and `layout` the position on a
12-column grid. rupta checks `type`, `chart` and that `query` is an
object, and stores everything else as given; at most 50 widgets and
256 KiB per dashboard. The time range is chosen in the UI, not stored.

## Performance

Through the API on 1,000,000 metric values (3 names) over 7 days: metric
names 17 ms, `p95` of one name 19 ms, `sum` grouped by an attribute 14 ms
(`benchmarks/telemetry-2026-11-14.txt`).

## Limits

- Values are stored as sent, not pre-aggregated: a counter incremented a
  million times is a million rows. The SDKs batch but do not aggregate
  `trace_metric` values.
- Gauges of the deprecated formats keep only the last value of a bucket.
- Percentiles are estimates; set cardinality is an estimate
  (`approx_count_distinct`).
