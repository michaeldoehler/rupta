# REST API

rupta has a JSON API under `/api/v1`. The web app uses it, and so can
scripts and other services. The full reference is an OpenAPI 3.1 document,
served by every instance at `/api/v1/openapi.json` and committed in this
repository as [`docs/api/openapi.json`](api/openapi.json). A test fails when
the committed copy differs from the code.

A small part of a second API, under `/api/0`, exists only for the source map
upload CLI; see [source map upload](sourcemap-upload.md).

## Conventions

- Bodies are JSON (`Content-Type: application/json`), field names are
  `snake_case`.
- Timestamps are ISO 8601 in UTC, e.g. `2026-11-14T12:00:00Z`.
- Database ids are 64-bit integers and are sent as **strings** (`"id": "42"`)
  so JavaScript does not lose precision. Requests accept ids as strings or
  numbers.
- Organizations are addressed by slug (`/orgs/acme`), projects by slug or id
  within their organization (`/projects/acme/web`), issues by id.
- JSON request bodies are limited to 1 MiB; larger bodies get `413`.

## Authentication

Two ways, described in detail in [authentication](authentication.md):

- **Session cookie** (`rupta_session`), set by `POST /api/v1/auth/login`,
  `POST /api/v1/auth/setup` or single sign-on. Every request that changes
  something (`POST`, `PUT`, `PATCH`, `DELETE`) must also send the session's
  CSRF token in the `X-CSRF-Token` header. The token is in the response of
  login, setup and `GET /api/v1/auth/session`.
- **API token**: `Authorization: Bearer rpt_...`. No CSRF header is needed.
  What a token may do is limited by its scopes and, for personal tokens, by
  the user's role.

Requests without valid credentials get `401`. Requests for resources of an
organization the caller does not belong to get `404`, exactly as if the
resource did not exist. Members without the necessary role, and tokens
without the necessary scope, get `403`.

## Errors

Every error has the same shape:

```json
{"error": {"code": "validation_failed", "message": "slug: use 1 to 50 lowercase letters, digits and dashes", "details": {"field": "slug", "reason": "..."}}}
```

| Status | `code` | Meaning |
| --- | --- | --- |
| 400 | `bad_request` | Malformed JSON, query string or cursor |
| 401 | `unauthorized`, `invalid_credentials` | Not signed in; wrong email or password |
| 403 | `forbidden`, `csrf_failed`, `sso_required`, `local_login_disabled` | Not allowed; missing CSRF header; the organization requires single sign-on; password login is off |
| 404 | `not_found` | Does not exist or belongs to another organization |
| 409 | `conflict` | Already exists (slug taken), or would remove the last owner |
| 413 | `payload_too_large` | Body over the limit |
| 422 | `validation_failed` | A field is invalid; `details.field` names it |
| 429 | `rate_limited` | Too many failed logins; see `Retry-After` |
| 502 | `delivery_failed`, `sso_unavailable` | A notification channel or the identity provider failed |
| 500 | `internal` | A bug or a storage failure; details are in the server log |

## Pagination

Lists that can grow take `?limit=` (1–100, default 50) and `?cursor=`, and
answer:

```json
{"data": [ ... ], "next_cursor": "eyJ..."}
```

Pass `next_cursor` as `cursor` to get the next page; it is `null` on the
last page. Cursors are opaque and stay valid when new items arrive (they
encode the position, not an offset), except for tag values, which are
paginated by offset.

Short lists that cannot grow large (teams, projects, channels, alert rules,
tokens, monitors, saved searches) are returned as plain arrays.

## Endpoints

Grouped as in the OpenAPI document.

| Area | Endpoints |
| --- | --- |
| Login and session | `GET /auth/config`, `POST /auth/setup`, `POST /auth/login`, `POST /auth/logout`, `GET /auth/session`; browser redirects `GET /auth/sso/{provider}/start`, `GET /auth/sso/{provider}/callback` (not under `/api/v1`) |
| User | `GET/PATCH /user`, `GET/POST /user/tokens`, `DELETE /user/tokens/{id}`, `GET /scopes` |
| Organizations | `GET/POST /orgs`, `GET/PATCH/DELETE /orgs/{org}` |
| Members | `GET/POST /orgs/{org}/members` (POST invites), `PATCH/DELETE /orgs/{org}/members/{user_id}`, `GET /orgs/{org}/invites`, `DELETE /orgs/{org}/invites/{id}`, `GET /invites/{token}`, `POST /invites/{token}/accept` |
| Teams | `GET/POST /orgs/{org}/teams`, `PATCH/DELETE /orgs/{org}/teams/{team}`, `PUT/DELETE /orgs/{org}/teams/{team}/members/{user_id}` |
| Security | `GET /orgs/{org}/audit-log`, `GET/PUT/DELETE /orgs/{org}/sso`, `GET/POST /orgs/{org}/tokens`, `DELETE /orgs/{org}/tokens/{id}` |
| Projects | `GET/POST /orgs/{org}/projects`, `GET/PATCH/DELETE /projects/{org}/{project}`, `GET/POST /projects/{org}/{project}/keys`, `PATCH/DELETE .../keys/{id}`, `GET .../stats` |
| Issues | `GET/PUT/DELETE /orgs/{org}/issues`, `GET/PATCH/DELETE /issues/{id}`, `GET /issues/{id}/activity`, `POST /issues/{id}/comments`, `PATCH/DELETE /issues/{id}/comments/{comment_id}`, `GET/POST /orgs/{org}/saved-searches`, `PATCH/DELETE .../saved-searches/{id}` |
| Events | `GET /issues/{id}/events`, `GET /issues/{id}/events/{latest\|oldest\|recommended\|event_id}`, `GET /orgs/{org}/events/{project}/{event_id}`, `GET /issues/{id}/tags`, `GET /issues/{id}/tags/{key}/values`, `GET /issues/{id}/stats`, `GET /orgs/{org}/events` |
| Feedback and files | `GET /issues/{id}/feedback`, `GET /projects/{org}/{project}/feedback`, `GET /issues/{id}/attachments`, `GET /attachments/{id}/download` |
| Releases | `GET /orgs/{org}/releases`, `GET /orgs/{org}/releases/{version}`, `GET /projects/{org}/{project}/health` |
| Alerts | `GET/POST /projects/{org}/{project}/alert-rules`, `GET/PATCH/DELETE .../alert-rules/{id}`, `GET/POST /orgs/{org}/channels`, `PATCH/DELETE /orgs/{org}/channels/{id}`, `POST /orgs/{org}/channels/{id}/test` |
| Cron monitors | `GET/POST /projects/{org}/{project}/monitors`, `GET/PATCH/DELETE .../monitors/{slug}`, `GET .../monitors/{slug}/checkins` |
| Performance | `GET /orgs/{org}/performance/transactions`, `GET /orgs/{org}/performance/transactions/summary`, `GET /orgs/{org}/traces/{trace_id}`, `GET /orgs/{org}/spans` ([performance](performance.md)) |
| Logs | `GET /orgs/{org}/logs`, `GET /orgs/{org}/logs/histogram`, `GET /orgs/{org}/logs/stream` (Server-Sent Events) ([logs](logs.md)) |
| Metrics | `GET /orgs/{org}/metrics/names`, `GET /orgs/{org}/metrics/query`, `GET/POST /orgs/{org}/dashboards`, `GET/PATCH/DELETE /orgs/{org}/dashboards/{id}` ([metrics](metrics.md)) |
| Profiling | `GET /orgs/{org}/profiles`, `GET /orgs/{org}/profiles/{project}/{profile_id}/flamegraph`, `GET .../raw`, `GET /orgs/{org}/profiles/aggregate-flamegraph` ([profiling](profiling.md)) |
| Replays | `GET /orgs/{org}/replays`, `GET /orgs/{org}/replays/{project}/{replay_id}`, `GET .../recording-segments`, `GET /issues/{id}/replays` ([session replay](session-replay.md)) |

Notes on some of them:

- **Issue list**: `?query=` takes the [search language](search.md);
  `?sort=` is `last_seen` (default), `first_seen`, `freq`, `users` or
  `priority`; `?project=` limits to project ids (comma-separated or
  repeated); `?environment=` limits to issues with events in an
  environment; `?period=` (e.g. `24h`, `14d`) limits to issues seen in that
  period and sets the range of each issue's `stats` sparkline (hourly
  buckets up to 48 hours, daily beyond; default 24 hours).
- **Bulk changes**: `PUT /orgs/{org}/issues?id=1&id=2` with the same body as
  `PATCH /issues/{id}`: `status` (`unresolved`, `resolved`, `ignored`),
  `substatus`, `assignee` (`"user:{id}"`, `"team:{id}"`, `"me"` or `null`),
  `priority` (`high`, `medium`, `low`), `ignore_duration` (minutes),
  `ignore_count` (further events), `resolved_in_release`, and the per-user
  flags `is_bookmarked` and `has_seen`. At most 100 ids; ids of other
  organizations are skipped. Viewers may only change the per-user flags.
- **Event documents** are the normalized events as stored by ingestion,
  plus `previous_event_id` and `next_event_id` within the issue, and
  `replay_id` and `trace_id` (from the replay and trace contexts, `null`
  when the event has none). Event ids
  are 32 lowercase hex digits. When the stored document has been removed by
  retention but the event row still exists, a summary is returned.
- **Tag distributions** combine first-class event attributes (`level`,
  `environment`, `release`, `dist`, `browser`, `os`, `device`, `runtime`,
  `transaction`, `server_name`, `url`, `user`) with the event's tags. A tag
  with the same name as an attribute is shown under the attribute.
- **Project stats** come from the outcomes table: `stat=received` (accepted
  errors), `rejected`, `filtered` or `rate_limited`; `since` and `until` as
  Unix seconds or RFC 3339; `resolution=1h` or `1d`. The answer is
  `[[unix_seconds, count], ...]` with empty buckets filled with zero.
- **Release health** is computed from hourly session counts. Crash-free
  users cannot be derived from them and are always `null`. Adoption is the
  release's share of the project's sessions in the last 24 hours (in the
  release list) or in the requested period (project health).
- **Telemetry endpoints** (performance, logs, metrics, profiles,
  replays) share the scope parameters `project` (ids or slugs,
  comma-separated; default all projects of the organization),
  `environment`, `period` (default 24 hours, 7 days for metric names,
  profiles aggregation and replays) or `start`/`end` (RFC 3339), at most
  366 days. Their `query` parameter uses the syntax of the [search
  language](search.md) (free text, `key:value`, `key:[a,b]`, `!key:value`,
  `key:>n`) with the keys listed on each endpoint; keys that are not
  columns match attributes, tags or span data. Time series are filled with
  a point per bucket (`interval` seconds: a minute up to 3 hours, an hour
  up to 3 days, a day beyond; `interval=1m|1h|1d` to choose).
  Percentiles are estimates (`approx_percentile`).
- **Project settings** also hold `retention` (`logs`, `spans`, `metrics`,
  `profiles`, `replays`, days, 1 to 3650), `sampling` (`transactions`,
  `spans`, `logs`, 0 to 1) and `data_types` (`traces`, `logs`, `metrics`,
  `profiles`, `replays`, booleans); a patch may change single fields of
  each.
- **Event query** (`GET /orgs/{org}/events`) is deliberately small: a filter
  in the search language (event keys only), up to 1,000 rows of chosen
  columns, or counts grouped by one column, over at most 90 days.
- **Attachments** are always sent with `Content-Disposition: attachment`;
  only PNG, JPEG, GIF, WebP, plain text and JSON keep their content type,
  everything else is `application/octet-stream`.
- **Project settings** (`PATCH /projects/{org}/{project}` with `settings`)
  are merged: given top-level keys replace the stored ones, `filters` and
  `scrubbing` are merged key by key. Fields: `allowed_origins`,
  `rate_limit_per_min`, `retention_days` (1–3650), `filters`
  (`browser_extensions`, `localhost`, `legacy_browsers`, `web_crawlers`,
  `error_messages`, `releases`), `scrubbing` (`enabled`, `extra_fields`,
  `safe_fields`, `scrub_ip_addresses`), `in_app_include`, `in_app_exclude`.
  Changes to projects, keys and settings reach ingestion without a restart.

## Examples

Log in and keep the cookie:

```sh
curl -c jar -H 'Content-Type: application/json' \
  -d '{"email":"admin@example.com","password":"..."}' \
  https://errors.example.com/api/v1/auth/login
# The answer contains "csrf_token"; send it on mutating requests:
curl -b jar -H "X-CSRF-Token: $CSRF" -H 'Content-Type: application/json' \
  -d '{"name":"Web shop","platform":"javascript-react"}' \
  https://errors.example.com/api/v1/orgs/acme/projects
```

With a token (create one under *Settings → API tokens*, or
`POST /api/v1/user/tokens` in a session):

```sh
export RUPTA_TOKEN=rpt_...
# Unresolved errors of the last day, most frequent first
curl -H "Authorization: Bearer $RUPTA_TOKEN" \
  'https://errors.example.com/api/v1/orgs/acme/issues?query=is:unresolved%20level:error&sort=freq&period=24h'

# Resolve two issues
curl -X PUT -H "Authorization: Bearer $RUPTA_TOKEN" -H 'Content-Type: application/json' \
  -d '{"status":"resolved","resolved_in_release":"web@1.4.0"}' \
  'https://errors.example.com/api/v1/orgs/acme/issues?id=1203&id=1207'

# The latest event of an issue
curl -H "Authorization: Bearer $RUPTA_TOKEN" \
  https://errors.example.com/api/v1/issues/1203/events/latest
```

## Security properties

- Responses carry `X-Content-Type-Options: nosniff`, `X-Frame-Options:
  DENY`, `Referrer-Policy: no-referrer`, `Cache-Control: no-store`,
  `Cross-Origin-Resource-Policy: same-origin` and `Content-Security-Policy:
  default-src 'none'; frame-ancestors 'none'`.
- Cross-origin browser access (CORS) is off unless origins are listed in
  `RUPTA_CORS_ALLOWED_ORIGINS`; listed origins may send credentials.
- Tokens, sessions, invitation links and CSRF tokens are compared in
  constant time; only their SHA-256 is stored.
- Search values are always bound as SQL parameters.
