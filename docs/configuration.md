# Configuration reference

rupta is configured entirely through environment variables. There is no
configuration file. Invalid values stop the server at start with a message
naming the variable.

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
| `RUPTA_CORS_ALLOWED_ORIGINS` | empty | Origins allowed to call the REST API from browsers other than the web app itself. SDK ingest endpoints have their own per-project setting. |
| `RUPTA_SHUTDOWN_TIMEOUT_SECS` | `30` | On SIGTERM, how long to wait for queued events to be written. |
| `RUPTA_LOG` | `info` | Log filter (`error`, `warn`, `info`, `debug`, or per module, e.g. `info,rupta_ingest=debug`). |
| `RUPTA_LOG_FORMAT` | `text` (`json` in the image) | `text` or `json` (one object per line). |
| `RUPTA_METRICS_TOKEN` | empty (*secret*) | If set, `GET /metrics` requires `Authorization: Bearer <token>`. |

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
| `RUPTA_OIDC_DEFAULT_ROLE` | `member` | Role given to new members whose claims map to no role. |
| `RUPTA_OIDC_JIT` | `true` | Create accounts for new users at their first login. With `false`, only users that already exist (invited) can log in through SSO. |

The redirect URI to register at the provider is
`<RUPTA_PUBLIC_URL>/auth/sso/oidc/callback`. Organizations can configure
their own provider in the settings in addition; see [single sign-on](sso.md).

## Email

| Variable | Default | Description |
| --- | --- | --- |
| `RUPTA_SMTP_URL` | empty (*secret*) | SMTP server for invitations and alert emails: `smtps://user:password@host:465` (TLS) or `smtp://user:password@host:587` (STARTTLS). Without it, emails are not sent and invitations show a link to copy. |
| `RUPTA_SMTP_FROM` | — | Sender, e.g. `rupta <rupta@example.com>`. Required with `RUPTA_SMTP_URL`. |

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
