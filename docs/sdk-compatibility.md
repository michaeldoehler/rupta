# SDK compatibility: Sentry SDKs and sentry-cli

rupta implements the ingestion protocol of the **Sentry SDKs**: the
open-source client libraries that Sentry publishes for each platform, such
as `@sentry/browser`, `@sentry/node`, `@sentry/react` and `@sentry/nextjs`
for JavaScript, `sentry/sentry`, `sentry/sentry-laravel` and
`sentry/sentry-symfony` for PHP, `sentry-sdk` for Python, and the SDKs for
Go, Java, .NET, Ruby, Rust and the mobile platforms. It also implements
the source map and release commands of **`sentry-cli`**
([source map upload](sourcemap-upload.md)).

rupta has no SDK of its own. You install the Sentry SDK for your platform,
unchanged, and give it a DSN of a rupta project
(`http(s)://<public key>@<host>/<project id>`); the SDK then sends to the
endpoints [below](#endpoints).

> Sentry is a trademark of Functional Software, Inc. rupta is an
> independent project and is not affiliated with or endorsed by Sentry.

This page lists, per SDK and data type, what was tested and what is
expected to work, what each part of the protocol does in rupta, what is
stored, what is acknowledged and dropped, and the limits.

## Switching an existing application

An application that already reports to Sentry (sentry.io or a self-hosted
Sentry) switches to rupta by changing its DSN:

1. Create a project in rupta (*Settings → All projects → New
   project*) and copy the DSN
   from its *Client keys & SDK* page.
2. Replace the DSN in the application's configuration, for example the
   `SENTRY_DSN` environment variable, `SENTRY_LARAVEL_DSN` in Laravel, or
   the `dsn` option of `Sentry.init` / `sentry_sdk.init`.
3. Redeploy. The SDK, its options (`release`, `environment`, sample rates,
   `beforeSend`, integrations) and your instrumentation code stay as they
   are.

Build pipelines that upload source maps with `sentry-cli` or a bundler
plugin change `SENTRY_URL` to rupta's URL and `SENTRY_AUTH_TOKEN` to a
rupta token; organization and project slugs are rupta's
([source map upload](sourcemap-upload.md)). Data already stored in Sentry
is not migrated: rupta starts with an empty history.

## Compatibility matrix

Levels:

- **tested**: tested end to end. The SDK sent the data to a running rupta
  server in the [smoke test](#smoke-test), and the stored result was
  checked; the captured envelopes are replayed in the test suite.
- **expected**: expected to work. The SDK sends the same envelope item
  types as a tested SDK, which rupta handles the same way regardless of
  the SDK, but this SDK was not tested. Only where the SDK offers the
  feature.
- **accepted, not stored**: rupta answers `200` as the SDK expects and
  counts the item as `dropped_unsupported`, but does not store it.
- **not supported**: does not work with rupta.
- **–**: the SDK does not send this kind of data.

| Data type | Browser JavaScript | Node.js | PHP | Python | Frameworks built on these SDKs | Other server SDKs | Mobile and native SDKs |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Errors and messages | tested | tested | tested | tested | expected | expected | expected ¹ |
| Transactions and spans (tracing) | tested | tested | tested | tested | expected | expected | expected |
| Logs | tested | tested | tested | tested | expected | expected | expected |
| Metrics (`trace_metric`) | tested | tested | tested | tested | expected | expected | expected |
| Profiles | tested ² | tested ³ | expected ⁴ | tested ⁵ | expected | expected | expected ¹ |
| Session replay | tested | – | – | – | expected (browser side) | – | accepted, not stored ⁶ |
| Sessions (release health) | tested | tested | – | tested | expected | expected | expected |
| Attachments | expected | tested | tested | tested | expected | expected | expected ⁷ |
| User feedback | tested | tested | expected | expected | expected | expected | expected |
| Cron check-ins | – | tested | tested | tested | expected | expected | – |
| Native crash reports (minidumps) | – | – | – | – | – | – | not supported ⁷ |

Columns:

- **Browser JavaScript**: `@sentry/browser` 11.12.0 in headless Chromium.
- **Node.js**: `@sentry/node` and `@sentry/profiling-node` 11.12.0.
- **PHP**: `sentry/sentry` 4.37.0.
- **Python**: `sentry-sdk` 2.75.0.
- **Frameworks built on these SDKs**: `@sentry/react`, `@sentry/vue`,
  `@sentry/angular`, `@sentry/svelte`, `@sentry/nextjs`,
  `@sentry/nuxt`, `@sentry/sveltekit` and other JavaScript framework
  packages; `sentry/sentry-laravel` and `sentry/sentry-symfony`; the
  Django, Flask, FastAPI and Celery integrations of `sentry-sdk`. They
  wrap a tested SDK and send the same envelopes, but were not tested
  themselves.
- **Other server SDKs**: Go (`sentry-go`), Java and Spring Boot, .NET and
  ASP.NET Core, Ruby and Rails, Rust (`sentry` crate), Elixir and others.
  Not tested.
- **Mobile and native SDKs**: Android, Apple (Cocoa), React Native,
  Flutter, Unity, Unreal and Native (C/C++). Not tested.

Notes:

1. Stack traces from native code and from obfuscated or minified mobile
   builds are stored as sent: rupta has no native symbolication (dSYM,
   PDB, ELF debug files), no ProGuard/R8 mapping and no Hermes source
   maps. Frames that the SDK already resolved (most JVM, .NET, Python,
   Ruby, PHP and Node frames) are readable.
2. With `browserProfilingIntegration`; the page must be served with
   `Document-Policy: js-profiling` ([profiling](profiling.md)).
3. Continuous profiling with `@sentry/profiling-node`.
4. Needs the `excimer` extension, which was not installed on the test
   machine. The profile format is the same as Python's transaction
   profiles and is covered by a unit test.
5. Transaction profiles (`profiles_sample_rate`) and continuous profiles
   (`profile_session_sample_rate`).
6. Mobile session replay (`replay_video` items) is acknowledged but not
   stored and cannot be played.
7. A minidump that the SDK sends as an attachment of an event is stored
   and can be downloaded from the event, but it is not processed: no stack
   trace is extracted from it. Crash uploads to the minidump endpoint
   (`/api/<project id>/minidump/`, used by Crashpad, Breakpad and
   Electron's crash reporter) and Unreal crash reports answer `404`.

### Not supported

- Native crash processing and native debug symbols (see notes 1 and 7);
  `sentry-cli debug-files`.
- Mobile session replay (`replay_video`).
- Source maps for profile frames: minified JavaScript frames in
  flamegraphs stay minified.
- The OpenTelemetry endpoints (`/api/<project id>/otlp/...`,
  `/api/<project id>/integration/otlp/...`): use a Sentry SDK, or send
  `otel_log` envelope items. `otel_span` items are accepted, not stored.
- Span links and span events.
- Commits, repositories and suspect commits (`sentry-cli releases
  set-commits`), and the other `sentry-cli` commands listed as not
  supported in [source map upload](sourcemap-upload.md#commands-that-work).
- Sentry's web API under `/api/0/` beyond the requests `sentry-cli` makes
  for releases, deploys and source maps. Tools that read issues or events
  through that API (dashboards, chat bots, editor plugins) do not work
  with rupta; use rupta's [REST API](api.md) instead.
- `metric_meta` items (code locations of the deprecated metrics format):
  accepted, not stored.

## Tested SDK details

The SDK smoke test (`scripts/sdk-smoke.sh`, see [below](#smoke-test)) runs
example programs with these packages; the envelopes they sent are kept as
test fixtures in `crates/rupta-ingest/tests/fixtures/` (errors, sessions,
check-ins) and `crates/rupta-ingest/tests/fixtures/telemetry/` (a later
capture that adds traces, logs, metrics, profiles and a session replay),
replayed by `cargo test -p rupta-ingest --test sdk_fixtures --test
telemetry_fixtures` and, through the API, by `cargo test -p rupta-api
--test telemetry`.

| SDK | Package | Version tested | Sent and checked |
| --- | --- | --- | --- |
| JavaScript, Node | `@sentry/node`, `@sentry/profiling-node` | 11.12.0 | exception, message, breadcrumbs, user, tags, release, environment, attachment, user feedback, cron check-in, transaction (streamed spans), sessions, logs (`Sentry.logger`), metrics (`Sentry.metrics`), continuous profiles (`profileLifecycle: "trace"`) |
| JavaScript, browser | `@sentry/browser` | 11.12.0 (headless Chromium 1243 via Playwright 1.63) | caught and unhandled exceptions, message, breadcrumbs, user, tags, user feedback, page-load and navigation transactions with web vitals (streamed spans), sessions, logs, metrics, session replay (`replayIntegration`, linked to the errors), browser profiling (`browserProfilingIntegration`, needs `Document-Policy: js-profiling`); `Content-Type: text/plain` envelopes with CORS |
| PHP | `sentry/sentry` | 4.37.0 | exception, message, breadcrumbs, user, tags, attachment, cron check-in, transaction, logs (`\Sentry\logger()`), metrics (`\Sentry\traceMetrics()`); profiling not tested (needs the `excimer` extension, not installed on the test machine) |
| Python | `sentry-sdk` | 2.75.0 | exception, message, breadcrumbs, user, tags, attachment, cron check-in, transaction, sessions, logs (`sentry_sdk.logger`), metrics (`sentry_sdk.metrics`), transaction profiles (`profiles_sample_rate`) and continuous profiles (`profile_session_sample_rate`, `profile_lifecycle="trace"`) |
| hand-written | `statsd` and `metric_buckets` envelopes | — | the deprecated metric formats, which no current SDK sends |

Newer SDK versions are expected to keep working as long as they speak the
same envelope protocol; the smoke test installs the latest versions by
default and is the way to check a new release.

## Endpoints

| Endpoint | Body | Notes |
| --- | --- | --- |
| `POST /api/{project_id}/envelope/` | envelope | What current SDKs use. Browser SDKs send it as `text/plain` to avoid a CORS preflight. |
| `POST /api/{project_id}/store/` | one event as JSON | Legacy endpoint. Also accepts base64-encoded zlib JSON from very old SDKs. |
| `POST /api/{project_id}/security/`, `POST /api/{project_id}/csp-report/` | CSP report | `{"csp-report": {...}}` (`report-uri`) or the Reporting API list (`report-to`). Each `csp-violation` becomes an event of type `csp`; other report types are acknowledged and counted as `dropped_unsupported`. `sentry_environment` and `sentry_release` query parameters set environment and release. |
| `OPTIONS` on all of them | — | CORS preflight. |

The paths work with and without the trailing slash, except
`csp-report/`, which needs it.

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
| `413` | Body over `RUPTA_MAX_ENVELOPE_MB` (default 20 MiB) or over `RUPTA_MAX_DECOMPRESSED_MB` (default 100 MiB) after decompression. |
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
| `event`, `security` | Normalized, filtered, symbolicated (JavaScript), scrubbed, grouped into an issue. | `events` (one row for search and counts), `event_payloads` (the full normalized event, zstd-compressed JSON), `issues` |
| `transaction` | Name, operation, status, duration, trace and span ids, release, environment, tags, measurements (web vitals as columns), profile links; the root span and every span in `spans`. See [performance](performance.md). | `transactions`, `spans` |
| `span` | Streamed spans (current JavaScript SDKs send transactions this way, as a `{"items": [...]}` container). Every span is stored; segment (root) spans also as transactions. | `spans`, `transactions` |
| `attachment` | Bytes go to the object store under `attachments/{project_id}/{day}/{event_id}/{id}`; linked to the envelope's event. Dropped with the event if the event is filtered or invalid. | `attachments`, object store |
| `session` | Release health, see [sessions](#sessions). | `session_counts` |
| `sessions` | Pre-aggregated sessions (server SDKs in request mode). | `session_counts` |
| `client_report` | What the SDK dropped itself (sampling, queue overflow, rate limits, `beforeSend`). | `outcomes` with outcome `client_discard` and the SDK's reason |
| `user_report` | Legacy user feedback (`event_id`, `name`, `email`, `comments`). | `user_feedback` |
| `feedback` | User feedback (`contexts.feedback`: message, name, contact e-mail, URL, associated event). | `user_feedback` |
| `check_in` | Cron monitor check-in. The monitor is created on first use (named after its slug) and its schedule updated from `monitor_config`; `in_progress` followed by `ok`/`error` with the same check-in id updates one check-in (duration from the SDK, or measured). | `monitors`, `checkins` |
| `replay_event`, `replay_recording` | Session replay segments, see [session replay](session-replay.md). | `replays`, `replay_segments`, object store |
| `replay_video` | Mobile replay video: acknowledged, not stored. | `outcomes` (`dropped_unsupported`, category `replay`) |
| `profile`, `profile_chunk` | Sampled profiles, see [profiling](profiling.md). | `profiles`, object store |
| `trace_metric`, `statsd`, `metric_buckets` | Metrics, see [metrics](metrics.md). | `metrics` |
| `metric_meta` | Acknowledged, not stored. | `outcomes` (`dropped_unsupported`, category `metric_bucket`) |
| `log`, `otel_log` | Structured logs, see [logs](logs.md). | `logs` |
| `otel_span` | Acknowledged, not stored. | `outcomes` (`dropped_unsupported`, category `span`) |
| anything else | Acknowledged, not stored. | `outcomes` (`unknown`, reason `unknown_item`) |

Every item handler is selected in one place (`handle_item` in
`crates/rupta-ingest/src/process.rs`), with the item bytes and the
envelope headers (`event_id`, `trace`, `sdk`, ...) at hand.

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

Distinct users: a session's `did` (distinct id; for aggregated
`sessions` items, the `did` of each aggregate) is hashed and written to
`session_users` once per user, project, release, environment and hour of
the session start, and once more when the user's session crashes (for
aggregates: when the aggregate has `crashed` > 0). Crash-free users =
1 − distinct crashed users / distinct users, counted with
`count(DISTINCT)` over these rows. Sessions without a `did` count for
sessions, not for users. Which users were written is kept in memory (up to
500,000 entries, then cleared); a duplicate row after a restart or a
cleared set only costs storage, since users are counted distinct.

Simplifications: which transitions of a session were counted is kept in
memory (the last 250,000 to 500,000 sessions). After a restart, an update
for a session started before the restart can count its error or crash
again, and its `init` was counted before. Sessions need a release; without
one they are invalid.

## Limits

| Limit | Default | Setting |
| --- | --- | --- |
| Request body | 20 MiB | `RUPTA_MAX_ENVELOPE_MB` |
| Decompressed body | 100 MiB | `RUPTA_MAX_DECOMPRESSED_MB` |
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
runs `scripts/sdk-smoke/{node.mjs,php.php,python.py,python-continuous.py}`
and the browser example (`browser.js` bundled with esbuild into
`browser.html`, driven by `run-browser.cjs` with Playwright in headless
Chromium: it clicks a button, navigates with the history API, triggers
errors and waits more than five seconds, because shorter replays are not
sent), sends a hand-written `statsd` and `metric_buckets` envelope, stops
the server and checks the stored data: errors, transactions, spans, logs
and metrics from every SDK, profiles, the replay with its clicks and
linked errors, and that nothing was rejected or dropped as unsupported.
It needs node and npm, php and composer, python3 and curl, and for the
browser the `playwright` Node module with its Chromium
(`PLAYWRIGHT_BROWSERS_PATH`); missing toolchains are skipped unless
`SMOKE_REQUIRE_ALL=1`. Chromium is started with `--no-proxy-server` and
`--disable-features=LocalNetworkAccessChecks`, because the page and the
server both run on `127.0.0.1` and recent Chromium otherwise asks before a
page may reach a loopback address. With `SMOKE_CAPTURE_DIR=<dir>` every request is also
recorded; `scripts/sdk-smoke/make-fixtures.py` turns the recording into the
test fixtures, replacing keys, paths, ports and host names.
