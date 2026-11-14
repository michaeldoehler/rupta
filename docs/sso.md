# Single sign-on (OpenID Connect)

rupta signs users in through any OpenID Connect provider. Zitadel is the
reference and has a step-by-step guide below; Keycloak, Authentik, Okta,
Microsoft Entra ID, Google and others work the same way.

There are two kinds of providers:

- **Instance provider**, configured with `RUPTA_OIDC_*` environment
  variables. Its id is `oidc`; users who sign in with it join the
  organization `RUPTA_DEFAULT_ORG`.
- **Organization providers**, configured by an organization's owner in the
  settings (`PUT /api/v1/orgs/{org}/sso`). Their id is `org-{slug}`; users
  who sign in with them join that organization.

## How it works

1. The login button opens `/auth/sso/{provider}/start?next=/path`.
   rupta fetches the provider's discovery document
   (`{issuer}/.well-known/openid-configuration`, cached for an hour) and
   redirects to its authorization endpoint with the authorization code flow,
   **PKCE** (S256), a random `state` and a random `nonce`.
2. The state, the nonce, the PKCE verifier and the target path are kept in
   the cookie `rupta_sso_state` (path `/auth/sso`, `HttpOnly`,
   `SameSite=Lax`, 10 minutes), signed with a key derived from
   `RUPTA_SECRET_KEY`. Nothing is stored on the server.
3. The provider redirects back to
   `{RUPTA_PUBLIC_URL}/auth/sso/{provider}/callback`. rupta checks the state,
   exchanges the code (with the client secret, if one is configured, and the
   PKCE verifier), and validates the ID token: signature against the
   provider's published keys, issuer, audience, expiry and nonce.
4. Claims are read from the ID token. If the email or the role claim is
   missing there, rupta also asks the user info endpoint (and checks that its
   `sub` matches).
5. The user is found or created (below), gets a session, and is sent to the
   target path. Only local paths are followed. On any failure the browser
   goes to `/login?error=sso_failed` and the reason is written to the server
   log and the audit log (`auth.sso_login_failed`).

The HTTP client used for the provider follows no redirects and times out
after 10 seconds.

## Users and roles

- An identity is the pair (provider, `sub`). A known identity always signs
  in its user, even if the email at the provider changed.
- A new identity is linked to an existing user with the same email **only
  if the provider marks the email as verified** (`email_verified: true`).
  Otherwise sign-in is refused, so that nobody can take over an account by
  registering its email at a provider.
- Without a matching user, a new user (without a password) is created if
  just-in-time provisioning is on (`RUPTA_OIDC_JIT=true`, the default). With
  it off, only existing users can sign in.
- **Roles**: the role claim (`RUPTA_OIDC_ROLE_CLAIM`) may be an object whose
  keys are role names (Zitadel), a list of strings, or a string with names
  separated by spaces or commas. Names are mapped with
  `RUPTA_OIDC_ROLE_MAPPING` (`provider-role:rupta-role,...`). The highest
  mapped role wins and is applied at every sign-in, so removing a role at
  the provider takes effect at the next sign-in. Two exceptions: owners are
  never demoted by single sign-on (owner is managed in rupta), and when no
  role name maps, a new member gets `RUPTA_OIDC_DEFAULT_ROLE` while an
  existing member keeps the role they have.
- Role changes made this way are recorded in the audit log
  (`member.join`, `member.role_change` with `"via": "oidc"`).
- Disabled users cannot sign in.

## Instance provider

| Variable | Default | Meaning |
| --- | --- | --- |
| `RUPTA_OIDC_ISSUER` | — | Issuer URL; enables single sign-on |
| `RUPTA_OIDC_CLIENT_ID` | — | Client id |
| `RUPTA_OIDC_CLIENT_SECRET` | — | Client secret (also `_FILE`) |
| `RUPTA_OIDC_NAME` | `Single sign-on` | Label of the login button |
| `RUPTA_OIDC_SCOPES` | `openid email profile urn:zitadel:iam:org:project:roles` | Scopes |
| `RUPTA_OIDC_ROLE_CLAIM` | `urn:zitadel:iam:org:project:roles` | Claim with role names |
| `RUPTA_OIDC_ROLE_MAPPING` | empty | e.g. `rupta-admin:admin,rupta-viewer:viewer` |
| `RUPTA_OIDC_DEFAULT_ROLE` | `member` | Role when nothing maps |
| `RUPTA_OIDC_JIT` | `true` | Create users at their first sign-in |
| `RUPTA_LOCAL_LOGIN` | `true` | `false` hides and refuses password login |

Redirect URI to register: `{RUPTA_PUBLIC_URL}/auth/sso/oidc/callback`.

## Organization providers

Owners configure them in *Organization settings → Single sign-on* or with
the API:

```sh
curl -X PUT -H "Authorization: Bearer $OWNER_TOKEN" -H 'Content-Type: application/json' \
  https://errors.example.com/api/v1/orgs/acme/sso -d '{
    "provider_name": "Acme login",
    "issuer": "https://acme.zitadel.cloud",
    "client_id": "2930...@rupta",
    "client_secret": "...",
    "scopes": "openid email profile urn:zitadel:iam:org:project:roles",
    "role_claim": "urn:zitadel:iam:org:project:roles",
    "role_mapping": {"rupta-admin": "admin", "rupta-viewer": "viewer"},
    "default_role": "member",
    "jit_provisioning": true,
    "enforce": false,
    "owner_local_login": true
  }'
```

- The issuer's discovery document is fetched before saving; an unreachable
  or invalid issuer is refused (`422`).
- The client secret is encrypted with XChaCha20-Poly1305 under a key
  derived from `RUPTA_SECRET_KEY` (HKDF-SHA256) and bound to the
  organization. It is never returned (`has_client_secret` tells whether one
  is stored). Omit it to keep the stored one; send `""` for a public client
  (PKCE only). If `RUPTA_SECRET_KEY` changes, stored secrets cannot be
  decrypted and must be entered again.
- `enforce: true` refuses password login for members of the organization;
  `owner_local_login: true` keeps it for owners as break-glass access.
- The redirect URI is in the answer (`redirect_uri`):
  `{RUPTA_PUBLIC_URL}/auth/sso/org-{slug}/callback`.
- The login page shows an organization's provider when it is opened with
  `?org={slug}` (`GET /api/v1/auth/config?org={slug}`). Providers of other
  organizations are not listed, so the login page does not reveal which
  organizations exist.

## Zitadel step by step

The steps refer to the Zitadel console (cloud or self-hosted). Replace
`https://errors.example.com` with your `RUPTA_PUBLIC_URL`.

1. **Project.** In your Zitadel organization, open *Projects* and create a
   project, e.g. `rupta`.
2. **Roles.** In the project, open *Roles* and add role keys, for example
   `rupta-admin`, `rupta-member` and `rupta-viewer`. The keys are what you
   map later; display names and groups do not matter.
3. **Role assertion.** In the project's *General* settings, enable
   **Assert Roles on Authentication**. Zitadel then puts the user's roles
   into the token and user info as the claim
   `urn:zitadel:iam:org:project:roles`, shaped like
   `{"rupta-admin": {"<org id>": "<org domain>"}}`. Also enable *Check
   authorization on Authentication* if users without any role should not be
   able to sign in at all.
4. **Application.** In the project, *New* application:
   - Type **Web**.
   - Authentication method: **PKCE** (no client secret) or **Code** with a
     client secret (*client secret basic*). rupta uses PKCE in both cases.
   - Redirect URI: `https://errors.example.com/auth/sso/oidc/callback`
     (for an organization provider: `.../auth/sso/org-{slug}/callback`).
     For a plain-`http` test instance enable *Development Mode*.
   - Post-logout URI: not used.
5. **Token settings** of the application: optionally enable *User roles
   inside ID Token* and *User Info inside ID Token*. Without them rupta reads
   roles and email from the user info endpoint, which also works.
6. **Client id and secret.** Copy the client id (and, for *Code*, the
   secret). The issuer is your instance URL, e.g.
   `https://acme-xyz.zitadel.cloud` or `https://auth.example.com`.
7. **Authorizations.** Grant users (or groups via *Authorizations*) the
   project roles they should have.
8. **Configure rupta:**

   ```sh
   RUPTA_PUBLIC_URL=https://errors.example.com
   RUPTA_OIDC_ISSUER=https://auth.example.com
   RUPTA_OIDC_CLIENT_ID=2930...@rupta
   RUPTA_OIDC_CLIENT_SECRET=...          # or RUPTA_OIDC_CLIENT_SECRET_FILE
   RUPTA_OIDC_NAME=Zitadel
   RUPTA_OIDC_SCOPES="openid email profile urn:zitadel:iam:org:project:roles"
   RUPTA_OIDC_ROLE_MAPPING=rupta-admin:admin,rupta-member:member,rupta-viewer:viewer
   RUPTA_OIDC_DEFAULT_ROLE=viewer
   ```

   The scope `urn:zitadel:iam:org:project:roles` asks for the roles claim.
   To limit roles to one project when the client belongs to several, use
   `urn:zitadel:iam:org:project:id:{project id}:aud` as an extra scope;
   Zitadel's per-project claim `urn:zitadel:iam:org:project:{project id}:roles`
   can be used as `RUPTA_OIDC_ROLE_CLAIM` as well.
9. **Try it.** Open the login page, click *Zitadel*, sign in. The first
   sign-in creates the user (if JIT is on) and the membership in
   `RUPTA_DEFAULT_ORG` with the mapped role. Check *Organization settings →
   Members* and the audit log.

Troubleshooting: the server log names the failing step (`discovery failed`,
`token exchange failed`, `the ID token is invalid: ...`, `no matching user
and just-in-time provisioning is off`, ...). A common cause is a redirect URI
that differs from the one registered, or `RUPTA_PUBLIC_URL` not matching the
URL users open.

## Limits

- Only the authorization code flow with PKCE is supported; no implicit
  flow, no front- or back-channel logout. Signing out of rupta does not sign
  out of the provider, and a session lives until it expires even if the user
  is removed at the provider (disable the user in rupta to end it).
- ID tokens must be signed (RS256, ES256 and the other algorithms the
  OpenID library supports); encrypted ID tokens are not supported.
- Provider changes take effect at the next sign-in; there is no SCIM or
  periodic synchronization.
- Tested against an in-process provider in the test suite
  (`crates/rupta-api/tests/sso.rs`), not against a live Zitadel instance.
