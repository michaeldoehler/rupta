# Authentication and permissions

rupta knows users, organizations and the role a user has in each
organization. People sign in with email and password, with single sign-on
(see [single sign-on](sso.md)), or both. Programs use API tokens.

## First start

- If `RUPTA_ADMIN_EMAIL` and `RUPTA_ADMIN_PASSWORD` are set and no user
  exists, the server creates that user as **superuser**, the organization
  `RUPTA_DEFAULT_ORG` (default `default`) and makes the user its owner. With
  users present, the variables are ignored.
- Otherwise the web app shows a setup form (`POST /api/v1/auth/setup` with
  email, name, password and organization name). It works exactly once:
  as soon as a user exists it answers `409`.

A superuser acts as owner of every organization and sees all of them.

## Local accounts

- Passwords need at least 10 characters (at most 256 bytes) and are hashed
  with Argon2id (19 MiB memory, 2 passes, 1 lane, random salt), stored as
  PHC strings.
- Emails are compared in lowercase.
- Login answers the same (`401 invalid_credentials`) for an unknown email,
  a wrong password and a disabled account, and spends the same time hashing
  in each case.
- **Lockout**: 10 failed logins for one email within 15 minutes lock that
  email for 15 minutes; 50 failures from one IP address lock the address.
  While locked, logins answer `429` with `Retry-After`, even with the right
  password. Unknown emails are counted like real ones. The counters are
  kept in memory and reset when the server restarts. Behind a reverse proxy,
  set `RUPTA_TRUSTED_PROXIES` so the client address comes from
  `X-Forwarded-For`.
- `RUPTA_LOCAL_LOGIN=false` disables password login for everybody (single
  sign-on must then be configured).
- An organization can **enforce single sign-on**: its members cannot log in
  with a password (`403 sso_required`, with the provider id in
  `details.provider`). Owners keep password login as break-glass access
  unless the organization turns that off too.
- Changing the password (`PATCH /api/v1/user` with `current_password` and
  `new_password`) ends all other sessions of the user. Accounts created
  through single sign-on have no password until they set one.

## Sessions

- A session is a random 32-byte token in the cookie `rupta_session`:
  `HttpOnly`, `SameSite=Lax`, `Path=/`, and `Secure` when
  `RUPTA_PUBLIC_URL` starts with `https://`.
- The database stores only the token's SHA-256, together with the user,
  the client IP, the user agent and the expiry.
- Sessions expire 14 days after their last use. The expiry is moved forward
  at most every 10 minutes.
- Each session has its own CSRF token. `POST`, `PUT`, `PATCH` and `DELETE`
  requests authenticated by the cookie must send it in `X-CSRF-Token`;
  otherwise they get `403 csrf_failed`. Together with `SameSite=Lax` this
  stops cross-site request forgery.
- Logout deletes the session. Disabling a user ends their sessions at their
  next request.

## API tokens

- Format: `rpt_` followed by 40 random characters from `[A-Za-z0-9]`
  (about 238 bits). Send as `Authorization: Bearer rpt_...`.
- The token is shown once, when it is created. The database stores its
  SHA-256 and its last four characters for recognition.
- **Personal tokens** (`/api/v1/user/tokens`) act as their user, limited by
  their scopes: a request needs both the user's role and a matching scope.
- **Organization tokens** (`/api/v1/orgs/{org}/tokens`, admins) act only in
  their organization, with at most the admin role, limited by their scopes.
  Use them for CI and the upload CLI.
- Tokens can expire (`expires_in_days`, up to 3650) and can be revoked at
  any time. `last_used_at` is updated at most once a minute.
- Tokens cannot create tokens, and cannot change passwords; these need a
  browser session.

| Scope | Grants |
| --- | --- |
| `org:read` | Read the organization, projects, issues, events, releases, members |
| `org:write` | Organization settings, teams, notification channels |
| `org:admin` | Organization tokens, audit log; delete organization and configure SSO (personal tokens of owners only) |
| `member:read` | List members and teams |
| `member:write` | Invite, change and remove members |
| `project:read` | Read projects, issues and events |
| `project:write` | Project settings, keys, alert rules; triage and delete issues |
| `project:admin` | Create and delete projects |
| `project:releases` | Upload source maps, create releases and deploys (upload CLI) |
| `issue:write` | Resolve, ignore, assign, comment |
| `release:write` | Create and change releases and deploys |

`GET /api/v1/scopes` lists them.

## Roles

Each member of an organization has one role. Owners can do everything;
the table lists the lowest role for each action.

| Action | Viewer | Member | Admin | Owner |
| --- | :---: | :---: | :---: | :---: |
| See projects, issues, events, releases, members, teams | ✓ | ✓ | ✓ | ✓ |
| Bookmark issues, mark as seen, save private searches | ✓ | ✓ | ✓ | ✓ |
| Resolve, ignore, assign, prioritize issues; comment | | ✓ | ✓ | ✓ |
| Create releases and deploys, upload source maps | | ✓ | ✓ | ✓ |
| Delete issues | | | ✓ | ✓ |
| Create, change and delete projects, keys, project settings | | | ✓ | ✓ |
| Alert rules and notification channels | | | ✓ | ✓ |
| Teams; rename the organization; shared saved searches | | | ✓ | ✓ |
| Invite, change and remove members (not owners) | | | ✓ | ✓ |
| Organization tokens, audit log | | | ✓ | ✓ |
| Invite, promote, demote and remove owners | | | | ✓ |
| Single sign-on settings, delete the organization | | | | ✓ |

Rules around owners: an organization always keeps at least one owner (the
last owner cannot be demoted, removed or leave). Admins cannot change
owners or make anyone owner. Every member may leave an organization.

The permission check lives in one place and is applied by one guard in
the API before any data is read. Callers that
are not members of an organization get `404` for everything in it.

## Invitations

Admins invite by email (`POST /api/v1/orgs/{org}/members` with `email` and
`role`). The link `{RUPTA_PUBLIC_URL}/accept-invite/{token}` is valid for 7
days and works once; a new invitation for the same email replaces the old
one. When SMTP is configured the link is emailed; the API also returns it
to the inviting admin (`accept_url`) so it can be passed on when email is
not set up. The link is accepted by a signed-in user, or, when password
login is enabled, by creating an account for the invited email. Only the
token's SHA-256 is stored.

## Audit log

Security-relevant actions are recorded with actor (user or token), target,
details and client IP, and shown to admins at
`GET /api/v1/orgs/{org}/audit-log` (filter with `?action=`):

- `auth.setup`, `auth.login`, `auth.login_failed`, `auth.login_locked`,
  `auth.logout`, `auth.sso_login`, `auth.sso_login_failed`
- `user.password_change`
- `org.create`, `org.update`, `org.delete`
- `member.invite`, `member.invite_cancel`, `member.join`,
  `member.role_change`, `member.remove`
- `team.create`, `team.update`, `team.delete`
- `token.create`, `token.delete`
- `sso.config_change`, `sso.config_delete`
- `project.create`, `project.update`, `project.delete`
- `key.create`, `key.update`, `key.delete`
- `channel.create`, `channel.update`, `channel.delete`
- `alert_rule.create`, `alert_rule.update`, `alert_rule.delete`
- `issue.delete`
- `artifact_bundle.delete`

Logins and logouts belong to no organization; an organization's log shows
those of its members. Failed logins for unknown emails are recorded but
appear in no organization's log. The audit log is not pruned automatically.
