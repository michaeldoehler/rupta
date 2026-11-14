# Issue tracker integrations

rupta can create tickets for issues in Jira, Linear, GitHub and GitLab,
link issues to existing tickets, and keep the two in step: closing the
ticket can resolve the issue, resolving the issue can close the ticket.

| Kind | Tracker | API rupta uses | Authentication | Descriptions |
| --- | --- | --- | --- | --- |
| `jira_cloud` | Jira Cloud | Jira Cloud platform REST API **v3** | basic auth: account email + API token | Atlassian Document Format |
| `jira_server` | Jira Data Center / Server (9.x, 10.x) | Jira REST API **v2** | personal access token (`Authorization: Bearer`) | wiki markup |
| `linear` | Linear | Linear **GraphQL** API (`https://api.linear.app/graphql`) | API key in `Authorization` | Markdown |
| `github` | GitHub.com, GitHub Enterprise Server | GitHub REST API, version **2022-11-28** | fine-grained or classic personal access token (`Bearer`) | Markdown |
| `gitlab` | GitLab.com, self-managed GitLab | GitLab REST API **v4** | personal, project or group access token (`PRIVATE-TOKEN`) | Markdown |

> **Self-hosted trackers on internal addresses.** rupta refuses to call
> hosts that resolve to loopback, private (10/8, 172.16/12, 192.168/16),
> link-local, carrier-grade NAT or unique-local IPv6 addresses, so that an
> organization admin cannot make the server reach internal services. Jira
> Data Center and self-managed GitLab usually run on such addresses: set
> **`RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS=true`** to allow them. The
> setting is instance-wide and lets every organization admin point an
> integration at any internal host, so turn it on only where all admins
> are trusted. It also applies to PagerDuty and Opsgenie channels with a
> custom `api_url`.

## Concepts

- An **integration** is a connection to one tracker, owned by an
  organization: kind, name, settings (`config`), the token and the webhook
  secret. Admins and owners manage integrations; members see them (without
  secrets) to create and link tickets.
- **Project defaults** say, per project and integration, where new tickets
  go (`target`: Jira project key, Linear team id, GitHub `owner/repo`,
  GitLab project path), with which issue type (Jira), Linear project and
  labels, and which way statuses sync.
- An **issue link** connects a rupta issue with a ticket. An issue can be
  linked to several tickets, in the same or in different trackers. The
  link stores the ticket's key (`OPS-42`, `ENG-42`, `#17`), URL, title,
  status, status category (`open` or `done`) and assignee, as last seen
  by rupta.

## Setting up

1. Create the token in the tracker (below, per tracker).
2. In rupta: organization settings → Integrations → add, or
   `POST /api/v1/orgs/{org}/integrations`:

   ```json
   {"kind": "jira_cloud", "name": "Jira", "token": "ATATT3x...",
    "config": {"base_url": "https://acme.atlassian.net", "email": "bot@acme.io"}}
   ```

   rupta checks the token against the tracker before saving (`validate:
   false` skips the check) and stores who it belongs to (`account`). The
   answer contains the **webhook URL** and, for every kind except Linear,
   a generated **webhook secret**, shown only in this answer. A new one can
   be generated later with `PATCH ... {"rotate_webhook_secret": true}`;
   your own can be set with `{"webhook_secret": "..."}`.
3. Create the webhook in the tracker with that URL and secret.
4. Per project: project settings → Integrations, or
   `PUT /api/v1/projects/{org}/{project}/integrations/{integration_id}`:

   ```json
   {"target": "OPS", "issue_type": "Bug", "labels": ["rupta"],
    "incoming_sync": true, "outgoing_transition": true, "outgoing_comment": true}
   ```

5. Test: `POST /api/v1/orgs/{org}/integrations/{id}/test` answers
   `{ok, account, error, reason}`.

The token and the webhook secret are encrypted with a key derived from
`RUPTA_SECRET_KEY` (XChaCha20-Poly1305, bound to the integration's id),
never returned (the token is shown as `****` and its last four
characters) and never written to the audit log. Changing
`RUPTA_SECRET_KEY` makes them unreadable: enter them again.

The webhook URL is
`{RUPTA_PUBLIC_URL}/api/integrations/{kind}/{integration_id}/webhook`. It
must be reachable from the tracker (for the cloud trackers: from the
internet).

## Jira Cloud (`jira_cloud`)

Settings: `base_url` (required, `https://<site>.atlassian.net`), `email`
(required, the account the token belongs to), `resolve_transition` and
`reopen_transition` (optional, see below).

**Token.** Log in as the account rupta should act as (a dedicated account
is best, since tickets and comments show it as author), open
<https://id.atlassian.com/manage-profile/security/api-tokens> → *Create
API token*, give it a name and an expiry date, copy it. Use a classic API
token: tokens *with scopes* only work through the `api.atlassian.com`
gateway, which rupta does not use.

**Permissions** of the account in the target projects: *Browse projects*,
*Create issues*, *Add comments*, *Transition issues*. Listing targets
needs *Browse projects* in each project to list.

**Webhook.** Jira admin → Settings → System → WebHooks → *Create a
WebHook*: URL = the webhook URL, *Secret* = the webhook secret, events
*Issue: updated* and *Issue: deleted* (and optionally *created*), and
optionally a JQL filter such as `project in (OPS, WEB)`. Jira signs the
body with the secret (`X-Hub-Signature: sha256=<hex HMAC-SHA256>`) and
names each delivery (`X-Atlassian-Webhook-Identifier`), which rupta uses
to ignore redeliveries. Creating admin webhooks needs the *Administer
Jira* global permission.

Endpoints called: `GET /rest/api/3/myself`, `GET
/rest/api/3/project/search?expand=issueTypes`, `GET
/rest/api/3/project/{key}`, `POST /rest/api/3/issue`, `GET
/rest/api/3/issue/{idOrKey}`, `POST /rest/api/3/issue/{idOrKey}/comment`,
`GET` and `POST /rest/api/3/issue/{idOrKey}/transitions`.

## Jira Data Center / Server (`jira_server`)

Settings: `base_url` (required, e.g. `https://jira.example.com` or
`https://example.com/jira`), `resolve_transition`, `reopen_transition`.

**Token.** Personal access tokens exist since Jira 8.14: profile picture
→ *Profile* → *Personal Access Tokens* → *Create token*. The token acts
with the permissions of its user; the same permissions as for Jira Cloud
are needed.

**Webhook.** Administration → System → WebHooks → *Create a WebHook*,
events *Issue updated* and *Issue deleted*. Jira versions whose form has a
*Secret* field sign deliveries like Jira Cloud. For versions without it,
append the secret to the URL:
`.../webhook?secret=<webhook secret>`. rupta accepts either; the query
string form is weaker, since URLs can end up in proxy and access logs.

Self-managed Jira on an internal address needs
`RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS=true`.

Endpoints called: as for Jira Cloud under `/rest/api/2`, with `GET
/rest/api/2/project?expand=issueTypes` for targets.

### Issue types and transitions (both Jira kinds)

- `issue_type` in the project defaults or in the create request is an id
  when it is numeric, otherwise a name. Without one, rupta takes the
  project's `Bug` type, or its first type that is not a sub-task.
- Labels cannot contain spaces in Jira; rupta replaces them with `-`.
- To resolve, rupta uses the transition named `resolve_transition` (the
  name of the transition or of its target status, case-insensitive). Not
  set: the first transition into a status of the *Done* category. To
  reopen: `reopen_transition`, else the first transition into a *To Do*
  category status, else any transition out of *Done*. Workflows that
  require screens or fields on a transition cannot be driven this way;
  configure a transition without required fields. When no transition fits,
  the activity of the issue records the failure.

## Linear (`linear`)

Settings: `resolve_state` and `reopen_state` (optional, state names or
ids), `base_url` (only for proxies and tests; default
`https://api.linear.app`).

**API key.** Linear → Settings → *Security & access* → *Personal API
keys* → *New API key*. Give it at least *Read*, *Create issues*, *Create
comments* and *Write* (moving issues between states is a write), and
access to the teams rupta should file into. The key acts as you; a
dedicated member is best. Keys start with `lin_api_` and are sent as is in
`Authorization`; other values (OAuth access tokens) are sent as `Bearer`.

**Webhook.** Settings → *API* → *Webhooks* → *New webhook* (workspace
admins): URL = the webhook URL, data change events *Issues*, all or
selected teams. Linear generates a **signing secret**; copy it and save it
in rupta (`PATCH /api/v1/orgs/{org}/integrations/{id}` with
`{"webhook_secret": "lin_wh_..."}`). Until then, webhooks are refused.
rupta checks `Linear-Signature` (hex HMAC-SHA256 of the body),
refuses `webhookTimestamp` values more than five minutes off (Linear
suggests one minute; rupta allows some clock skew) and uses
`Linear-Delivery` against redeliveries.

**Targets** are teams, with their workflow states, labels (team and
workspace labels) and projects. In the project defaults, `target` is the
team id, `project` an optional Linear project id, and `labels` label names
or ids; unknown labels make ticket creation fail with a message.

Without `resolve_state`, resolving moves the issue to the team's first
*completed* state; without `reopen_state`, reopening moves it to the first
*unstarted* state (then *backlog*, then *started*). *Canceled* counts as
done when it comes from Linear.

Operations: `viewer`, `teams` with `states`, `labels`, `projects`;
`issueLabels`; `issueCreate`; `issue(id:)` (UUID or identifier like
`ENG-42`); `commentCreate`; `issueUpdate`.

## GitHub (`github`)

Settings: `base_url` (default `https://api.github.com`; GitHub Enterprise
Server: `https://<host>/api/v3`).

**Token.** Fine-grained (recommended): GitHub → Settings → Developer
settings → *Personal access tokens* → *Fine-grained tokens* → *Generate
new token*; resource owner = the organization; repository access = the
repositories; permissions: **Issues: Read and write** (Metadata: read is
added automatically). Organizations may require approval of fine-grained
tokens. Classic tokens need the `repo` scope (`public_repo` for public
repositories only).

**Webhook.** Repository (or organization) → Settings → *Webhooks* → *Add
webhook*: payload URL = the webhook URL, content type
**`application/json`**, secret = the webhook secret, *Let me select
individual events* → **Issues**. rupta checks `X-Hub-Signature-256`, reads
`X-GitHub-Event` (only `issues` changes links; `ping` is answered) and
uses `X-GitHub-Delivery` against redeliveries.

**Targets** are the repositories the token can access that have issues
enabled and are not archived (`GET /user/repos`, up to 1,000).

Tickets are `owner/repo#17`, shown as `#17`. Existing issues can be
linked by URL (`https://github.com/owner/repo/issues/17`), by
`owner/repo#17`, or by `#17` when the project defaults name the
repository. Resolving closes the issue as *completed*; reopening reopens
it. An issue closed as *not planned* counts as done. Pull requests cannot
be linked.

Endpoints: `GET /user`, `GET /user/repos`, `POST
/repos/{owner}/{repo}/issues`, `GET` and `PATCH
/repos/{owner}/{repo}/issues/{number}`, `POST
/repos/{owner}/{repo}/issues/{number}/comments`. A `403` with
`x-ratelimit-remaining: 0` is reported as rate limited.

## GitLab (`gitlab`)

Settings: `base_url` (default `https://gitlab.com`; self-managed:
`https://gitlab.example.com`).

**Token** with the **`api`** scope: a project access token (project →
Settings → *Access tokens* → *Add new token*, role *Reporter* or higher so
that it may close and reopen issues), a group access token (the same in
the group), or a personal access token (user settings → *Access tokens*).
Project and group access tokens act as a bot user, which shows as the
author of tickets and comments.

**Webhook.** Project → Settings → *Webhooks* → *Add new webhook*: URL =
the webhook URL, *Secret token* = the webhook secret, trigger **Issues
events**. GitLab sends the secret in `X-Gitlab-Token`, which rupta
compares in constant time; `Idempotency-Key` (GitLab 17.4 and later) or
`X-Gitlab-Event-UUID` identifies redeliveries. Group webhooks work the
same where your GitLab edition offers them.

**Targets** are the projects the token is a member of with issues enabled
(`GET /api/v4/projects?membership=true&with_issues_enabled=true`, up to
1,000). Tickets are `group/project#5`, shown as `#5`; existing issues can
be linked by URL (`.../group/project/-/issues/5`), by `group/project#5` or
by `#5` with a default project.

Endpoints: `GET /api/v4/user`, `GET /api/v4/projects`, `POST
/api/v4/projects/:id/issues`, `GET` and `PUT
/api/v4/projects/:id/issues/:iid` (`state_event: close|reopen`), `POST
/api/v4/projects/:id/issues/:iid/notes`, with `:id` the URL-encoded
project path.

Self-managed GitLab on an internal address needs
`RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS=true`.

## Creating and linking tickets

`POST /api/v1/issues/{id}/links` (members):

- `{"integration_id": "3"}` creates a ticket in the project's default
  target. `target`, `issue_type`, `project` and `labels` override the
  defaults; `title` and `description` replace the generated ones (a link
  back to the issue is still added).
- `{"integration_id": "3", "mode": "link", "reference": "OPS-42"}` links
  an existing ticket by key or URL.

The generated ticket has the title `[SHORT-ID] issue title` (at most 255
characters) and a description with: a link to the issue
(`{RUPTA_PUBLIC_URL}/organizations/{org}/issues/{id}`), the error type
and message, culprit, project, level, first and last seen, event and user
counts, first and last release, the five most common tags with their top
three values and shares, and up to ten in-app frames of the latest event
(crashing frame first) as a code block. `GET
/api/v1/issues/{id}/ticket-draft` returns title and description as
Markdown so that the create dialog can show them.

`DELETE /api/v1/issues/{id}/links/{link_id}` removes the link (the ticket
stays). `POST .../links/{link_id}/refresh` reads the ticket again.
Linking, unlinking and every sync step write an entry into the issue's
activity (`ticket_linked`, `ticket_unlinked`, `ticket_status_change`,
`ticket_sync`).

The alert rule action `create_ticket` creates a ticket when the rule
fires, at most once per issue and integration (see
[alerts](alerts.md#actions)).

## Status synchronisation

| Direction | Setting (project defaults) | What happens |
| --- | --- | --- |
| tracker → rupta | `incoming_sync` | A webhook says the ticket moved into a done state (Jira category *Done*, Linear *completed* or *canceled*, GitHub/GitLab *closed*): an unresolved rupta issue is resolved. Moved out of done: a resolved issue is unresolved. Ignored issues are not changed. |
| rupta → tracker | `outgoing_transition` | Resolving the issue moves the ticket to done (see the transition rules above); unresolving it, or a regression (a new event for a resolved issue), reopens it. |
| rupta → tracker | `outgoing_comment` | The same three events add a comment ("WEB-12 was resolved in rupta by Ann Lee.", with a link). |

Every webhook, with or without `incoming_sync`, updates the stored title,
status and assignee of the links to the ticket. A ticket deleted in the
tracker gets the status `deleted`; the link stays until it is removed.

**Loop protection.**

- A change that came from a tracker's webhook is never sent back to that
  tracker. (It is sent to other trackers linked to the same issue, when
  their defaults ask for it.)
- Applying a status the issue or ticket already has does nothing, so the
  tracker's webhook about a change rupta made ends there.
- For two minutes after rupta moves a ticket (the quiet period), webhooks
  about that ticket update the link but do not resolve or unresolve the
  rupta issue. Trackers do not guarantee the order of webhooks; without
  this, the late webhook of an earlier change could undo what a person
  just did in rupta. A person who changes the ticket in the tracker within
  these two minutes has to change the rupta issue by hand.

**Idempotency.** Deliveries with a delivery id already seen (kept for
seven days) are answered `200 {"ok": true, "duplicate": true}` without
effect. Deliveries without an id are applied again, which changes nothing
when the state is the same.

**Answers of the webhook endpoint.** `200 {"ok": true, "duplicate":
false, "updated_links": n, "changed_issues": n}`; `401` for an unknown or
disabled integration, a URL whose kind does not match, a missing or wrong
signature or token, or a Linear integration without a signing secret;
`400` for a verified body that is not JSON. Events for tickets that are
not linked are accepted and ignored.

## Requests to trackers

All calls go through one HTTP client: connection timeout 5 s, request
timeout 15 s, answers up to 5 MiB, redirects not followed (an `http://`
URL that the tracker redirects to `https://` fails; use the final URL).
`429`, `503` and failures to connect are retried for every request; other
`5xx` answers, timeouts and connections that break after the request was
sent only for requests that are safe to repeat (reads, updates, GraphQL
queries), up to three attempts with 250 ms and
500 ms backoff plus jitter, or the server's `Retry-After` when it is at
most 30 seconds. Ticket creation is never retried after a `500`, `502` or
`504`, since the ticket may exist.

The last failure is stored on the integration (`last_error`,
`last_error_at`) and cleared by the next success. On `/metrics`:
`rupta_integration_requests_total{kind, operation, result}`,
`rupta_integration_webhooks_total{kind, result}` and
`rupta_integration_sync_total{kind, direction, result}` (see
[monitoring](monitoring.md)).

## Permissions

| Action | Role (token scope) |
| --- | --- |
| List and read integrations | viewer (`org:read`) |
| Create, change, delete, test integrations | admin (`org:write`) |
| List targets, create, link, unlink and refresh tickets | member (`issue:write` or `project:write`) |
| Read links and the ticket draft | viewer (`project:read`) |
| Read project defaults | viewer (`project:read`) |
| Change project defaults | admin (`project:write`) |

Creating, changing and deleting integrations and changing project
defaults are recorded in the audit log (`integration.create`,
`integration.update`, `integration.delete`,
`integration.project_defaults_change`), with the changed field names but
no secrets.

## Limits

- Webhooks sent while rupta was unreachable are not fetched later; trackers
  retry for a while (GitHub does not retry failed deliveries
  automatically). *Refresh* on a link reads the current state.
- Only statuses sync. Comments, assignees, priorities and labels are not
  mirrored; the description is written once when the ticket is created.
- Attachments, screenshots and full events are not uploaded to tickets.
- One integration has one token; tickets and comments show its account
  as author.
- Target lists are read live and capped: 1,000 Jira projects, 100 Linear
  teams (250 labels and 100 projects each), 1,000 GitHub repositories,
  1,000 GitLab projects.
- The `create_ticket` action skips an issue that already has a ticket of
  the integration; two alerts for the same issue at the same moment are
  merged in one process, not across several servers (rupta runs as one
  writer anyway).
- Jira Cloud with scoped API tokens, OAuth apps, GitHub Apps and Linear
  OAuth applications are not supported.
- Jira transitions with required screens or fields cannot be used.
