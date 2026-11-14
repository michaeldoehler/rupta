# Security policy

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x | yes |

Until 1.0, only the latest minor release receives fixes.

## Reporting a vulnerability

Please do not open a public issue. Report vulnerabilities privately through
GitHub: **Security → Report a vulnerability** on
<https://github.com/michaeldoehler/rupta/security/advisories/new>.

Include what you found, how to reproduce it, the version, and the impact you
expect. You will receive an acknowledgement within three working days and an
assessment within ten. Fixes are released as soon as they are ready, with a
GitHub security advisory crediting the reporter unless you prefer otherwise.

## Scope

In scope: the `rupta` server, the web app, the container image and the
deployment files in this repository. Out of scope: vulnerabilities in the
SDKs that send events to rupta (report those to their maintainers), and
findings that require an already compromised administrator account.

## Hardening guidance

See [docs/security.md](docs/security.md) for how rupta protects data and how
to run it securely (TLS termination, secrets, SSO enforcement, data
scrubbing, network exposure of the ingest endpoints).
