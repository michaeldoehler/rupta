# Architecture

rupta is a single Rust binary that embeds its web app and its database.
This chapter explains the parts and the reasons behind them.

```text
  Sentry SDKs (browser, Node, PHP, Python, …)   users, sentry-cli, scripts
        │ POST /api/{project}/envelope/                │ /api/v1, /api/0, web app
        ▼                                              ▼
 ┌────────────────────────────┐            ┌────────────────────────────┐
 │ ingest endpoints           │            │ REST API + auth            │
 │ DSN auth, limits, decode   │            │ sessions, SSO, tokens,     │
 │ → bounded queue            │            │ roles, OpenAPI             │
 └──────────────┬─────────────┘            └──────────────┬─────────────┘
                ▼                                         │
 ┌────────────────────────────┐   issue signals   ┌───────┴────────────┐
 │ workers: normalize, scrub, │ ────────────────► │ alert engine,      │
 │ symbolicate, group         │                   │ notifications      │
 └──────────────┬─────────────┘                   └────────────────────┘
                ▼ batches (≤ 1,000 events / 50 ms)
 ┌──────────────────────────────────────────────────────────────────────┐
 │ squad (embedded, PostgreSQL dialect)                                 │
 │ row tables: orgs, users, projects, issues, payloads, …               │
 │ columnar tables: events, transactions, spans, logs, metrics          │
 └──────────────┬───────────────────────────────────────────────────────┘
                ▼ every commit (sync) or every second (async)
        S3-compatible bucket (Garage): durable copy, restore source
```

## Crates

| Crate | Responsibility |
| --- | --- |
| `rupta-core` | Shared domain types: ids, roles, project settings, the normalized event model, grouping, data scrubbing, the search query language. No I/O. |
| `rupta-store` | Opens the database (local or with a bucket), applies numbered migrations, runs blocking database work on a dedicated pool. |
| `rupta-ingest` | The SDK protocol endpoints, the processing pipeline and the batch writer, source map symbolication, retention. |
| `rupta-auth` | Accounts, password hashing, sessions, CSRF, OpenID Connect, API tokens, role checks, audit log. |
| `rupta-api` | REST API, the upload-CLI subset, alert rules and notification channels. |
| `rupta-server` | Configuration, wiring, HTTP server, embedded web app, operational endpoints, graceful shutdown. |

## Why an embedded database

Error tracking writes much more than it reads, and its reads are of two
kinds: point lookups and small transactions (an issue, its status, a
user's session) and aggregations over many events (counts over time, tag
distributions, search). rupta uses [squad](https://github.com/michaeldoehler/squad),
an embedded database with PostgreSQL's SQL dialect that offers both:
ordinary row tables with transactions for the first kind and columnar
tables for the second. Embedding it removes the network hop and the
operational burden of a separate database cluster, message queue and
analytics store: a single process does the work of several services.

The trade-off is one writer per database. rupta scales up (more cores,
faster disk) rather than out, and relies on the bucket for durability and
on rescheduling for availability.

## Ingestion path

1. **Accept.** The handler authenticates the DSN key against an in-memory
   cache, checks size limits and origin, decompresses, splits the envelope
   into items and pushes them onto a bounded queue. It answers immediately
   (`RUPTA_DURABILITY=fast`) or after the item is written (`safe`). A full
   queue answers 429, so overload never exhausts memory.
2. **Process.** Worker threads normalize each event into one canonical
   shape, apply inbound filters and data scrubbing, symbolicate JavaScript
   frames with uploaded source maps, compute the grouping fingerprint and
   resolve or create the issue.
3. **Write.** A single batch writer collects processed items for up to
   50 ms or 1,000 items and writes them in one transaction: event rows
   appended to the columnar `events` table, compressed payloads, issue
   counters aggregated per issue, session counts, outcomes. One commit per
   batch makes the durable round trip to the bucket affordable.
4. **Signal.** New issues, regressions and event counts are published to
   the alert engine.

## Data model

- `issues` (row table): one row per group, with status, assignment,
  counters and first/last seen. Updated in place by the batch writer.
- `events` (columnar): one row per event with the searchable attributes and
  tags; ordered by project and time, so queries over a project and a time
  range read only the matching segments.
- `event_payloads` (row table): the full normalized event, zstd-compressed,
  fetched by id for the event view.
- Attachments and source map bundles: in the object store (the bucket, or
  a local directory without one).

## Web app

React 19 with TypeScript, Tailwind CSS 4 and shadcn/ui, routed by TanStack
Router and fed by TanStack Query. It is built to static files and embedded
into the binary at compile time, so a release is one artifact. It uses only
the public REST API.
