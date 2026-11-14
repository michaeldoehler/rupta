# Comparison

How rupta relates to the established self-hosted options that work with
the Sentry SDKs: Sentry itself (self-hosted) and GlitchTip. All three
receive data from the same open-source Sentry SDKs, so an application can
move between them by changing its DSN. This is the maintainers' reading of
public documentation as of November 2026; features change, so check each
project's own documentation before deciding. Corrections are welcome.

> Sentry is a trademark of Functional Software, Inc. rupta is an
> independent project and is not affiliated with or endorsed by Sentry.

| | rupta 0.1 | Sentry (self-hosted) | GlitchTip |
| --- | --- | --- | --- |
| License | MIT | Functional Source License (each release becomes Apache 2.0 after two years) | MIT |
| Works with the Sentry SDKs | yes: errors, transactions and spans, logs, metrics, profiles, browser replays, sessions, attachments, feedback, cron check-ins; tested end to end with JavaScript (browser, Node.js), PHP and Python, other SDKs expected to work ([matrix](sdk-compatibility.md#compatibility-matrix)) | yes (the SDKs are made for it) | yes (errors and a subset of other data) |
| Native crash reports, mobile replay | no | yes | no |
| `sentry-cli` source map upload | yes (artifact bundles, releases, deploys; no commits) | yes | yes |
| Components to operate | 1 container (+ an S3 bucket for durability) | many containers (Kafka, ClickHouse, PostgreSQL, Redis, relay and others) | web, worker, PostgreSQL, Redis/Valkey |
| Minimum resources (vendor guidance) | 1 core, 1 GiB | 4 cores, 16 GiB RAM | small (Django + PostgreSQL) |
| Storage | embedded database, durable copy in S3/Garage | PostgreSQL, ClickHouse, Kafka, object storage | PostgreSQL |
| Error issues, grouping, stack traces, breadcrumbs | yes | yes | yes |
| JavaScript source maps | yes (debug ids, release and dist) | yes | yes |
| Release health (sessions) | crash-free sessions per release | yes, more detail | limited |
| Cron monitoring | yes | yes | yes (uptime/heartbeat) |
| Performance tracing | transactions, percentiles, Web Vitals, trace waterfall, span explorer | yes | basic |
| Logs | search, facets, live tail | yes | no |
| Custom metrics | explorer and dashboards | yes | no |
| Profiling | flamegraphs (JavaScript, Node.js, Python, PHP) | yes, more platforms | no |
| Session replay | browser (rrweb); mobile replay not supported | browser and mobile | no |
| Single sign-on | OpenID Connect (Zitadel, Keycloak, Entra ID, …), role mapping, enforcement per organization | SAML 2.0 and OAuth providers | OpenID Connect and social logins |
| SCIM provisioning | no | yes (paid plans of the hosted service) | no |
| High availability | one writer, restore from bucket on another node | horizontally scalable components | horizontally scalable web/workers |
| REST API | yes, OpenAPI 3.1 | yes | yes |

## When to choose what

- **rupta** fits teams that want error tracking and application
  monitoring with the Sentry SDKs at very low operational cost — one
  container, a bucket, SSO through their existing identity provider — and
  whose load fits one node.
- **Sentry** fits teams that want the complete product surface and can run
  (or pay for) the infrastructure it needs.
- **GlitchTip** fits teams that prefer a conventional Django + PostgreSQL
  stack they already know how to operate.
