# Changelog

All notable changes to rupta are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
uses [semantic versioning](https://semver.org/). Before 1.0, minor versions
may change the API, the configuration and the storage format; the
changelog says when and how to migrate.

## [0.3.2] - 2026-11-25

### Security

- **Mail notifications: lettre 0.11.24.** Fixes RUSTSEC-2026-0560 to
  -0565: a man-in-the-middle could forge SMTP replies after STARTTLS, a
  bare CR or LF could inject SMTP commands, a quoted local part sent the
  message to a different address, and malformed replies or display names
  could crash or exhaust memory.
- url 2.5.9 (RUSTSEC-2026-0592, stack overflow on nested `blob:` URLs).

### Changed

- **The container image runs on distroless Debian 13** instead of
  debian:bookworm-slim: glibc, CA certificates and tzdata, no shell or
  package manager. Scanners reported 1 critical and 19 high findings in
  the old base image, all in packages rupta does not use. It still runs as
  uid 10001, so existing `/data` volumes need no change. `docker exec ... sh`
  no longer works; use `rupta` subcommands instead.
- Dependency updates (Rust and web).

## [0.3.1] - 2026-11-17

### Fixed

- **Memory no longer grows until the container is killed.** The embedded
  database rewrote every columnar table about every 100 seconds, kept the
  directories of up to 4096 replaced segments in memory and started new
  threads for every parallel read; with glibc's allocator the process
  grew by about 500 MiB per hour under steady ingest. The database update
  (squad) merges segments by size tier, bounds its caches in bytes and
  drops deleted segments from them, and rupta now uses mimalloc as its
  allocator.
- **Restores after a move to another node are short again.** Snapshots
  of the database in the object store are now taken by time and by the
  number of log objects too, and the counter no longer starts at zero on
  every start; a restore loads snapshot chunks and log objects in
  parallel and logs its progress.
- **The database's own messages reach rupta's log.** Restore progress
  (every 5 s) and snapshots (with their trigger) are
  logged by squad and now appear in rupta's log output, so a long start is
  no longer silent.

### Added

- `RUPTA_COLUMNAR_CACHE_MB` (default 128): memory for cached data of the
  columnar tables. Before, the database's default of 512 MiB applied.
- Metrics `rupta_storage_columnar_cache_bytes{cache}` and
  `rupta_storage_columnar_retired_segments`.

## [0.3.0] - 2026-11-15

### Added

- An organization can have an icon: when an admin saves a website domain
  in *Organization settings → General*, the server fetches the site's
  favicon and shows it in the organization menu. `PATCH
  /api/v1/orgs/{org}` takes `domain`; organizations and the session's
  organizations carry `domain` and `icon_url`; `GET
  /api/v1/orgs/{org}/icon` serves the image. Migration 6000 adds the
  columns.

### Changed

- Projects show the mark of their platform (React, Next.js, Node.js,
  Python, Django, Flask, FastAPI, PHP, Laravel, Symfony, Go, Java, .NET,
  Ruby, Rust, JavaScript, Vue) instead of a two-letter badge, and Slack and
  Microsoft Teams channels show their logos instead of a letter. The marks
  are bundled from simple-icons (CC0); see
  `web/src/components/brand/NOTICE.md`.
- The settings pages use more of the screen width; the project header and
  the page content share one left edge, and the project tabs wrap instead
  of scrolling.
- The channel type picker is wider so that names and descriptions fit.
- All service marks (trackers, channels) sit on the same light tile, and
  picker tiles line up their titles.
- The top bar's logo is centred; avatars from identity providers keep
  their aspect ratio and fit their circle.

### Fixed

- Release lists showed a placeholder instead of each project's platform
  icon: the API now returns `platform` with a release's projects.
- Counts read "1 events", "1 users", "1 values" and "1 need attention";
  ages of 360 to 364 days read "0y ago".
- On phones the settings navigation drew labels on top of each other; it
  now scrolls sideways and keeps the current page in view. Tab rows fade
  out at the edge when more follows.
- The event highlights grid left a cell without borders; the status
  buttons of an issue did not line up with the controls below them.
- Sortable table headers lost their capitals; chart axis labels wrapped
  or collided with the first bar.
- Smaller layout fixes: the event JSON dialog no longer opens with a
  tooltip over the event id; the keyboard shortcut list fits; the
  Activity tab, alert rules, the alert rule editor and feedback start at
  the page edge like their headers; a disabled client key says
  "Disabled"; a disabled cron monitor's check-ins are grey; an
  integration's last error is not shown twice; disabled primary buttons
  look disabled rather than dim; profiles use the project's platform.
- The embedded database (squad) is updated to c85aa97. It adds row-level
  security and read-only logins, and fixes parameter types of the
  backend and advisory lock functions. rupta's schema and behaviour are
  unchanged.

## [0.2.0] - 2026-11-15

### Added

- Email through the HTTP API of a Camelmailer server as an alternative to
  SMTP (`RUPTA_CAMELMAILER_API_KEY`, `RUPTA_CAMELMAILER_URL`), for
  invitations, alert emails and workflow emails. `RUPTA_MAIL_FROM` names
  the sender for both; `RUPTA_SMTP_FROM` still works.

- **MCP server** at `/mcp` (Model Context Protocol, Streamable HTTP,
  stateless, official Rust SDK): 29 tools to list organizations, projects,
  members and teams; search issues with the query language; read issues,
  tag distributions, events with readable stack traces, activity,
  releases and release health, transactions, traces, spans, logs,
  metrics, cron monitors, alert rules and firings, and integrations; and
  to resolve, ignore, assign, bookmark and comment on issues and create or
  link tickets. Tools call the REST API in process as the caller, so token
  scopes, roles and organizations apply unchanged. Resources for the query
  language, event fields and grouping, issue and event resource templates,
  and three prompts (triage, investigation, release check). API tokens
  only (`Authorization: Bearer`), `401` with a `WWW-Authenticate`
  challenge and OAuth protected resource metadata (RFC 9728) at
  `/.well-known/oauth-protected-resource`, browser origin checks.
  `RUPTA_MCP=false` turns it off. See `docs/mcp.md`.
- `docs/event-fields.md`: the fields of the event document.
- `GET /api/v1/projects/{org}/{project}/alert-fires`: the recent firings of
  a project's alert rules (rule, issue, time), newest first, paginated.
- `RUPTA_OIDC_DEFAULT_ROLE=none`, and *No access (refuse sign-in)*
  (`default_role: "none"`) for organization providers: users whose claims
  map to no role are refused at single sign-on, no account is created for
  them, and existing members without a mapped role cannot sign in through
  single sign-on. Owners still sign in. The login page explains the refusal
  (`/login?error=sso_no_access`), and the audit log entry names the
  organization, subject and email. `none` needs a role mapping. Earlier
  versions read a stored `none` as `member`: change the setting before
  downgrading.
- `RUPTA_OIDC_ALLOWED_AUDIENCES`: the audiences besides the client id that
  ID tokens of the instance provider may carry.
- `GET /api/v1/orgs/{org}/sso/instance` (admins and owners): the
  instance-wide provider from the `RUPTA_OIDC_*` variables, read-only and
  without the client secret. The organization's *Single sign-on* settings
  page shows it first; the organization's own provider is an optional,
  collapsible section below it, with an explanation of how the two work
  together.

### Fixed

- Single sign-on refused ZITADEL ID tokens with `aud = [client id,
  project id]` (`... is not a trusted audience`). Additional audiences are
  now accepted (or only those in `RUPTA_OIDC_ALLOWED_AUDIENCES`), and, as
  OpenID Connect Core 1.0 section 3.1.3.7 requires, tokens with several
  audiences must carry `azp` equal to the client id; a present `azp` must
  always be the client id. Organization providers follow the same rules
  without a list of allowed audiences.
- The organization *Single sign-on* page no longer says "Not configured"
  when the instance provider is active.
- The Zitadel preset of the organization settings asked for the scope
  `urn:zitadel:iam:org:projects:roles`; it now uses
  `urn:zitadel:iam:org:project:roles`, like the `RUPTA_OIDC_SCOPES`
  default and the role claim it reads.

## [0.1.1] - 2026-11-14

The first published release. The `v0.1.0` tag was not published: its
container image build failed, so no release and no image exist for it.

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
- **Service marks** of integrated services (Jira, Linear, GitHub, GitLab,
  PagerDuty, Opsgenie, Zitadel) in the integration and channel settings,
  the tracker pickers, linked tickets, the alert editor and the activity
  feed; trackers and channels are chosen from tiles.
- **Deployment templates:** `compose.yaml` passes every configuration
  variable from `.env`; `deploy/stack.yml` creates the bootstrap
  administrator from the `rupta_admin_password` Docker secret, so no setup
  screen is open on a fresh public instance.
- **Container image** for linux/amd64 and linux/arm64, each built on a
  runner of its own architecture, with provenance attestations and an SBOM.

### Documentation

- SDK compatibility stated explicitly: rupta implements the ingestion
  protocol of the Sentry SDKs and the source map upload commands of
  `sentry-cli`. `docs/sdk-compatibility.md` has a matrix per SDK and data
  type (tested, expected to work, accepted but not stored, not supported),
  the tested SDK versions and a list of what is not supported.
- `docs/getting-started.md` (from Docker Compose to the first error, SDK
  examples, moving an existing application, source maps) and
  `docs/README.md` (an index with a reading order), next to the chapters on
  deployment, configuration, operations, security, monitoring, integrations
  and each feature.
