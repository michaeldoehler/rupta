# SDK compatibility

rupta accepts events from the official open-source SDKs without changes
to the SDKs: point their DSN at rupta (`http(s)://<public key>@<host>/<project id>`)
and they send to the endpoints below. This page lists what each part of
the protocol does in rupta, what is stored, what is acknowledged and
dropped, and the limits.

## Tested SDKs

The SDK smoke test (`scripts/sdk-smoke.sh`, see [below](#smoke-test)) runs
example programs with these packages; the envelopes they sent are kept as
test fixtures in `crates/rupta-ingest/tests/fixtures/` and replayed by
`cargo test -p rupta-ingest --test sdk_fixtures`.

| SDK | Package | Version tested | Sent and checked |
| --- | --- | --- | --- |
| JavaScript, Node | `@sentry/node` | 11.12.0 | exception, message, breadcrumbs, user, tags, release, environment, attachment, user feedback, cron check-in, transaction (streamed spans), sessions |
| JavaScript, browser | `@sentry/browser` | 11.12.0 (headless Chrome) | caught and unhandled exceptions, message, breadcrumbs, user, tags, user feedback, page-load transaction (streamed spans), sessions; `Content-Type: text/plain` envelopes with CORS |
| PHP | `sentry/sentry` | 4.37.0 | exception, message, breadcrumbs, user, tags, attachment, cron check-in, transaction |
| Python | `sentry-sdk` | 2.75.0 | exception, message, breadcrumbs, user, tags, attachment, cron check-in, transaction, sessions |

Other SDKs that speak the envelope protocol (Go, Java, .NET, Ruby, Rust,
mobile) use the same endpoints and item types and are expected to work,
but are not tested. Native crash reports (minidumps) are stored as
attachments but not processed.

## Endpoints

| Endpoint | Body | Notes |
| --- | --- | --- |
| `POST /api/{project_id}/envelope/` | envelope | What current SDKs use. Browser SDKs send it as `text/plain` to avoid a CORS preflight. |
| `POST /api/{project_id}/store/` | one event as JSON | Legacy endpoint. Also accepts base64-encoded zlib JSON from very old SDKs. |
| `POST /api/{project_id}/security/` | CSP report | `{"csp-report": {...}}` (`report-uri`) or the Reporting API list (`report-to`). Each `csp-violation` becomes an event of type `csp`; other report types are acknowledged and counted as `dropped_unsupported`. `sentry_environment` and `sentry_release` query parameters set environment and release. |
| `OPTIONS` on all three | — | CORS preflight. |

The paths work with and without the trailing slash.

### Authentication

The public key of the DSN is read from, in this order:

1. the `X-Sentry-Auth` header (`Sentry sentry_key=<key>, sentry_version=7, ...`),
   or the same value in `Authorization`;
2. the `sentry_key` query parameter (browser SDKs);
3. the `dsn` in the envelope header.

The key must exist, be active and belong to the project in the URL. The
secret key part of old DSNs is ignored.

### CORS

Requests with an `Origin` header (or, without one, a `Referer`) are checked
against the project's `allowed_origins` setting: `*` (the default) allows
any origin; otherwise an entry is a full origin (`https://app.example.com`),
a host name (any scheme and port) or `*.example.com` (the domain and its
subdomains). Responses carry `Access-Control-Allow-Origin` for the request's
origin and expose `X-Sentry-Error`, `X-Sentry-Rate-Limits` and `Retry-After`.
Preflight requests are answered for allowed origins.

### Responses

| Status | When |
| --- | --- |
| `200 {"id": "<event id>"}` | Accepted. The id is the envelope's event id (32 hex digits); `{}` for envelopes without an event (sessions only, for example). Items that are dropped (filtered, unsupported, invalid) still get 200, as the SDKs expect. |
| `400` | Malformed envelope or JSON, invalid compressed data, bad project id. |
| `401` | No key, or an unknown key. |
| `403` | Disabled key, key of another project, origin not allowed. |
| `413` | Body over `RUPTA_MAX_ENVELOPE_BYTES` (default 20 MiB) or over 100 MiB decompressed. |
| `415` | Unsupported `Content-Encoding`. |
| `429` | Rate limit or full ingest queue, with `Retry-After` and `X-Sentry-Rate-Limits` (see [ingestion](ingestion.md#rate-limits-and-backpressure)). |
| `503` | Shutting down; with durability `safe`, the batch could not be written. |

Error bodies are `{"detail": "..."}`, and the message is repeated in
`X-Sentry-Error`.

`Content-Encoding` may be `gzip`, `deflate` (zlib or raw), `br`, `zstd` or
`identity`.

## Envelope items

| Item type | What rupta does | Stored in |
| --- | --- | --- |
| `event` | Normalized, filtered, symbolicated (JavaScript), scrubbed, grouped into an issue. | `events` (one row for search and counts), `event_payloads` (the full normalized event, zstd-compressed JSON), `issues` |
| `transaction` | Name, operation, status, duration, trace and span ids, release, environment, tags, measurements. No UI in 0.1. | `transactions` |
| `span` | Streamed spans (current JavaScript SDKs send transactions this way, as a `{"items": [...]}` container). Segment (root) spans are stored as transactions; child spans are counted as `dropped_unsupported`. | `transactions` |
| `attachment` | Bytes go to the object store under `attachments/{project_id}/{day}/{event_id}/{id}`; linked to the envelope's event. Dropped with the event if the event is filtered or invalid. | `attachments`, object store |
| `session` | Release health, see [sessions](#sessions). | `session_counts` |
| `sessions` | Pre-aggregated sessions (server SDKs in request mode). | `session_counts` |
| `client_report` | What the SDK dropped itself (sampling, queue overflow, rate limits, `beforeSend`). | `outcomes` with outcome `client_discard` and the SDK's reason |
| `user_report` | Legacy user feedback (`event_id`, `name`, `email`, `comments`). | `user_feedback` |
| `feedback` | User feedback (`contexts.feedback`: message, name, contact e-mail, URL, associated event). | `user_feedback` |
| `check_in` | Cron monitor check-in. The monitor is created on first use (named after its slug) and its schedule updated from `monitor_config`; `in_progress` followed by `ok`/`error` with the same check-in id updates one check-in (duration from the SDK, or measured). | `monitors`, `checkins` |
| `replay_event`, `replay_recording`, `replay_video` | Acknowledged, not stored. | `outcomes` (`dropped_unsupported`, category `replay`) |
| `profile`, `profile_chunk` | Acknowledged, not stored. | `outcomes` (`profile`) |
| `statsd`, `metric_buckets`, `metric_meta`, `trace_metric` | Acknowledged, not stored. | `outcomes` (`metric_bucket`) |
| `log`, `otel_log` | Acknowledged, not stored. | `outcomes` (`log_item`) |
| `otel_span` | Acknowledged, not stored. | `outcomes` (`span`) |
| anything else | Acknowledged, not stored. | `outcomes` (`unknown`, reason `unknown_item`) |

Every item handler is selected in one place (`handle_item` in
`crates/rupta-ingest/src/process.rs`), with the item bytes and the
envelope headers (`event_id`, `trace`, `sdk`, ...) at hand, so support for
replays, profiles, metrics and logs can be added there.

### Events

What normalization accepts and produces is described in the code
(`crates/rupta-core/src/normalize.rs`); in short:

- `exception` as `{values: [...]}`, a list or a single object; exception
  chains with `mechanism`; frames with in-app detection (SDK value, project
  `in_app_include`/`in_app_exclude`, path heuristics for dependencies and
  runtimes); `threads`; a top-level `stacktrace` is shown as the current
  thread.
- `message` as a string or `{message, params, formatted}`, and `logentry`;
  `%s`, `%(name)s`, `{}` and `{0}` placeholders are filled in.
- `tags` as an object or a list of pairs. rupta adds tags derived from the
  event when the SDK did not set them: `level`, `environment`, `release`,
  `dist`, `transaction`, `server_name`, `logger`, `browser`, `browser.name`,
  `os`, `os.name`, `runtime`, `runtime.name`, `device`, `url`, `user`
  (`id:…`, `email:…`, `username:…` or `ip:…`), `handled` (`yes`/`no`) and
  `mechanism`.
- `user` (`id` numbers become strings; other fields go to `user.data`);
  `ip_address: "{{auto}}"`, `sdk.settings.infer_ip: "auto"`, or a browser
  SDK without an address means the client address, taken from the
  connection or, behind trusted proxies, `X-Forwarded-For`.
- `request` with headers as an object or pairs, `query_string` as a string,
  object or pairs (stored as a string), cookies as a string, object or pairs.
- `contexts` (each gets a `type`); `browser` and `os` are derived from the
  `User-Agent` when the SDK sent none.
- `breadcrumbs` (the latest 100), `extra`, `modules`, `sdk`, `fingerprint`,
  `level` (`warn` becomes `warning`, `critical` becomes `fatal`), `logger`,
  `transaction`, `release`, `environment`, `dist`, `server_name`,
  `debug_meta`.
- Timestamps as numbers or ISO 8601 strings; a time more than a minute in
  the future is replaced by the receive time and recorded in `errors`.

Messages that some SDKs send as a type-less exception (to attach a stack
trace) are titled by their text.

### Sessions

`session_counts` holds, per project, release, environment and hour of the
session start: `total`, `errored`, `crashed`, `abnormal`.

- A session counts in `total` once, on its `init` update.
- It counts once in `errored` when it first reports `errors > 0` or ends
  `errored`, `crashed` or `abnormal`; so `errored` includes crashed and
  abnormal sessions. Crash-free rate = 1 − `crashed` / `total`.
- `crashed` and `abnormal` count once each. The status `unhandled` sent by
  current JavaScript SDKs counts as crashed; unknown statuses count as
  still running.
- Aggregated `sessions` items add their counts directly (`errored` of the
  item plus `crashed` and `abnormal` go to `errored`).

Simplifications: which transitions of a session were counted is kept in
memory (the last 250,000 to 500,000 sessions). After a restart, an update
for a session started before the restart can count its error or crash
again, and its `init` was counted before. Distinct users (`did`) are not
counted. Sessions need a release; without one they are invalid.

## Limits

| Limit | Default | Setting |
| --- | --- | --- |
| Request body | 20 MiB | `RUPTA_MAX_ENVELOPE_BYTES` |
| Decompressed body | 100 MiB | — |
| One event or transaction (JSON) | 1 MiB; larger ones are dropped as `invalid` (`too_large`) | — |
| One attachment | 100 MiB | — |
| Items per envelope | 1,000 | — |
| Breadcrumbs per event | 100 (the latest) | — |
| Frames per stack trace | 250 (oldest and newest 125) | — |
| Strings in free-form data | 8 KiB; message and exception value 8 KiB; tag keys and values 200 characters | — |
| Nesting / items in free-form data | depth 8, 200 items per object or array | — |

## Smoke test

```sh
cargo build -p rupta-server
scripts/sdk-smoke.sh
```

The script starts a server on a free port with a temporary data directory,
seeds a project and key (`cargo run -p rupta-ingest --example ingest_admin`),
installs the SDKs into a temporary directory (`SMOKE_SDK_DIR` reuses one),
runs `scripts/sdk-smoke/{node.mjs,browser.js,php.php,python.py}` (the
browser example in headless Chrome when `google-chrome` or `chromium` is
found), stops the server and checks the stored data. It needs node and npm,
php and composer, python3 and curl; missing toolchains are skipped unless
`SMOKE_REQUIRE_ALL=1`. With `SMOKE_CAPTURE_DIR=<dir>` every request is also
recorded; `scripts/sdk-smoke/make-fixtures.py` turns the recording into the
test fixtures, replacing keys, paths, ports and host names.
