# Changelog

All notable changes to rupta are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
uses [semantic versioning](https://semver.org/). Before 1.0, minor versions
may change the API, the configuration and the storage format; the
changelog says when and how to migrate.

## [Unreleased]

## [0.1.0]

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
- **Web app** in React with light and dark themes, keyboard navigation and
  a command palette, embedded in the server binary.
- **Storage** on the embedded squad database with a durable copy in any
  S3-compatible bucket (Garage, AWS S3, MinIO), automatic restore of a
  lost volume, writer fencing, retention per project and data type.
- **Operations**: configuration by environment variables with `_FILE`
  secrets, liveness, readiness and Prometheus metrics endpoints, graceful
  shutdown, a container image running as an unprivileged user, a Docker
  Compose stack with Garage, a Docker Swarm stack, CI and release
  workflows with multi-architecture images and provenance attestations.
