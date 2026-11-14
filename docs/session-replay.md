# Session replay

rupta stores the session replays recorded by the browser SDK's
`replayIntegration` (rrweb recordings) and links them to the errors that
happened during the session, so an issue shows the replays in which it
occurred.

## What the SDK sends

For each recording segment the SDK sends one envelope with:

- a `replay_event` item: replay id, segment id, `replay_start_timestamp`,
  `timestamp`, the URLs visited, ids of errors and traces seen since the
  last segment, replay type (`session` or `buffer`), user, browser and OS
  contexts, release and environment;
- a `replay_recording` item: a JSON header line `{"segment_id": n}`, then
  the rrweb events as a JSON array, usually zlib-compressed.

Error events recorded while a replay runs carry `contexts.replay.replay_id`
(older SDKs: the tag `replayId`).

## What is stored

- The recording is decompressed (zlib or gzip, at most 100 MiB), checked
  to be a JSON array, compressed with zstd and stored in the object store
  under `replays/{project}/{day}/{replay_id}/{segment_id}.zst`. `day` is
  the day the replay started, so all segments of a replay expire
  together. A segment id seen before is not stored again.
- `replay_segments` has one row per segment (object key, size).
- `replays` has one row per replay, created by its first item and widened
  by later ones: start and end (from the replay events and the rrweb event
  timestamps), segment count, URLs, error and trace ids (each up to 1,000),
  user (id, e-mail, name, IP address unless IP scrubbing is on), browser,
  OS, device (from the contexts or the User-Agent), environment, release,
  replay type, number of errors, number of clicks (rrweb mouse
  interactions of type click), size.
- `issue_replays` links each error event that names a replay to its issue;
  the event id is also added to the replay's error ids.

`replay_video` items (mobile replays) are acknowledged and not stored
(outcome `dropped_unsupported`).

Note: the SDK sends nothing for sessions shorter than five seconds
(`minReplayDuration`).

## Retention and switches

`settings.retention.replays` (days, default 30) deletes replays started
before the cutoff with their segments, links and objects (by day prefix).
`settings.data_types.replays = false` drops replay items (outcome
`filtered`, reason `disabled`). Sampling is the SDK's
(`replaysSessionSampleRate`, `replaysOnErrorSampleRate`); there is no
server-side sampling.

## Privacy

The SDK masks text and blocks media by default; rupta stores the
recording as sent. Data scrubbing of rupta does not look inside
recordings. Keep the SDK's masking on for applications that show
personal data, and see [data scrubbing](data-scrubbing.md).

## API

Schemas under the `replays` tag of `docs/api/openapi.json`.

### `GET /api/v1/orgs/{org}/replays`

Scope parameters (default range 7 days, by start time), `query`, `sort`,
`cursor`, `limit`:

```json
{"data": [{"replay_id": "4577a92088af4a41974244c895c056b6", "project_id": "1",
           "started": "...", "finished": "...", "duration_ms": 9512, "segment_count": 1,
           "count_errors": 3, "count_clicks": 2, "urls": ["http://localhost:3000/index.html"],
           "user": {"id": "browser-user", "email": "browser@example.com", "name": null, "ip_address": "203.0.113.7"},
           "browser": "Chrome 140.0", "os": "Linux", "device": null, "environment": "smoke",
           "release": "smoke-browser@1.0.0", "platform": "javascript", "sdk": "sentry.javascript.browser",
           "replay_type": "session", "error_ids": ["..."], "trace_ids": ["..."]}],
 "next_cursor": null}
```

`query` keys: `url` (substring of any URL), `user.email`, `user.id`,
`user.name`, `user.ip`, `browser`, `os`, `device` (substrings), `release`,
`platform`, `replay_type`, `count_errors`, `count_clicks`,
`count_segments`, `duration` (milliseconds), `error_id`, `trace`; free
text matches URLs and the user. `sort`: `-started` (default), `started`,
`-duration`, `duration`, `-count_errors`, `-count_clicks`.

### `GET /api/v1/orgs/{org}/replays/{project}/{replay_id}`

The replay as above plus `issues: [{id, short_id, title, level, status,
event_ids}]` (issues of errors recorded during the replay) and
`recording_url`.

### `GET /api/v1/orgs/{org}/replays/{project}/{replay_id}/recording-segments`

`{"data": [[...rrweb events of segment 0...], [...segment 1...]], "next_cursor": null}`:
the segments in order, each exactly as recorded (decompressed), `limit`
segments per page (1 to 100, default 100). The body is streamed segment by
segment; pass it to `rrweb-player` as one concatenated event list.

### `GET /api/v1/issues/{id}/replays`

Replays in which errors of the issue were recorded, newest first, in the
list format. Event documents (`GET /api/v1/issues/{id}/events/...`) carry
`replay_id`.

## Tested

`@sentry/browser` 11.12.0 with `replayIntegration` in headless Chromium
(Playwright): page load, a click, a history navigation, a handled and an
unhandled error; the captured envelope is replayed by
`crates/rupta-ingest/tests/telemetry_fixtures.rs` and through the API by
`crates/rupta-api/tests/telemetry.rs`.

## Limits

- Network and console panels depend on what the SDK puts into the
  recording (breadcrumb and performance span custom events); rupta does
  not add data.
- Canvas recording (`replayCanvasIntegration`) is stored like any other
  rrweb event but was not tested.
- Search over the contents of recordings is not available.
