# Configuration reference

rupta is configured entirely through environment variables. There is no
configuration file. Invalid values stop the server at start with a message
naming the variable.

With Docker Compose, the variables are read from `.env`; see
[deployment](deployment.md#docker-compose-single-host).

**Secrets from files.** Every variable marked *secret* can instead be given
as `<NAME>_FILE` containing the path of a file with the value, as Docker and
Kubernetes secrets provide them (`RUPTA_SECRET_KEY_FILE=/run/secrets/rupta_secret_key`).
The file wins over the plain variable; trailing whitespace and newlines are
removed.

**Units.** Durations are in seconds unless the name says otherwise, sizes in
mebibytes (`_MB`). Underscores in numbers are ignored (`100_000`).

## Server

| Variable | Default | Description |
| --- | --- | --- |
| `RUPTA_LISTEN` | `0.0.0.0:8080` | Address and port the HTTP server binds to. |
| `RUPTA_PUBLIC_URL` | `http://localhost:<port>` | The URL users and SDKs reach rupta under, without trailing slash. DSNs, single sign-on redirect URIs and links in notifications are built from it. Session cookies are marked `Secure` when it starts with `https://`. |
| `RUPTA_SECRET_KEY` | — (**required**, *secret*) | At least 32 characters. Signs cookies and encrypts secrets stored in the database (for example SSO client secrets). Changing it logs everybody out and makes stored secrets unreadable. Generate with `openssl rand -hex 32`. |
| `RUPTA_TRUSTED_PROXIES` | empty | Comma-separated IP addresses or CIDR ranges of reverse proxies. `X-Forwarded-For` is honoured only for requests from them, for the client IP shown on events and used for rate limits and audit logs. |
| `RUPTA_CORS_ALLOWED_ORIGINS` | empty | Origins allowed to call the REST API from browsers other than the web app itself; browser requests to `/mcp` from other origins are refused. SDK ingest endpoints have their own per-project setting. |
| `RUPTA_SHUTDOWN_TIMEOUT_SECS` | `30` | On SIGTERM, how long to wait for queued events to be written. |
| `RUPTA_LOG` | `info` | Log filter (`error`, `warn`, `info`, `debug`, or per module, e.g. `info,rupta_ingest=debug`). |
| `RUPTA_LOG_FORMAT` | `text` (`json` in the image) | `text` or `json` (one object per line). |
| `RUPTA_METRICS_TOKEN` | empty (*secret*) | If set, `GET /metrics` requires `Authorization: Bearer <token>`. See [monitoring](monitoring.md). |
| `RUPTA_MCP` | `true` | Serve the Model Context Protocol endpoint `/mcp` and its OAuth protected resource metadata under `/.well-known/oauth-protected-resource`. `false` answers `404` there. See [MCP server](mcp.md). |
| `RUPTA_METRICS_PROJECT_LABELS` | `50` | Projects with their own labels in `rupta_project_events_total` and `rupta_project_new_issues_total`: the first ones that receive events after the start. All others are counted as `project="other"`. `0` puts every project in `other`. |

## Storage

rupta stores everything in an embedded database in `RUPTA_DATA_DIR`. With
`RUPTA_STORAGE_URL` set, the local directory becomes a cache and every
commit is also stored in an S3-compatible bucket, from which a lost volume
is restored automatically. See [deployment](deployment.md).

| Variable | Default | Description |
| --- | --- | --- |
| `RUPTA_DATA_DIR` | `/data` | Local directory for the database (`db/`) and, without a bucket, for attachments and uploaded source maps (`objects/`). |
| `RUPTA_STORAGE_URL` | empty | Durable copy in an object store: `s3://<bucket>/<prefix>?endpoint=<url>&region=<region>&path_style=true`. For Garage: `s3://rupta/prod?endpoint=http://garage:3900&region=garage&path_style=true`. Empty: local disk only. |
| `RUPTA_S3_ACCESS_KEY_ID` | empty (*secret*) | Access key for the bucket; added to the URL. |
| `RUPTA_S3_SECRET_ACCESS_KEY` | empty (*secret*) | Secret key for the bucket; added to the URL. |
| `RUPTA_REMOTE_SYNC` | `sync` | `sync`: a write is acknowledged only after it is in the bucket, so losing the machine loses nothing that was stored. `async`: upload once per second; up to about a second of writes can be lost with the machine. |
| `RUPTA_CACHE_MB` | `256` | Memory for the database page cache. |
| `RUPTA_RETENTION_DAYS` | `90` | Days events, attachments and statistics are kept. Projects can set a shorter period. |

## Accounts and single sign-on

| Variable | Default | Description |
| --- | --- | --- |
| `RUPTA_ADMIN_EMAIL` | empty | With `RUPTA_ADMIN_PASSWORD`: creates this superuser, the organization `RUPTA_DEFAULT_ORG` and makes the user its owner, when the database has no users yet. Ignored afterwards. Without it, the first visitor completes a setup screen. |
| `RUPTA_ADMIN_PASSWORD` | empty (*secret*) | Password of the bootstrap administrator. |
| `RUPTA_DEFAULT_ORG` | `default` | Slug of the organization created at bootstrap; users provisioned by instance-wide single sign-on join it. |
| `RUPTA_LOCAL_LOGIN` | `true` | Allow login with email and password. `false` requires single sign-on to be configured. |
| `RUPTA_OIDC_ISSUER` | empty | Issuer URL of an OpenID Connect provider for instance-wide single sign-on, e.g. `https://auth.example.com` (Zitadel). Enables SSO. |
| `RUPTA_OIDC_CLIENT_ID` | — | Client id of the application registered at the provider. Required with the issuer. |
| `RUPTA_OIDC_CLIENT_SECRET` | — (*secret*) | Client secret. Required with the issuer. |
| `RUPTA_OIDC_NAME` | `Single sign-on` | Label of the login button, e.g. `Zitadel`. |
| `RUPTA_OIDC_SCOPES` | `openid email profile urn:zitadel:iam:org:project:roles` | Space-separated scopes. The last one asks Zitadel to include project roles. |
| `RUPTA_OIDC_ROLE_CLAIM` | `urn:zitadel:iam:org:project:roles` | Claim holding the user's roles: an object whose keys are role names (Zitadel), a list of strings, or a string. |
| `RUPTA_OIDC_ROLE_MAPPING` | empty | Comma-separated `provider-role:rupta-role` pairs, e.g. `rupta-admin:admin,rupta-viewer:viewer`. rupta roles: `owner`, `admin`, `member`, `viewer`. The highest mapped role wins and is applied at every login. Owners are never demoted by SSO, and a member whose claims map to no role keeps their current role. |
| `RUPTA_OIDC_DEFAULT_ROLE` | `member` | Role given to new members whose claims map to no role. `none` refuses users whose claims map to no role: no account is created for them, and existing members without a mapped role cannot log in through SSO (owners still can). Needs `RUPTA_OIDC_ROLE_MAPPING`. See [single sign-on](sso.md#users-and-roles). |
| `RUPTA_OIDC_ALLOWED_AUDIENCES` | empty | Comma-separated audiences that ID tokens may carry besides the client id (for ZITADEL: the project id). Empty accepts any additional audience. Tokens with several audiences must have `azp` equal to the client id either way. See [single sign-on](sso.md#audiences). |
| `RUPTA_OIDC_JIT` | `true` | Create accounts for new users at their first login. With `false`, only users that already exist (invited) can log in through SSO. |
| `RUPTA_SSO_ALLOW_PRIVATE_ISSUERS` | `false` | Let organization SSO settings (the connection test and saving) use issuers whose host resolves to loopback, private, link-local or other internal addresses. Without it only the host of `RUPTA_OIDC_ISSUER` may be internal. Turn it on when organization providers run on the internal network. See [single sign-on](sso.md#testing-the-connection). |

The redirect URI to register at the provider is
`<RUPTA_PUBLIC_URL>/auth/sso/oidc/callback`. Organizations can configure
their own provider in the settings in addition; see [single sign-on](sso.md).

## Integrations

| Variable | Default | Description |
| --- | --- | --- |
| `RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS` | `false` | Let issue tracker integrations (Jira Data Center, self-managed GitLab, …) notification channels (Slack, Teams and webhook URLs, PagerDuty/Opsgenie with a custom `api_url`) and the favicon fetch for organization icons reach hosts that resolve to loopback, private, link-local or other internal addresses. Without it such hosts are refused, so that organization admins cannot make the server call internal services. Turn it on when your trackers run on the internal network and all organization admins are trusted. See [integrations](integrations.md). |

## Email

| Variable | Default | Description |
| --- | --- | --- |
| `RUPTA_SMTP_URL` | empty (*secret*) | SMTP server for invitations, alert emails and workflow emails (an issue assigned to you, a comment on an issue assigned to you; each user can turn these off in their preferences): `smtps://user:password@host:465` (TLS) or `smtp://user:password@host:587` (STARTTLS). Without it (and without `RUPTA_CAMELMAILER_API_KEY`), emails are not sent and invitations show a link to copy. Set either this or `RUPTA_CAMELMAILER_API_KEY`, not both. |
| `RUPTA_CAMELMAILER_API_KEY` | empty (*secret*) | Send the same emails through the HTTP API of a Camelmailer server instead of SMTP: the server API key (`cm_…`). |
| `RUPTA_CAMELMAILER_URL` | the hosted service | Base URL of a self-hosted Camelmailer server, e.g. `https://mail.example.com`. |
| `RUPTA_MAIL_FROM` | — | Sender of all emails, e.g. `rupta <rupta@example.com>`. Required with `RUPTA_SMTP_URL` or `RUPTA_CAMELMAILER_API_KEY`. `RUPTA_SMTP_FROM` is still read when this is not set. |

## Ingestion

| Variable | Default | Description |
| --- | --- | --- |
| `RUPTA_INGEST_WORKERS` | number of CPU cores | Threads that process events (normalization, scrubbing, symbolication, grouping). |
| `RUPTA_INGEST_QUEUE` | `100000` | Events waiting for processing. When full, SDKs receive `429 Too Many Requests` with `Retry-After` and resend later. |
| `RUPTA_DURABILITY` | `fast` | `fast`: an event is acknowledged once it is queued; events still in the queue are lost if the process is killed. `safe`: acknowledged only after it is written, at the cost of latency. See [ingestion](ingestion.md). |
| `RUPTA_MAX_ENVELOPE_MB` | `20` | Largest request body an SDK may send, compressed. |
| `RUPTA_MAX_DECOMPRESSED_MB` | `100` | Largest request body after decompression. |

## Commands

The binary takes one optional command:

| Command | Description |
| --- | --- |
| `rupta serve` (default) | Runs the server. |
| `rupta healthcheck` | Exits 0 if `GET /readyz` on the local port answers 200. Used by the container health check. |
| `rupta version` | Prints the version. |
