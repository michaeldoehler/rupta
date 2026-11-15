# Event fields

An event is one occurrence of an error or message. Ingestion normalizes
what the SDK sent into one shape and stores it; the API returns it
unchanged from `GET /api/v1/issues/{id}/events/{event}` and
`GET /api/v1/orgs/{org}/events/{project}/{event_id}`, and the MCP tool
`get_event` renders it as text. Every top-level key is always present
(`null` or empty when unknown); inside nested objects, absent values are
left out.

## Top level

| Field | Meaning |
| --- | --- |
| `event_id` | 32 lowercase hex digits |
| `project_id`, `issue_id` | ids as strings |
| `type` | `error` (has an exception), `default` (a message), `csp` (a security report) |
| `timestamp`, `received` | when it happened (SDK clock) and when rupta received it, UTC |
| `level` | `debug`, `info`, `warning`, `error` or `fatal` |
| `platform` | e.g. `javascript`, `node`, `python`, `php` |
| `title` | `Type: value` of the exception, or the message |
| `culprit` | where it happened: the transaction, or else the function and module of the last in-app frame |
| `message` | the log message, if any |
| `logger`, `server_name`, `transaction` | as sent by the SDK |
| `environment`, `release`, `dist` | deployment |
| `fingerprint` | custom grouping values, if the SDK set them |
| `tags` | ordered `[key, value]` pairs, including tags derived from the event (`level`, `environment`, `release`, `browser`, `os`, `url`, …) |
| `user` | `id`, `email`, `username`, `ip_address`, `name`, `geo`, `data` |
| `request` | `url`, `method`, `headers` (`[name, value]` pairs), `query_string`, `data` (body), `cookies`, `env` |
| `contexts` | objects by name: `browser`, `os`, `runtime`, `device`, `app`, `trace` (`trace_id`, `span_id`), `replay`, custom contexts |
| `sdk` | `name`, `version`, `integrations`, `packages` |
| `exception` | `{values: [...]}`, see below |
| `threads` | `{values: [{id, name, crashed, current, main, stacktrace}]}` |
| `breadcrumbs` | `{values: [{timestamp, type, category, level, message, data}]}`, oldest first |
| `extra` | free-form data set by the application |
| `modules` | package versions |
| `debug_meta` | debug images and source map ids |
| `csp` | the violation of a `csp` event |
| `grouping` | `hash`, `version`, `method` (`fingerprint`, `exception`, `stacktrace`, `message`, `csp`, `fallback`) and the `components` hashed; see [grouping](grouping.md) |
| `errors` | problems found while processing (`type`, `name`, `value`), e.g. a source map that could not be applied |

The API adds `previous_event_id` and `next_event_id` (neighbours within
the issue), `trace_id` (from `contexts.trace`) and `replay_id` (from
`contexts.replay` or the `replayId` tag), each `null` when there is none.

Data scrubbing runs before storage, so passwords, tokens, cookies and card
numbers are already replaced by `[Filtered]` ([data scrubbing](data-scrubbing.md)).

## Exceptions

`exception.values` keeps the order the SDK sent: the innermost cause first,
**the exception that was raised last**. Each value has:

| Field | Meaning |
| --- | --- |
| `type`, `value` | class and message, e.g. `TypeError`, `x is undefined` |
| `module` | the module of the class |
| `mechanism` | how it was captured: `type` (`onerror`, `generic`, …), `handled` (`false` for crashes) |
| `stacktrace.frames` | the frames, **oldest call first; the frame that raised is last** |

## Stack frames

| Field | Meaning |
| --- | --- |
| `filename`, `abs_path`, `module`, `package` | where the code is |
| `function` | the function name |
| `lineno`, `colno` | position (1-based) |
| `in_app` | `true` for the application's own code, `false` for libraries and the runtime. Projects can adjust it with `in_app_include` and `in_app_exclude` |
| `pre_context`, `context_line`, `post_context` | source lines around `lineno`, when the SDK or a source map provided them |
| `vars` | local variables (some SDKs) |
| `symbolicated` | `true` when the position was mapped through an uploaded source map; `raw` then holds the minified frame |

## Summaries

When retention has removed the stored event but its row still exists, the
endpoints return a summary instead: `event_id`, `issue_id`, `project_id`,
`timestamp`, `received`, `level`, `title`, `message`, `environment`,
`release`, `transaction`, `user` (`id`, `email`, `ip_address`), `browser`,
`os`, `device`, `url` and `tags` (an object). It has no `type` and no
exception or breadcrumbs. Event lists (`GET /api/v1/issues/{id}/events`)
return the same fields for each event.
