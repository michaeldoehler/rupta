# MCP server

rupta serves the [Model Context Protocol](https://modelcontextprotocol.io)
(MCP) at `/mcp`. MCP clients, such as agents in editors, chat applications
and command-line tools, can then search issues, read events with their
stack traces, look at traces, logs, metrics, releases, cron monitors and
alerts, and triage issues, with the same permissions as the REST API.

The endpoint is part of the server and on by default. Turn it off with
`RUPTA_MCP=false` ([configuration](configuration.md#server)); `/mcp` and
the metadata below then answer `404`.

## Connecting a client

The server URL is `<RUPTA_PUBLIC_URL>/mcp`, for example
`https://errors.example.com/mcp`. Clients authenticate with a rupta API
token in the `Authorization` header.

1. Create a token under *Settings → API tokens* (or `POST
   /api/v1/user/tokens`; see [authentication](authentication.md#api-tokens)).
   Give it only the scopes the client needs:
   - reading: `org:read`, `project:read`, `member:read`;
   - triage (resolve, ignore, assign, comment, create tickets): add
     `issue:write`.

   A personal token acts as you, limited by your role in each
   organization. An organization token works in its organization only and
   cannot comment. Set an expiry (`expires_in_days`) for tokens you hand to
   a tool.
2. Configure the client with the URL and the header. The transport is
   called *Streamable HTTP* (`http` or `streamable-http` in most
   configuration files).

Clients that read an `mcpServers` object:

```json
{
  "mcpServers": {
    "rupta": {
      "type": "http",
      "url": "https://errors.example.com/mcp",
      "headers": { "Authorization": "Bearer rpt_..." }
    }
  }
}
```

Visual Studio Code (`.vscode/mcp.json`), with the token asked for once and
kept out of the file:

```json
{
  "inputs": [
    { "id": "rupta-token", "type": "promptString", "description": "rupta API token", "password": true }
  ],
  "servers": {
    "rupta": {
      "type": "http",
      "url": "https://errors.example.com/mcp",
      "headers": { "Authorization": "Bearer ${input:rupta-token}" }
    }
  }
}
```

Clients that only start local servers over standard input and output can
reach rupta through the `mcp-remote` bridge:

```json
{
  "mcpServers": {
    "rupta": {
      "command": "npx",
      "args": ["-y", "mcp-remote", "https://errors.example.com/mcp", "--header", "Authorization:${AUTH_HEADER}"],
      "env": { "AUTH_HEADER": "Bearer rpt_..." }
    }
  }
}
```

To check the connection from a shell, the MCP Inspector has a command-line
mode:

```sh
npx @modelcontextprotocol/inspector --cli https://errors.example.com/mcp \
  --transport http --header "Authorization: Bearer $RUPTA_TOKEN" --method tools/list
npx @modelcontextprotocol/inspector --cli https://errors.example.com/mcp \
  --transport http --header "Authorization: Bearer $RUPTA_TOKEN" \
  --method tools/call --tool-name search_issues --tool-arg org=acme --tool-arg 'query=is:unresolved level:error'
```

Or by hand:

```sh
curl -s https://errors.example.com/mcp \
  -H "Authorization: Bearer $RUPTA_TOKEN" -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"curl","version":"1"}}}'
```

## Tools

Arguments are described in each tool's JSON schema; clients show them.
Ids are strings. Issues can be given by id (`1203`), by short id (`WEB-31`,
with `org`) or by their link in the web app. Lists take `limit` and return
`next_cursor`; pass it back as `cursor` for the next page.

Reading (annotated `readOnlyHint`):

| Tool | What it returns | REST API |
| --- | --- | --- |
| `list_organizations` | organizations of the token, with the caller's role | `GET /orgs` |
| `list_projects` | projects of an organization | `GET /orgs/{org}/projects` |
| `list_members` | members (user ids for assignment) | `GET /orgs/{org}/members` |
| `list_teams` | teams with members (team ids for assignment) | `GET /orgs/{org}/teams` |
| `search_issues` | issues matching a query in the [search language](search.md), with status, level, project, environment, period and sort | `GET /orgs/{org}/issues` |
| `get_issue` | one issue: state, counts, first/last seen, releases, assignee, tag distribution, linked tickets, recent activity | `GET /issues/{id}`, `.../tags`, `.../links`, `.../activity` |
| `get_issue_tag_values` | all values of one tag of an issue | `GET /issues/{id}/tags/{key}/values` |
| `list_issue_events` | events of an issue, optionally filtered | `GET /issues/{id}/events` |
| `get_event` | an event as text: exception chain, stack trace with source context of in-app frames, breadcrumbs, request, user, contexts, tags, extra, SDK | `GET /issues/{id}/events/{event}`, `GET /orgs/{org}/events/{project}/{event_id}` |
| `list_issue_activity` | comments and history of an issue | `GET /issues/{id}/activity` |
| `list_releases` | releases with health and adoption | `GET /orgs/{org}/releases` |
| `get_release` | one release: deploys, health per project, daily sessions | `GET /orgs/{org}/releases/{version}` |
| `get_project_health` | crash-free sessions and users of a project over a period | `GET /projects/{org}/{project}/health` |
| `search_transactions` | transactions by name with throughput, percentiles, failure rate, Apdex | `GET /orgs/{org}/performance/transactions` |
| `get_transaction_summary` | one transaction: time series, slowest samples, related issues, web vitals | `GET /orgs/{org}/performance/transactions/summary` |
| `get_trace` | a trace as a span tree, with its transactions and errors | `GET /orgs/{org}/traces/{trace_id}` |
| `search_spans` | spans grouped by op, description or transaction, or single spans | `GET /orgs/{org}/spans` |
| `search_logs` | logs with level, text, trace and attribute filters | `GET /orgs/{org}/logs` |
| `list_metrics` | metric names with type and unit | `GET /orgs/{org}/metrics/names` |
| `query_metric` | a metric aggregated over time, optionally grouped | `GET /orgs/{org}/metrics/query` |
| `list_monitors` | cron monitors with health and next check-in | `GET /projects/{org}/{project}/monitors` |
| `list_monitor_checkins` | check-ins of a monitor | `GET .../monitors/{slug}/checkins` |
| `list_alert_rules` | alert rules of a project | `GET /projects/{org}/{project}/alert-rules` |
| `list_alert_firings` | when alert rules fired, for which issue | `GET /projects/{org}/{project}/alert-fires` |
| `list_integrations` | issue tracker integrations, and a project's default target | `GET /orgs/{org}/integrations`, `GET /projects/{org}/{project}/integrations` |

Changing (annotated `readOnlyHint: false`, `destructiveHint: false`; every
change can be undone):

| Tool | What it does | Needs | REST API |
| --- | --- | --- | --- |
| `update_issue` | resolve (also in a release or the next release), unresolve, ignore (forever, for a time, until more events or users, until it escalates), assign or unassign, set the priority, bookmark | `issue:write` and the member role; bookmarks: any role | `PATCH /issues/{id}` |
| `add_comment` | comment on an issue as the token's user | `issue:write`, the member role, a personal token | `POST /issues/{id}/comments` |
| `create_ticket` | create a ticket in Jira, Linear, GitHub or GitLab through an existing integration and link it (annotated `openWorldHint`) | `issue:write` and the member role | `POST /issues/{id}/links` (`mode: create`) |
| `link_ticket` | link an existing ticket by key or URL | `issue:write` and the member role | `POST /issues/{id}/links` (`mode: link`) |

The tools do what the REST API offers and nothing more. Not available
through MCP: deleting or merging issues (merging does not exist in rupta),
changing projects, alert rules, channels, integrations, members or
tokens, profiles, session replays, user feedback, attachments and
dashboards.

## Resources and prompts

Resources:

| URI | Content |
| --- | --- |
| `rupta://docs/search` | the [query language](search.md) |
| `rupta://docs/event-fields` | the [event fields](event-fields.md) |
| `rupta://docs/grouping` | how events become issues ([grouping](grouping.md)) |
| `rupta://issue/{id}` | an issue as `get_issue` returns it (JSON) |
| `rupta://issue/{id}/event/{event}` | an event as `get_event` renders it; `event` is `latest`, `oldest`, `recommended` or an event id |

Issue and event resources are marked as private to the caller and not
cacheable.

Prompts, which clients usually offer as commands:

| Prompt | Arguments | Guides the agent through |
| --- | --- | --- |
| `triage_new_issues` | `org`, `project`, `period` | new, regressed and escalating issues; a proposal of priority, owner and status, applied only after confirmation |
| `investigate_issue` | `issue`, `org` | events, stack trace, breadcrumbs, tags, the trace and its logs, releases; the likely cause and a fix |
| `check_release` | `org`, `version` | health, new issues and regressions of a release |

## Permissions

Every tool calls the REST API in process, as the caller: the token's
organization, the user's role and the token's scopes apply exactly as
described in [authentication](authentication.md#roles), with no checks of
its own. In particular:

- Resources of organizations the token cannot access answer *not found*,
  exactly like resources that do not exist.
- A missing scope or role answers *forbidden*; the message tells the agent
  not to retry.
- Changes are recorded in the issue's activity as the token's user (or as
  nobody for organization tokens), and a ticket created by a tool is
  created by the user, like in the web app.

Errors of a tool are returned as tool results with `isError: true` and a
message for the agent (*Not found…*, *Forbidden…*, *Invalid request
(argument query)…*). Unknown tools are a JSON-RPC error.

## Protocol

- Transport: Streamable HTTP, `POST /mcp`, implemented with the official
  Rust SDK (`rmcp` 3.10). Protocol revisions 2024-11-05, 2025-03-26,
  2025-06-18, 2025-11-25 and 2026-07-28 are accepted; the client's revision
  is negotiated in `initialize` (or per request with 2026-07-28).
- Stateless: rupta does not create sessions (no `Mcp-Session-Id`), and
  every request is authenticated on its own. Answers are plain JSON;
  `GET /mcp` (a server-initiated event stream) answers `405`. The server
  sends no notifications or requests to the client.
- Requests are limited to 1 MiB.

## Authentication and OAuth

Requests without a valid token get `401` with

```http
WWW-Authenticate: Bearer resource_metadata="https://errors.example.com/.well-known/oauth-protected-resource/mcp"
```

(and `error="invalid_token"` when a token was sent but is unknown, expired
or revoked). The OAuth 2.0 Protected Resource Metadata (RFC 9728) is served
at `/.well-known/oauth-protected-resource` and
`/.well-known/oauth-protected-resource/mcp`:

```json
{
  "resource": "https://errors.example.com/mcp",
  "resource_name": "rupta",
  "bearer_methods_supported": ["header"],
  "scopes_supported": ["org:read", "org:write", "..."],
  "authorization_servers": ["https://auth.example.com"],
  "resource_documentation": "https://github.com/michaeldoehler/rupta/blob/main/docs/mcp.md"
}
```

`authorization_servers` names `RUPTA_OIDC_ISSUER` and is left out when no
instance-wide single sign-on is configured.

**Limit:** rupta accepts only its own API tokens at `/mcp`. It does not yet
accept access tokens issued by the identity provider, and it is not an
authorization server itself, so the OAuth sign-in that some clients start
after a `401` does not lead to a working connection. Configure the
`Authorization` header with a rupta token instead, as shown above.
Session cookies of the web app are not accepted either.

## Limits

- A tool result has at most 50,000 characters. Lists that would be longer
  are shortened and say so (`truncated`); ask for fewer items or narrow the
  query.
- Page sizes: up to 100 items (logs up to 200).
- `get_event` shows up to 60 frames per stack trace, collapses library
  frames unless `all_frames` is set (the frame that raised is always
  shown), shows all source lines the SDK sent for the innermost in-app
  frame and two lines around the other in-app frames, and the last 20
  breadcrumbs (`max_breadcrumbs`, at most 100). Long values are cut.
- `get_trace` shows 200 spans (`max_spans`, at most 2,000); traces with
  more than 10,000 spans are loaded partially, as in the web app.
- Telemetry tools default to the last 24 hours (metric names: 7 days) and
  accept `period` or `start`/`end` up to 366 days.
- The limits of the REST API apply otherwise, for example at most 32
  search terms ([search](search.md#errors-and-limits)).

## Security notes

- A token used by an agent can do everything its scopes and its user's
  role allow. Give agents tokens of their own, with the fewest scopes
  (without `issue:write` they cannot change anything), an expiry, and
  revoke them when no longer needed. `last_used_at` of the token shows when
  it was used.
- Tool results contain data your applications sent: error messages,
  breadcrumbs, request data, log lines, tags. They are input from your
  users and can contain text written to mislead an agent (prompt
  injection). Treat them as data; keep the client's confirmation for
  changing tools, which are annotated as such, and do not give agents more
  scopes than the task needs. Server-side [data scrubbing](data-scrubbing.md)
  applies, so filtered values stay filtered.
- Browser requests are refused (`403`) when their `Origin` is neither the
  origin of `RUPTA_PUBLIC_URL` nor listed in `RUPTA_CORS_ALLOWED_ORIGINS`.
  This protects against DNS rebinding. Clients outside a browser send no
  `Origin` and are not affected. The `Host` header is not checked, so the
  endpoint works behind proxies and under internal host names.
- Requests go through the same HTTP stack as the REST API: TLS at the
  reverse proxy, request limits, logs and metrics.
