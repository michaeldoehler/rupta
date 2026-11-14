# Comparison

How rupta relates to the established self-hosted options for error tracking
with the same SDKs. This is the maintainers' reading of public
documentation as of November 2026; features change, so check each
project's own documentation before deciding. Corrections are welcome.

| | rupta 0.1 | Sentry (self-hosted) | GlitchTip |
| --- | --- | --- | --- |
| License | MIT | Functional Source License (each release becomes Apache 2.0 after two years) | MIT |
| Works with the official open-source SDKs | yes (errors, messages, sessions, attachments, feedback, cron check-ins; see [SDK compatibility](sdk-compatibility.md)) | yes (reference implementation) | yes (errors and a subset of other data) |
| Components to operate | 1 container (+ an S3 bucket for durability) | many containers (Kafka, ClickHouse, PostgreSQL, Redis, relay and others) | web, worker, PostgreSQL, Redis/Valkey |
| Minimum resources (vendor guidance) | 1 core, 1 GiB | 4 cores, 16 GiB RAM | small (Django + PostgreSQL) |
| Storage | embedded database, durable copy in S3/Garage | PostgreSQL, ClickHouse, Kafka, object storage | PostgreSQL |
| Error issues, grouping, stack traces, breadcrumbs | yes | yes | yes |
| JavaScript source maps | yes (upload CLI, debug ids) | yes | yes |
| Release health (sessions) | crash-free sessions per release | yes, more detail | limited |
| Cron monitoring | yes | yes | yes (uptime/heartbeat) |
| Performance tracing UI | no (transactions are stored, not shown) | yes | basic |
| Session replay, profiling | no (accepted and dropped) | yes | no |
| Single sign-on | OpenID Connect (Zitadel, Keycloak, Entra ID, …), role mapping, enforcement per organization | SAML 2.0 and OAuth providers | OpenID Connect and social logins |
| SCIM provisioning | no | yes (paid plans of the hosted service) | no |
| High availability | one writer, restore from bucket on another node | horizontally scalable components | horizontally scalable web/workers |
| REST API | yes, OpenAPI 3.1 | yes | yes |

## When to choose what

- **rupta** fits teams that want error tracking with the official SDKs at
  very low operational cost — one container, a bucket, SSO through their
  existing identity provider — and do not need tracing, replays or
  profiling.
- **Sentry** fits teams that want the complete product surface and can run
  (or pay for) the infrastructure it needs.
- **GlitchTip** fits teams that prefer a conventional Django + PostgreSQL
  stack they already know how to operate.
