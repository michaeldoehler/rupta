# Search

The issue list (`GET /api/v1/orgs/{org}/issues?query=...`) and the search
bar of the web app take a small query language. A query is a list of
terms separated by spaces; an issue must match **all** of them.

```text
is:unresolved level:error browser:Chrome "connection reset" !environment:staging
first_seen:-24h times_seen:>10 tags[region]:eu-west assigned:me
```

## Terms

| Form | Example | Meaning |
| --- | --- | --- |
| `word` | `timeout` | Free text: title or culprit contains it (case-insensitive) |
| `"some words"` | `"connection reset"` | Free text with spaces; `\"` and `\\` escape |
| `key:value` | `level:error` | Filter |
| `key:"value with spaces"` | `os:"Windows 11"` | Quoted value |
| `key:[a,b]` | `level:[error,fatal]` | Any of the values |
| `key:*part*` | `url:*checkout*` | `*` is a wildcard |
| `!key:value` or `-key:value` | `!environment:staging` | Negation (also `!"text"`) |
| `key:>n`, `>=`, `<`, `<=` | `times_seen:>10` | Comparison (numbers and times) |

Keys are case-insensitive; values are compared exactly (except free text
and wildcards). Something that looks like `scheme://...` is free text, not a
filter.

## Issue keys

| Key | Values |
| --- | --- |
| `is:` | `unresolved`, `resolved`, `ignored` (also `archived`), `assigned`, `unassigned`, `bookmarked` (by you), `unseen` (by you), `new`, `ongoing`, `regressed`, `escalating` (substatus) |
| `assigned:` | `me` (you or one of your teams), `none`, an email (`ada@example.com`), `#team-slug` |
| `level:` | `debug`, `info`, `warning`, `error`, `fatal` (the issue's level) |
| `priority:` | `high`, `medium`, `low` |
| `issue.type:` | e.g. `error` |
| `issue.platform:` | e.g. `javascript` |
| `first_release:` | release in which the issue first appeared |
| `first_seen:` | time (below) |
| `last_seen:` | time (below) |
| `times_seen:` | number of events, e.g. `times_seen:>=100` |
| `users_seen:` | number of distinct users |

## Event keys

These match issues that have **at least one event** with the value:

| Key | Event attribute |
| --- | --- |
| `environment:` | environment |
| `release:`, `dist:` | release, distribution |
| `browser:`, `os:`, `device:`, `runtime:` | name and version, e.g. `Chrome 120.0`; a bare name (`browser:Chrome`) also matches every version |
| `user.email:`, `user.id:`, `user.ip:` | user |
| `server_name:`, `transaction:`, `url:`, `sdk:`, `platform:` | |
| `error.type:`, `error.value:` | exception type and message |
| `message:` | log message |
| `tags[key]:value` | any tag, e.g. `tags[region]:eu-west` |
| `key:value` | any other key is a tag: `region:eu-west` |
| `has:key` | the attribute or tag is present, e.g. `has:release`, `has:user` |

## Times

Relative values are **ages**, measured back from now, with the units `s`,
`m`, `h`, `d`, `w`:

| Query | Meaning |
| --- | --- |
| `first_seen:-24h` or `first_seen:<24h` | first seen within the last 24 hours |
| `last_seen:-7d` | seen within the last 7 days |
| `first_seen:+30d` or `first_seen:>30d` | first seen more than 30 days ago |

Absolute dates and timestamps compare the time itself:

| Query | Meaning |
| --- | --- |
| `last_seen:>2026-11-01` | after the start of 1 November 2026 (UTC) |
| `first_seen:2026-11-01` | during that day |
| `first_seen:>=2026-11-01T08:00:00Z` | RFC 3339 timestamps work too |

## Other parameters of the issue list

`sort` (`last_seen`, `first_seen`, `freq`, `users`, `priority`), `project`
(ids), `environment` (same as an `environment:` term) and `period` (only
issues seen within it) are query parameters, not part of the query text;
see [the API](api.md#endpoints).

## Event search

The event list of an issue (`GET /api/v1/issues/{id}/events?query=`) and the
event query endpoint take the same language with the event keys, `level:`,
`timestamp:` and free text (matched against the message and the exception
type and value). Issue keys such as `is:` are refused there.

## Errors and limits

- Invalid queries answer `422 validation_failed` with `details.field` =
  `query` and a message naming the problem (`unknown value is:sideways`,
  `unclosed quote`, ...).
- At most 32 terms and 2,048 bytes.
- Values are always passed to the database as parameters, never as SQL
  text.
- Event keys run a subquery over the `events` table of the selected
  projects; they cost more than issue keys on large projects. Narrow them
  with `period` or issue keys.
- There is no OR between terms (only within `[a,b]`) and no grouping with
  parentheses.
- Text comparison follows the database: values are compared bytewise, free
  text matching is case-insensitive.
