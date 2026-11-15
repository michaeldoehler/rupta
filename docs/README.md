# rupta documentation

rupta is self-hosted error tracking and application monitoring in a single
container. Applications send to it with the open-source **Sentry SDKs**
and upload source maps with **`sentry-cli`**; an application that already
uses them switches by changing its DSN.

> Sentry is a trademark of Functional Software, Inc. rupta is an
> independent project and is not affiliated with or endorsed by Sentry.

If you are new, read the chapters of *Get started* in order, then pick
what you need from the other sections.

## Get started

1. [Getting started](getting-started.md): run rupta with Docker Compose,
   create a project, send a first error with `curl` and with an SDK, move
   an existing application, upload source maps. About five minutes.
2. [SDK compatibility](sdk-compatibility.md): which Sentry SDKs and data
   types work, tested versions, what is not supported.
3. [Deployment](deployment.md): Docker Compose, Docker Swarm, the bucket,
   TLS, sizing.
4. [Configuration](configuration.md): every environment variable.

## Use

- [User guide](user-guide.md): a tour of the web app, keyboard shortcuts.
- [Search](search.md): the query language of the issue list.
- [Alerts](alerts.md): rules and notification channels.
- [Performance](performance.md): transactions, traces and spans.
- [Logs](logs.md), [Metrics](metrics.md), [Profiling](profiling.md),
  [Session replay](session-replay.md).
- [Source maps](sourcemaps.md): readable JavaScript stack traces.
- [Grouping](grouping.md): how events become issues.
- [Data scrubbing](data-scrubbing.md): what is removed before storage.

## Operate

- [Operations](operations.md): health checks, backup and restore,
  upgrades, troubleshooting.
- [Monitoring](monitoring.md): Prometheus metrics, alert rules, Grafana.
- [Security](security.md): threat model and hardening.
- [Authentication](authentication.md): accounts, sessions, roles, API
  tokens.
- [Single sign-on](sso.md): OpenID Connect, step by step with Zitadel.

## Integrate

- [Source map upload](sourcemap-upload.md): `sentry-cli` commands that
  work, CI example.
- [Integrations](integrations.md): Jira, Linear, GitHub and GitLab tickets
  with status sync.
- [REST API](api.md) and the [OpenAPI description](api/openapi.json).
- [MCP server](mcp.md): connect agents and other MCP clients with an API
  token.

## Reference

- [Ingestion](ingestion.md): the pipeline, durability, rate limits,
  issue states, retention, throughput.
- [Event fields](event-fields.md): the event document of the API.
- [Architecture](architecture.md): the parts and the reasons behind them.
- [Comparison](comparison.md): rupta, Sentry (self-hosted) and GlitchTip.
- [Changelog](../CHANGELOG.md), [contributing](../CONTRIBUTING.md),
  [reporting a vulnerability](../SECURITY.md).
