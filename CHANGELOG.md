# Changelog

All notable changes to rupta are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
uses [semantic versioning](https://semver.org/). Before 1.0, minor versions
may change the API, the configuration and the storage format; the
changelog says when and how to migrate.

## [Unreleased]

### Documentation

- Name the compatibility explicitly: rupta implements the ingestion
  protocol of the Sentry SDKs and the source map upload commands of
  `sentry-cli`. `docs/sdk-compatibility.md` has a compatibility matrix per
  SDK and data type (tested, expected to work, accepted but not stored, not
  supported) and a list of what is not supported; README, source map
  upload and comparison say the same, with a trademark note.
- `docs/getting-started.md`: a five-minute path from Docker Compose to the
  first error, with `curl` and SDK examples for JavaScript, Node.js, PHP,
  Laravel, Python and Django, moving an existing application, and source
  map upload. `docs/README.md`: a documentation index with a reading order.

## [0.1.0] - 2026-11-14

The first release.

### Added

- **Ingestion** compatible with the official open-source SDKs: the
  envelope, store and security endpoints with DSN authentication, gzip,
  deflate, brotli and zstd bodies, CORS for browsers, rate limits per
  project and key, back-pressure with `429` and `Retry-After`, and two
  durability modes. Every envelope item type is accepted; see
  `docs/sdk-compatibility.md` for what is stored. Tested end to end with
  the JavaScript (browser and Node.js), PHP and Python SDKs.
- **Error tracking**: normalization of all common event interfaces,
  server-side data scrubbing, inbound filters, grouping by fingerprint,
  exception and in-app frames, issues with short ids, regression,
  ignore conditions, resolution in a release, escalation detection,
  comments, assignment, bookmarks, and a search language.
- **JavaScript source map symbolication** with bundles uploaded by the
  official upload CLI (debug ids and release artifacts), including the
  CLI's chunk upload, release and deploy endpoints.
- **Performance monitoring**: transactions and spans, throughput,
  percentiles, failure rate, Apdex, Web Vitals, trace waterfall, span
  explorer.
- **Logs** with search, facets and live tail; **metrics** explorer and
  dashboards; **profiling** with flamegraphs; **session replay** for
  browsers; **release health**; **cron monitors**; **user feedback**;
  **attachments**.
- **Alerts** on new issues, regressions, reappearance, escalation, event
  and user frequency, delivered by email, Slack, Microsoft Teams and signed
  webhooks.
- **Accounts and access**: local accounts with Argon2id, sessions with
  CSRF protection, OpenID Connect single sign-on with Zitadel role mapping
  and per-organization enforcement, organizations, teams, four roles,
  invitations, scoped API tokens, audit log.
- **REST API** with an OpenAPI 3.1 description.
- **Web app** in React with a dark interface, top navigation, issue preview
  and context panels, keyboard navigation and
  a command palette, embedded in the server binary.
- **Storage** on the embedded squad database with a durable copy in any
  S3-compatible bucket (Garage, AWS S3, MinIO), automatic restore of a
  lost volume, writer fencing, retention per project and data type.
- **Operations**: configuration by environment variables with `_FILE`
  secrets, liveness, readiness and Prometheus metrics endpoints, graceful
  shutdown, a container image running as an unprivileged user, a Docker
  Compose stack with Garage, a Docker Swarm stack, CI and release
  workflows with multi-architecture images and provenance attestations.
- Issue tracker integrations for Jira Cloud (REST v3), Jira Data Center
  and Server (REST v2), Linear (GraphQL), GitHub Issues and GitLab Issues
  (REST v4): `/api/v1/orgs/{org}/integrations` with connection test and
  target lists, per-project defaults, and issue links
  (`/api/v1/issues/{id}/links`) that create a ticket with a generated
  description or link an existing one. Verified webhooks at
  `/api/integrations/{kind}/{id}/webhook` keep links current and, when
  enabled per project, resolve and unresolve issues; resolving,
  unresolving and regressions comment on and transition linked tickets.
  Tokens and webhook secrets are stored encrypted and never returned.
  See docs/integrations.md.
- `RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS` allows integrations on
  internal addresses (Jira Data Center, self-managed GitLab); by default
  they are refused.
- Alert rule action `create_ticket` (once per issue and integration).
- Notification channels `pagerduty` (Events API v2, the issue id as dedup
  key) and `opsgenie` (Alert API v2, US or EU); their incidents are
  resolved when the issue is resolved.
- `/metrics`: request count and latency histogram per route template and
  status class, error events and new issues per project (capped by
  `RUPTA_METRICS_PROJECT_LABELS`, default 50), issues by status, readiness,
  alert notifications per channel kind and result, integration calls,
  webhooks and sync results, database size, cache hit ratio, object store
  positions, and process metrics. Existing metric names are unchanged.
- `deploy/prometheus` (scrape configuration and alert rules with promtool
  tests), `deploy/grafana` (dashboard and provisioning) and
  `compose.monitoring.yaml`. See docs/monitoring.md.
- `GET /api/v1/orgs/{org}/annotations`: releases, deploys and new-issue
  spikes as JSON annotations for Grafana.

- `GET /api/v1/orgs/{org}/issues/by-short-id/{short_id}` finds an issue by
  its short id (`WEB-APP-31`), case-insensitively.
- Resolving an issue "in the next release" (`resolved_in: next_release`
  or `substatus: in_next_release`): the project's latest release is
  recorded and only events of a release first seen afterwards regress the
  issue. Resolving in a specific release now regresses for that release
  and releases first seen after it.
- Ignoring an issue until N more users are affected
  (`ignore_user_count`, optionally within `ignore_user_window` minutes).
- Escalation detection: a check every five minutes marks issues
  `escalating` (more than 10× their hourly 7-day average and at least 50
  events in the last hour), returns them to `ongoing` after a calm day,
  and moves issues older than a week from `new` to `ongoing`. Alert rules
  have the condition `escalating`; issues ignored with substatus
  `archived_until_escalating` come back when they escalate.
- Crash-free users in release health, from the distinct ids (`did`) of
  sessions; the health series carry user counts.
- Artifact bundles of a project can be listed and deleted
  (`/api/v1/projects/{org}/{project}/artifact-bundles`); upload chunks
  older than 24 hours are deleted by the hourly maintenance.
- `POST /api/v1/orgs/{org}/sso/test` checks an identity provider's
  discovery document before the settings are saved, with protection
  against internal addresses (`RUPTA_SSO_ALLOW_PRIVATE_ISSUERS`).
- User preferences on the server (`preferences` on `GET/PATCH
  /api/v1/user`): time zone, clock, theme and email switches. Alert rule
  emails respect `issue_alerts`; new workflow emails (issue assigned to
  you or your team, comment on an issue assigned to you) respect
  `workflow`.
