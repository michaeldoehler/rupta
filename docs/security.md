# Security

How rupta protects data, and how to run it securely. To report a
vulnerability, see [SECURITY.md](../SECURITY.md).

## Threat model in brief

rupta receives data from applications (SDKs, unauthenticated except for
the public DSN key, which ships inside web pages and apps) and serves it to
authenticated users. The ingest endpoints are therefore exposed to the
internet and treat all input as hostile; the management API and web app
require authentication and enforce roles.

## Ingestion

- The DSN public key only allows sending events to its project. It cannot
  read anything. Rotate or deactivate keys in the project settings.
- Request bodies are size-limited before and after decompression
  (`RUPTA_MAX_ENVELOPE_MB`, `RUPTA_MAX_DECOMPRESSED_MB`), parsing is
  bounded, and malformed input is rejected without affecting other events.
- Browser submissions are restricted by the project's allowed origins.
- Rate limits per project and per key protect against floods; a full queue
  answers 429 instead of exhausting memory.
- **Data scrubbing** runs on the server before anything is stored: values of
  keys that look like passwords, secrets, tokens, API keys, authorization
  headers, cookies and session ids, and strings that look like credit card
  numbers, are replaced. Projects can add fields, exempt fields and stop
  storing IP addresses. See [data scrubbing](data-scrubbing.md). Prefer
  scrubbing in the SDK as well (`send_default_pii` off, `beforeSend`).

## Authentication and authorization

- Passwords are hashed with Argon2id. Failed logins are rate-limited per
  account and per IP address.
- Sessions use random 256-bit tokens in an `HttpOnly`, `SameSite=Lax`
  cookie (`Secure` with an `https://` public URL); the database stores only
  their SHA-256. Mutating requests with a session cookie require a CSRF
  token.
- API tokens are random, shown once, stored as SHA-256, scoped, optionally
  expiring, and can be revoked.
- Roles per organization (viewer, member, admin, owner) are enforced on
  every API request. Resources of other organizations answer 404, not 403,
  so their existence does not leak.
- Single sign-on uses OpenID Connect with PKCE, state and nonce, and
  validates ID tokens against the provider's published keys. Organizations
  can enforce SSO for their members.
- Security-relevant actions are recorded in the audit log: logins and
  failed logins, role and membership changes, token and key changes, SSO
  configuration, project deletion.

## Secrets at rest

- `RUPTA_SECRET_KEY` signs cookies and encrypts secrets stored in the
  database (SSO client secrets, webhook secrets) with an authenticated
  cipher. Keep it in a secret store; losing it logs everybody out and makes
  stored secrets unreadable.
- Event data is stored unencrypted in the embedded database and the bucket.
  Use encrypted volumes and server-side encryption of the bucket where
  required, and restrict access to both.

## Running rupta securely

- Terminate TLS at a reverse proxy; set `RUPTA_PUBLIC_URL` to the `https://`
  URL and `RUPTA_TRUSTED_PROXIES` to the proxy so client IPs are not
  spoofable.
- Expose only port 8080 through the proxy. `/metrics` should be internal or
  protected with `RUPTA_METRICS_TOKEN`.
- Give the bucket key access to the rupta bucket only.
- Enable SSO and disable local login (`RUPTA_LOCAL_LOGIN=false`) once SSO
  works, keeping one owner account for emergencies if your policy allows.
- The container runs as an unprivileged user (uid 10001) on a minimal
  Debian base; images are published with provenance attestations and an
  SBOM.
- Keep up with releases; security fixes are announced as GitHub security
  advisories.
