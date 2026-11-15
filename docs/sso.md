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
   provider's published keys, issuer, audience and authorized party (see
   [Audiences](#audiences)), expiry and nonce.
4. Claims are read from the ID token. If the email or the role claim is
   missing there, rupta also asks the user info endpoint (and checks that its
   `sub` matches).
5. The user is found or created (below), gets a session, and is sent to the
   target path. Only local paths are followed. On any failure the browser
   goes to `/login?error=sso_failed` (or `/login?error=sso_no_access` when
   the user has no role, see below) and the reason is written to the server
   log and the audit log (`auth.sso_login_failed`).

## Audiences

The ID token's `aud` claim must contain the client id. Some providers add
more audiences: ZITADEL, for example, issues `aud = [client id, project
id]`. rupta follows OpenID Connect Core 1.0, section 3.1.3.7:

- Audiences other than the client id are accepted. With
  `RUPTA_OIDC_ALLOWED_AUDIENCES` (comma-separated) set, each of them must
  be in that list, otherwise the token is refused (`... is not a trusted
  audience`). The client id itself never needs to be listed.
- When `aud` has more than one entry, the token must carry `azp`
  (authorized party) equal to the client id.
- When `azp` is present, it must equal the client id, also with a single
  audience.

Organization providers follow the same rules, but have no list of allowed
audiences: any additional audience is accepted as long as `azp` is the
client id.

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
- **Default role `none`**: with `RUPTA_OIDC_DEFAULT_ROLE=none` (or
  *No access (refuse sign-in)* for an organization provider), a user whose
  claims map to no role is refused:
  - no user, identity or membership is created, even with just-in-time
    provisioning on;
  - an existing member whose claims no longer map to any role cannot sign
    in through single sign-on. The membership itself is kept, and so are
    sessions that already exist, API tokens and, unless single sign-on is
    enforced, password login. Remove the member or disable the user in rupta
    to end access completely;
  - **owners still sign in**, because single sign-on never changes an
    owner's role. Remove the owner role in rupta to apply the rule to them.

  The browser goes to `/login?error=sso_no_access`, and the login page says
  that the account has no role that gives access. The audit log entry
  `auth.sso_login_failed` carries the organization, the provider, the
  subject, the email, the reason and, for an existing user, the user as
  target, so it appears in the organization's audit log. `none` needs a
  role mapping (`RUPTA_OIDC_ROLE_MAPPING`; for organization providers a
  role claim and at least one mapping), otherwise only owners could sign
  in.
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
| `RUPTA_OIDC_DEFAULT_ROLE` | `member` | Role when nothing maps; `none` refuses the sign-in |
| `RUPTA_OIDC_JIT` | `true` | Create users at their first sign-in |
| `RUPTA_OIDC_ALLOWED_AUDIENCES` | empty (any) | Audiences besides the client id that ID tokens may carry, comma-separated |
| `RUPTA_LOCAL_LOGIN` | `true` | `false` hides and refuses password login |

Redirect URI to register: `{RUPTA_PUBLIC_URL}/auth/sso/oidc/callback`.

The instance provider is shown read-only on every organization's
*Settings → Single sign-on* page (admins and owners), with its name,
issuer, client id, whether a client secret is set (never the secret),
scopes, role claim and mapping, default role, just-in-time provisioning,
allowed audiences, redirect URI, the organization its users join and
whether password login is enabled. The same is available from
`GET /api/v1/orgs/{org}/sso/instance`:

```json
{"local_login": true,
 "provider": {"provider_id": "oidc", "provider_name": "Zitadel", "issuer": "https://auth.example.com",
   "client_id": "2930...@rupta", "has_client_secret": true,
   "scopes": "openid email profile urn:zitadel:iam:org:project:roles",
   "role_claim": "urn:zitadel:iam:org:project:roles",
   "role_mapping": {"rupta-admin": "admin"}, "default_role": "viewer", "jit_provisioning": true,
   "allowed_audiences": [], "redirect_uri": "https://errors.example.com/auth/sso/oidc/callback",
   "default_org": "default"}}
```

`provider` is `null` when `RUPTA_OIDC_ISSUER` is not set. The values can
only be changed in the deployment's `RUPTA_OIDC_*` variables (and a
restart).

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
- `default_role` is `viewer`, `member` (default), `admin` or `none`.
  `none` refuses users whose claims map to no role (see
  [Users and roles](#users-and-roles)) and needs `role_claim` and at least
  one `role_mapping` entry (`422` otherwise).
- ID tokens with additional audiences are accepted when `azp` is the client
  id; there is no list of allowed audiences for organization providers (see
  [Audiences](#audiences)).
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

## Instance and organization providers together

Both kinds can be configured at the same time. They are separate
providers with separate identities:

- The sign-in page always shows the instance provider. An organization's
  provider is shown next to it when the page is opened with `?org={slug}`.
- Each provider changes memberships only in its own organization: the
  instance provider in `RUPTA_DEFAULT_ORG`, an organization provider in
  its organization. When `RUPTA_DEFAULT_ORG` is the same organization,
  the role a member has is the one set by the provider they signed in with
  last.
- *Require single sign-on* (`enforce`) of an organization refuses password
  login for its members. A session from either provider is a normal rupta
  session, so members can still sign in with the instance provider.
- With `RUPTA_LOCAL_LOGIN=false`, password login is off for the whole
  instance, whatever the organization settings say.

On the settings page, the organization's own provider is an optional
section below the instance provider; it is collapsed while no
organization provider is configured.

## Testing the connection

`POST /api/v1/orgs/{org}/sso/test` (owners; the settings form's *Test
connection* button) checks a provider before it is saved. The body is the
form: `issuer`, and optionally `client_id` and `client_secret`. The server
fetches `{issuer}/.well-known/openid-configuration` and answers:

```json
{"ok": false, "issuer": "https://acme.zitadel.cloud",
 "authorization_endpoint": "https://acme.zitadel.cloud/oauth/v2/authorize",
 "token_endpoint": "https://acme.zitadel.cloud/oauth/v2/token",
 "userinfo_endpoint": "https://acme.zitadel.cloud/oidc/v1/userinfo",
 "jwks_uri": "https://acme.zitadel.cloud/oauth/v2/keys",
 "scopes_supported": ["openid", "profile", "email"],
 "problems": ["without a client secret the provider must allow public clients (token endpoint auth method none)"]}
```

`ok` is true when `problems` is empty. Checked: the document's `issuer`
equals the configured one (ignoring a trailing `/`); the authorization and
token endpoints and `jwks_uri` are present and http(s) URLs; response type
`code` and PKCE with `S256` are announced; `openid` is among the scopes if
scopes are listed; the token endpoint accepts the client: `none` for a
public client (no secret), `client_secret_basic` or `client_secret_post`
with a secret (`client_secret_basic` is assumed when the provider lists no
methods). Reachability problems (DNS, timeout, HTTP status, invalid JSON)
are reported in `problems` as well. The client id and secret are not sent
to the provider: whether the provider knows the client shows only at the
first sign-in.

The request is made by the server, so it is limited:

- The issuer host must resolve only to public addresses. Loopback,
  private (RFC 1918, IPv6 unique local), link-local (including the cloud
  metadata address `169.254.169.254`), carrier-grade NAT and unspecified
  addresses are refused, unless the host is the host of the instance
  provider (`RUPTA_OIDC_ISSUER`) or `RUPTA_SSO_ALLOW_PRIVATE_ISSUERS=true`.
- The connection goes to the address that was checked (no second DNS
  lookup), redirects are not followed, the answer may be at most 1 MiB and
  the whole request takes at most 5 seconds.
- When the server reaches the internet through an HTTP proxy configured in
  the environment (`HTTPS_PROXY`), the proxy opens the connection; the
  check covers the address rupta resolves, not what the proxy does.

Saving the settings (`PUT .../sso`) applies the same address check
before it fetches the discovery document, because the server contacts the
provider again at every sign-in of the organization's members. Providers
on the internal network therefore need `RUPTA_SSO_ALLOW_PRIVATE_ISSUERS=true`
(or must be the instance provider). The check runs when the settings are
saved; a provider whose DNS name later resolves to an internal address is
not detected.

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

   The reserved scope `urn:zitadel:iam:org:project:roles` asks for the
   roles of the project the application belongs to; Zitadel returns them in
   the claim of the same name, which is the default `RUPTA_OIDC_ROLE_CLAIM`.
   The organization settings' Zitadel preset uses the same scope and claim.
   (`urn:zitadel:iam:org:projects:roles`, with `projects`, asks for the
   roles of all projects in the audience; they arrive in the same claim.)
   To limit roles to one project when the client belongs to several, use
   `urn:zitadel:iam:org:project:id:{project id}:aud` as an extra scope;
   Zitadel's per-project claim `urn:zitadel:iam:org:project:{project id}:roles`
   can be used as `RUPTA_OIDC_ROLE_CLAIM` as well.

   Zitadel's ID tokens carry the project id as a second audience and the
   client id as `azp`; rupta accepts that (see [Audiences](#audiences)). To
   accept only your project, set
   `RUPTA_OIDC_ALLOWED_AUDIENCES={project id}`.
9. **Try it.** Open the login page, click *Zitadel*, sign in. The first
   sign-in creates the user (if JIT is on) and the membership in
   `RUPTA_DEFAULT_ORG` with the mapped role. Check *Organization settings →
   Members* and the audit log.

Troubleshooting: the server log names the failing step (`discovery failed`,
`token exchange failed`, `the ID token is invalid: ...`, `no matching user
and just-in-time provisioning is off`, `no role is mapped for the user's
claims and the default role is none`, ...). `Invalid audiences: ... is not
a trusted audience` means an additional audience is missing from
`RUPTA_OIDC_ALLOWED_AUDIENCES`; `no authorized party (azp)` means the
provider sent several audiences without `azp`. A common cause is a redirect URI
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
