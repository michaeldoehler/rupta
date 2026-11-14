// Every endpoint the web app calls, following docs/api/openapi.json of the
// server. Responses pass through the normalisers in adapt.ts. Calls marked
// "beyond the API" are not in the server's API yet; the UI degrades when
// they answer 404 (see the frontend report).

import {
  issueUpdateBody,
  normActivity,
  normAttachment,
  normAudit,
  normCheckin,
  normFeedback,
  normInvitation,
  normInvite,
  normIssue,
  normIssueDetail,
  normIssueStats,
  normMember,
  normMonitor,
  normRelease,
  normReleaseDetail,
  normRule,
  normSavedSearch,
  normTagSummary,
  normTeam,
  series,
} from './adapt'
import { request, requestList } from './client'
import { readPreferences, writePreferences } from './preferences'
import type {
  AlertRuleInput,
  ApiToken,
  ArtifactBundle,
  AuthConfig,
  Channel,
  EventSummary,
  Id,
  IssueQueryParams,
  IssueUpdate,
  Org,
  Page,
  Project,
  ProjectKey,
  ProjectRef,
  Role,
  RuptaEvent,
  Session,
  SsoConfig,
  SsoTestResult,
  TagValue,
  TokenScope,
  User,
  UserPreferences,
  UserProfile,
} from './types'

const e = encodeURIComponent

async function page<T>(p: Promise<Page<unknown>>, map: (v: unknown) => T): Promise<Page<T>> {
  const r = await p
  return { data: (r?.data ?? []).map(map), next_cursor: r?.next_cursor ?? null }
}

/** Search keys that map to columns of the event query endpoint, for value suggestions. */
export const EVENT_COLUMNS: Record<string, string> = {
  environment: 'environment',
  release: 'release',
  dist: 'dist',
  browser: 'browser',
  os: 'os',
  device: 'device',
  runtime: 'runtime',
  transaction: 'transaction',
  server_name: 'server_name',
  url: 'url',
  platform: 'platform',
  sdk: 'sdk',
  'user.email': 'user_email',
  'user.id': 'user_id',
  'error.type': 'exception_type',
}

/** Top values of an event column over the last 30 days (`GET /orgs/{org}/events?group_by=`). */
async function eventValues(org: string, column: string, project?: string[]): Promise<TagValue[]> {
  const r = await request<{ data: Record<string, unknown>[] }>(`/orgs/${e(org)}/events`, { query: { group_by: column, period: '30d', limit: 100, project } })
  return (r?.data ?? [])
    .filter((row) => row[column] !== null && row[column] !== undefined && row[column] !== '')
    .map((row) => ({ value: String(row[column]), count: Number(row.count ?? 0) }))
}

interface ServerPreferences {
  timezone?: string | null
  clock?: '24h' | '12h'
  theme?: 'system' | 'light' | 'dark'
  notifications?: { issue_alerts?: boolean; workflow?: 'all' | 'participating' | 'none'; weekly_report?: boolean }
}

/**
 * Preferences come from the server (`GET /user`); deploy notifications,
 * which the server does not store, and servers without preferences fall
 * back to this browser's copy.
 */
function profile(u: User & Partial<Omit<UserProfile, 'preferences'>> & { preferences?: ServerPreferences | null }): UserProfile {
  const local = readPreferences()
  const sp = u.preferences
  const preferences: UserPreferences = sp
    ? {
        ...local,
        timezone: sp.timezone ?? 'browser',
        clock_24h: sp.clock ? sp.clock === '24h' : local.clock_24h,
        theme: sp.theme ?? local.theme,
        notifications: { ...local.notifications, ...sp.notifications },
      }
    : local
  return { created_at: '', last_login_at: null, has_password: true, identities: [], ...u, preferences }
}

function serverPreferences(p: UserPreferences): ServerPreferences {
  const out: ServerPreferences = {}
  if (p.timezone !== undefined) out.timezone = p.timezone === 'browser' ? null : p.timezone
  if (p.clock_24h !== undefined) out.clock = p.clock_24h ? '24h' : '12h'
  if (p.theme !== undefined) out.theme = p.theme
  if (p.notifications) {
    const { deploys: _local, ...n } = p.notifications
    out.notifications = n
  }
  return out
}

export const api = {
  auth: {
    config: (org?: string) => request<AuthConfig>('/auth/config', { quiet401: true, query: { org } }),
    session: () => request<Session>('/auth/session', { quiet401: true }),
    login: (body: { email: string; password: string }) => request<Session>('/auth/login', { method: 'POST', body, quiet401: true }),
    logout: () => request<void>('/auth/logout', { method: 'POST', quiet401: true }),
    setup: (body: { email: string; name: string; password: string; org_name: string }) =>
      request<Session>('/auth/setup', { method: 'POST', body, quiet401: true }),
    ssoStartUrl: (providerId: string, next = '/') => `/auth/sso/${e(providerId)}/start?next=${e(next)}`,
  },

  invitations: {
    get: (token: string) => request<unknown>(`/invites/${e(token)}`, { quiet401: true }).then(normInvitation),
    accept: (token: string, body: { name?: string; password?: string }) =>
      request<Session>(`/invites/${e(token)}/accept`, { method: 'POST', body, quiet401: true }),
  },

  user: {
    get: () => request<User>('/user').then(profile),
    /** Name, avatar and preferences go to the server (a copy of the preferences stays in this browser). */
    update: async (body: Partial<Pick<UserProfile, 'name' | 'avatar_url' | 'preferences'>>): Promise<UserProfile> => {
      const { preferences, ...rest } = body
      if (preferences) writePreferences(preferences)
      const patch = { ...rest, ...(preferences ? { preferences: serverPreferences(preferences) } : {}) }
      const u = Object.keys(patch).length ? await request<User>('/user', { method: 'PATCH', body: patch }) : await request<User>('/user')
      return profile(u)
    },
    changePassword: (body: { current_password: string; new_password: string }) => request<unknown>('/user', { method: 'PATCH', body }),
    tokens: () => requestList<ApiToken>('/user/tokens'),
    createToken: (body: { name: string; scopes: TokenScope[]; expires_in_days?: number | null }) => request<ApiToken>('/user/tokens', { method: 'POST', body }),
    deleteToken: (id: Id) => request<void>(`/user/tokens/${e(id)}`, { method: 'DELETE' }),
    scopes: () => requestList<{ scope: string; description: string }>('/scopes'),
  },

  orgs: {
    list: () => requestList<Org>('/orgs'),
    create: (body: { name: string; slug?: string }) => request<Org>('/orgs', { method: 'POST', body }),
    get: (org: string) => request<Org>(`/orgs/${e(org)}`),
    update: (org: string, body: { name?: string; slug?: string }) => request<Org>(`/orgs/${e(org)}`, { method: 'PATCH', body }),
    remove: (org: string) => request<void>(`/orgs/${e(org)}`, { method: 'DELETE' }),

    members: (org: string) => requestList<unknown>(`/orgs/${e(org)}/members`, { query: { limit: 100 } }).then((l) => l.map(normMember)),
    invite: (org: string, body: { email: string; role: Role }) => request<unknown>(`/orgs/${e(org)}/members`, { method: 'POST', body }).then(normInvite),
    updateMember: (org: string, userId: Id, body: { role: Role }) => request<unknown>(`/orgs/${e(org)}/members/${e(userId)}`, { method: 'PATCH', body }),
    removeMember: (org: string, userId: Id) => request<void>(`/orgs/${e(org)}/members/${e(userId)}`, { method: 'DELETE' }),
    invites: (org: string) => requestList<unknown>(`/orgs/${e(org)}/invites`).then((l) => l.map(normInvite)),
    revokeInvite: (org: string, id: Id) => request<void>(`/orgs/${e(org)}/invites/${e(id)}`, { method: 'DELETE' }),

    teams: (org: string) => requestList<unknown>(`/orgs/${e(org)}/teams`).then((l) => l.map(normTeam)),
    createTeam: (org: string, body: { name: string; slug?: string }) => request<unknown>(`/orgs/${e(org)}/teams`, { method: 'POST', body }).then(normTeam),
    updateTeam: (org: string, team: string, body: { name?: string; slug?: string }) =>
      request<unknown>(`/orgs/${e(org)}/teams/${e(team)}`, { method: 'PATCH', body }).then(normTeam),
    deleteTeam: (org: string, team: string) => request<void>(`/orgs/${e(org)}/teams/${e(team)}`, { method: 'DELETE' }),
    addTeamMember: (org: string, team: string, userId: Id) => request<unknown>(`/orgs/${e(org)}/teams/${e(team)}/members/${e(userId)}`, { method: 'PUT' }),
    removeTeamMember: (org: string, team: string, userId: Id) => request<void>(`/orgs/${e(org)}/teams/${e(team)}/members/${e(userId)}`, { method: 'DELETE' }),

    auditLog: (org: string, params: { cursor?: string; action?: string; limit?: number }) =>
      page(request<Page<unknown>>(`/orgs/${e(org)}/audit-log`, { query: params }), normAudit),

    sso: (org: string) =>
      request<SsoConfig | null>(`/orgs/${e(org)}/sso`).catch((err: { status?: number }) => {
        if (err.status === 404) return null
        throw err
      }),
    saveSso: (org: string, body: Partial<SsoConfig>) => request<SsoConfig>(`/orgs/${e(org)}/sso`, { method: 'PUT', body }),
    deleteSso: (org: string) => request<void>(`/orgs/${e(org)}/sso`, { method: 'DELETE' }),
    /** Server-side discovery check of the issuer (the page falls back to a browser check on servers without it). */
    testSso: (org: string, body: Partial<SsoConfig>) =>
      request<Partial<SsoTestResult> & { problems?: string[] }>(`/orgs/${e(org)}/sso/test`, {
        method: 'POST',
        body: { issuer: body.issuer, client_id: body.client_id || undefined, client_secret: body.client_secret || undefined },
      }).then((r): SsoTestResult => ({
        ok: Boolean(r?.ok),
        issuer: r?.issuer,
        authorization_endpoint: r?.authorization_endpoint,
        problems: r?.problems ?? [],
        message: r?.message ?? (r?.problems?.length ? r.problems.join(' ') : r?.ok ? 'The server loaded the discovery document and found the endpoints it needs.' : 'The check failed.'),
      })),

    tokens: (org: string) => requestList<ApiToken>(`/orgs/${e(org)}/tokens`),
    createToken: (org: string, body: { name: string; scopes: TokenScope[]; expires_in_days?: number | null }) =>
      request<ApiToken>(`/orgs/${e(org)}/tokens`, { method: 'POST', body }),
    deleteToken: (org: string, id: Id) => request<void>(`/orgs/${e(org)}/tokens/${e(id)}`, { method: 'DELETE' }),

    channels: (org: string) => requestList<Channel>(`/orgs/${e(org)}/channels`),
    createChannel: (org: string, body: Omit<Channel, 'id' | 'created_at'>) => request<Channel>(`/orgs/${e(org)}/channels`, { method: 'POST', body }),
    updateChannel: (org: string, id: Id, body: Partial<Omit<Channel, 'id' | 'created_at'>>) =>
      request<Channel>(`/orgs/${e(org)}/channels/${e(id)}`, { method: 'PATCH', body }),
    deleteChannel: (org: string, id: Id) => request<void>(`/orgs/${e(org)}/channels/${e(id)}`, { method: 'DELETE' }),
    testChannel: (org: string, id: Id) =>
      request<{ ok?: boolean; message?: string } | undefined>(`/orgs/${e(org)}/channels/${e(id)}/test`, { method: 'POST' }).then((r) => ({
        ok: r?.ok ?? true,
        message: r?.message ?? 'Test notification sent.',
      })),

    environments: (org: string, project?: string[]) => eventValues(org, 'environment', project).then((l) => l.map((v) => v.value)),
    tagValues: (org: string, key: string, params: { project?: string[]; query?: string }) => {
      const column = EVENT_COLUMNS[key]
      if (!column) return Promise.resolve([] as TagValue[])
      const q = (params.query ?? '').toLowerCase()
      return eventValues(org, column, params.project).then((l) => l.filter((v) => v.value.toLowerCase().includes(q)).slice(0, 20))
    },

    savedSearches: (org: string) => requestList<unknown>(`/orgs/${e(org)}/saved-searches`).then((l) => l.map(normSavedSearch)),
    createSavedSearch: (org: string, body: { name: string; query: string; sort: string; is_shared: boolean }) =>
      request<unknown>(`/orgs/${e(org)}/saved-searches`, {
        method: 'POST',
        body: { name: body.name, query: body.query, sort: body.sort, shared: body.is_shared },
      }).then(normSavedSearch),
    deleteSavedSearch: (org: string, id: Id) => request<void>(`/orgs/${e(org)}/saved-searches/${e(id)}`, { method: 'DELETE' }),
  },

  projects: {
    list: (org: string) => requestList<Project>(`/orgs/${e(org)}/projects`),
    create: (org: string, body: { name: string; slug?: string; platform?: string; team?: Id | null }) =>
      request<Project>(`/orgs/${e(org)}/projects`, { method: 'POST', body }),
    get: (org: string, project: string) => request<Project>(`/projects/${e(org)}/${e(project)}`),
    update: (
      org: string,
      project: string,
      body: Partial<Pick<Project, 'name' | 'slug' | 'platform'>> & { team?: Id | null; settings?: Partial<Project['settings']> },
    ) => request<Project>(`/projects/${e(org)}/${e(project)}`, { method: 'PATCH', body }),
    remove: (org: string, project: string) => request<void>(`/projects/${e(org)}/${e(project)}`, { method: 'DELETE' }),

    keys: (org: string, project: string) => requestList<ProjectKey>(`/projects/${e(org)}/${e(project)}/keys`),
    createKey: (org: string, project: string, body: { label: string }) =>
      request<ProjectKey>(`/projects/${e(org)}/${e(project)}/keys`, { method: 'POST', body }),
    updateKey: (org: string, project: string, id: Id, body: Partial<Pick<ProjectKey, 'label' | 'is_active' | 'rate_limit_per_min'>>) =>
      request<ProjectKey>(`/projects/${e(org)}/${e(project)}/keys/${e(id)}`, { method: 'PATCH', body }),
    deleteKey: (org: string, project: string, id: Id) => request<void>(`/projects/${e(org)}/${e(project)}/keys/${e(id)}`, { method: 'DELETE' }),

    stats: (
      org: string,
      project: string,
      params: { stat: 'received' | 'rejected' | 'filtered' | 'rate_limited'; since?: number; until?: number; resolution?: string },
    ) => request<unknown>(`/projects/${e(org)}/${e(project)}/stats`, { query: params }).then(series),

    // beyond the API: source map bundles uploaded by the CLI
    bundles: (org: string, project: string) => requestList<ArtifactBundle>(`/projects/${e(org)}/${e(project)}/artifact-bundles`),
    deleteBundle: (org: string, project: string, id: Id) => request<void>(`/projects/${e(org)}/${e(project)}/artifact-bundles/${e(id)}`, { method: 'DELETE' }),

    alertRules: (org: string, project: ProjectRef) =>
      requestList<unknown>(`/projects/${e(org)}/${e(project.slug)}/alert-rules`).then((l) => l.map((r) => normRule(r, project))),
    alertRule: (org: string, project: ProjectRef, id: Id) =>
      request<unknown>(`/projects/${e(org)}/${e(project.slug)}/alert-rules/${e(id)}`).then((r) => normRule(r, project)),
    createAlertRule: (org: string, project: string, body: AlertRuleInput) =>
      request<unknown>(`/projects/${e(org)}/${e(project)}/alert-rules`, { method: 'POST', body }),
    updateAlertRule: (org: string, project: string, id: Id, body: Partial<AlertRuleInput>) =>
      request<unknown>(`/projects/${e(org)}/${e(project)}/alert-rules/${e(id)}`, { method: 'PATCH', body }),
    deleteAlertRule: (org: string, project: string, id: Id) => request<void>(`/projects/${e(org)}/${e(project)}/alert-rules/${e(id)}`, { method: 'DELETE' }),

    feedback: (org: string, project: ProjectRef, params: { cursor?: string } = {}) =>
      page(request<Page<unknown>>(`/projects/${e(org)}/${e(project.slug)}/feedback`, { query: params }), (f) => normFeedback(f, [project])),

    monitors: (org: string, project: ProjectRef) =>
      requestList<unknown>(`/projects/${e(org)}/${e(project.slug)}/monitors`).then((l) => l.map((m) => normMonitor(m, project))),
    monitor: (org: string, project: ProjectRef, slug: string) =>
      request<unknown>(`/projects/${e(org)}/${e(project.slug)}/monitors/${e(slug)}`).then((m) => normMonitor(m, project)),
    checkins: (org: string, project: string, slug: string, params: { cursor?: string } = {}) =>
      page(request<Page<unknown>>(`/projects/${e(org)}/${e(project)}/monitors/${e(slug)}/checkins`, { query: params }), normCheckin),
    updateMonitor: (org: string, project: string, slug: string, body: { status?: 'active' | 'disabled'; name?: string }) =>
      request<unknown>(`/projects/${e(org)}/${e(project)}/monitors/${e(slug)}`, { method: 'PATCH', body }),
  },

  issues: {
    list: (org: string, params: IssueQueryParams, signal?: AbortSignal) =>
      request<Page<unknown> & { total?: number }>(`/orgs/${e(org)}/issues`, { query: { ...params }, signal }).then((r) => ({
        data: (r?.data ?? []).map(normIssue),
        next_cursor: r?.next_cursor ?? null,
        total: r?.total,
      })),
    bulkUpdate: (org: string, ids: Id[], body: IssueUpdate) =>
      request<unknown>(`/orgs/${e(org)}/issues`, { method: 'PUT', query: { id: ids }, repeat: ['id'], body: issueUpdateBody(body) }),
    bulkDelete: (org: string, ids: Id[]) => request<void>(`/orgs/${e(org)}/issues`, { method: 'DELETE', query: { id: ids }, repeat: ['id'] }),
    // beyond the API: resolve a short id such as "WEB-12"
    byShortId: (org: string, shortId: string) => request<unknown>(`/orgs/${e(org)}/issues/by-short-id/${e(shortId)}`).then(normIssue),

    get: (id: Id) => request<unknown>(`/issues/${e(id)}`).then(normIssueDetail),
    update: (id: Id, body: IssueUpdate) => request<unknown>(`/issues/${e(id)}`, { method: 'PATCH', body: issueUpdateBody(body) }).then(normIssueDetail),
    remove: (id: Id) => request<void>(`/issues/${e(id)}`, { method: 'DELETE' }),
    events: (id: Id, params: { query?: string; cursor?: string; limit?: number }) => request<Page<EventSummary>>(`/issues/${e(id)}/events`, { query: params }),
    event: (id: Id, which: 'latest' | 'oldest' | 'recommended' | string) => request<RuptaEvent>(`/issues/${e(id)}/events/${e(which)}`),
    tags: (id: Id) => requestList<unknown>(`/issues/${e(id)}/tags`).then((l) => l.map(normTagSummary)),
    tagValues: (id: Id, key: string, params: { cursor?: string } = {}) =>
      request<Page<TagValue>>(`/issues/${e(id)}/tags/${e(key)}/values`, { query: { limit: 100, ...params } }),
    activity: (id: Id) => requestList<unknown>(`/issues/${e(id)}/activity`, { query: { limit: 100 } }).then((l) => l.map(normActivity)),
    addComment: (id: Id, text: string) => request<unknown>(`/issues/${e(id)}/comments`, { method: 'POST', body: { text } }).then(normActivity),
    updateComment: (id: Id, commentId: Id, text: string) =>
      request<unknown>(`/issues/${e(id)}/comments/${e(commentId)}`, { method: 'PATCH', body: { text } }).then(normActivity),
    deleteComment: (id: Id, commentId: Id) => request<void>(`/issues/${e(id)}/comments/${e(commentId)}`, { method: 'DELETE' }),
    stats: (id: Id, period: '24h' | '30d') => request<unknown>(`/issues/${e(id)}/stats`, { query: { period } }).then(normIssueStats),
    feedback: (id: Id, projects: ProjectRef[] = []) => requestList<unknown>(`/issues/${e(id)}/feedback`).then((l) => l.map((f) => normFeedback(f, projects))),
    attachments: (id: Id) => requestList<unknown>(`/issues/${e(id)}/attachments`).then((l) => l.map(normAttachment)),
    attachmentUrl: (attachmentId: Id) => `/api/v1/attachments/${e(attachmentId)}/download`,
  },

  events: {
    get: (org: string, project: string, eventId: string) => request<RuptaEvent>(`/orgs/${e(org)}/events/${e(project)}/${e(eventId)}`),
  },

  releases: {
    list: (org: string, params: { project?: string[]; query?: string; sort?: string; environment?: string[] }) =>
      requestList<unknown>(`/orgs/${e(org)}/releases`, { query: { ...params, limit: 100 } }).then((l) => l.map(normRelease)),
    get: (org: string, version: string) => request<unknown>(`/orgs/${e(org)}/releases/${e(version)}`).then(normReleaseDetail),
  },
}

export type Api = typeof api
