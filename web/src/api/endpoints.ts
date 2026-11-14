// Every endpoint the web app calls. Paths follow the design brief's REST API
// v1; endpoints marked "beyond contract" are listed in the frontend report.

import { request, requestList } from './client'
import type {
  Activity,
  AlertRule,
  AlertRuleInput,
  ApiToken,
  ArtifactBundle,
  Attachment,
  AuditEntry,
  AuthConfig,
  Channel,
  EventSummary,
  Feedback,
  Id,
  Invitation,
  Issue,
  IssueDetail,
  IssueQueryParams,
  IssueUpdate,
  Member,
  Monitor,
  Checkin,
  Org,
  Page,
  PendingInvite,
  Project,
  ProjectKey,
  Release,
  ReleaseDetail,
  Role,
  RuptaEvent,
  SavedSearch,
  Session,
  SsoConfig,
  SsoTestResult,
  TagDistribution,
  TagKey,
  TagValue,
  Team,
  TokenScope,
  User,
  UserProfile,
} from './types'

const e = encodeURIComponent

export const api = {
  auth: {
    config: () => request<AuthConfig>('/auth/config', { quiet401: true }),
    session: () => request<Session>('/auth/session', { quiet401: true }),
    login: (body: { email: string; password: string }) => request<{ user: User }>('/auth/login', { method: 'POST', body, quiet401: true }),
    logout: () => request<void>('/auth/logout', { method: 'POST', quiet401: true }),
    setup: (body: { email: string; name: string; password: string; org_name: string }) =>
      request<Session>('/auth/setup', { method: 'POST', body, quiet401: true }),
    ssoStartUrl: (providerId: string, next = '/') => `/auth/sso/${e(providerId)}/start?next=${e(next)}`,
  },

  // beyond contract: invitation lookup and acceptance
  invitations: {
    get: (token: string) => request<Invitation>(`/invitations/${e(token)}`, { quiet401: true }),
    accept: (token: string, body: { name?: string; password?: string }) =>
      request<Session>(`/invitations/${e(token)}/accept`, { method: 'POST', body, quiet401: true }),
  },

  user: {
    get: () => request<UserProfile>('/user'),
    update: (body: Partial<Pick<UserProfile, 'name' | 'avatar_url' | 'preferences'>>) => request<UserProfile>('/user', { method: 'PATCH', body }),
    // beyond contract
    changePassword: (body: { current_password: string; new_password: string }) => request<void>('/user/password', { method: 'POST', body }),
    tokens: () => requestList<ApiToken>('/user/tokens'),
    createToken: (body: { name: string; scopes: TokenScope[]; expires_at?: string | null }) => request<ApiToken>('/user/tokens', { method: 'POST', body }),
    deleteToken: (id: Id) => request<void>(`/user/tokens/${e(id)}`, { method: 'DELETE' }),
  },

  orgs: {
    list: () => requestList<Org>('/orgs'),
    create: (body: { name: string; slug?: string }) => request<Org>('/orgs', { method: 'POST', body }),
    get: (org: string) => request<Org>(`/orgs/${e(org)}`),
    update: (org: string, body: Partial<Pick<Org, 'name' | 'slug' | 'settings'>>) => request<Org>(`/orgs/${e(org)}`, { method: 'PATCH', body }),
    remove: (org: string) => request<void>(`/orgs/${e(org)}`, { method: 'DELETE' }),

    members: (org: string) => requestList<Member>(`/orgs/${e(org)}/members`),
    invite: (org: string, body: { email: string; role: Role; teams?: string[] }) => request<PendingInvite>(`/orgs/${e(org)}/members`, { method: 'POST', body }),
    updateMember: (org: string, userId: Id, body: { role: Role }) => request<Member>(`/orgs/${e(org)}/members/${e(userId)}`, { method: 'PATCH', body }),
    removeMember: (org: string, userId: Id) => request<void>(`/orgs/${e(org)}/members/${e(userId)}`, { method: 'DELETE' }),
    // beyond contract: pending invitations
    invites: (org: string) => requestList<PendingInvite>(`/orgs/${e(org)}/invitations`),
    revokeInvite: (org: string, id: Id) => request<void>(`/orgs/${e(org)}/invitations/${e(id)}`, { method: 'DELETE' }),

    teams: (org: string) => requestList<Team>(`/orgs/${e(org)}/teams`),
    createTeam: (org: string, body: { name: string; slug?: string }) => request<Team>(`/orgs/${e(org)}/teams`, { method: 'POST', body }),
    updateTeam: (org: string, team: string, body: { name?: string; slug?: string }) =>
      request<Team>(`/orgs/${e(org)}/teams/${e(team)}`, { method: 'PATCH', body }),
    deleteTeam: (org: string, team: string) => request<void>(`/orgs/${e(org)}/teams/${e(team)}`, { method: 'DELETE' }),
    addTeamMember: (org: string, team: string, userId: Id) => request<void>(`/orgs/${e(org)}/teams/${e(team)}/members/${e(userId)}`, { method: 'PUT' }),
    removeTeamMember: (org: string, team: string, userId: Id) => request<void>(`/orgs/${e(org)}/teams/${e(team)}/members/${e(userId)}`, { method: 'DELETE' }),

    auditLog: (org: string, params: { cursor?: string; action?: string; actor?: string; limit?: number }) =>
      request<Page<AuditEntry>>(`/orgs/${e(org)}/audit-log`, { query: params }),

    sso: (org: string) => request<SsoConfig | null>(`/orgs/${e(org)}/sso`),
    saveSso: (org: string, body: Partial<SsoConfig>) => request<SsoConfig>(`/orgs/${e(org)}/sso`, { method: 'PUT', body }),
    // beyond contract
    testSso: (org: string, body: Partial<SsoConfig>) => request<SsoTestResult>(`/orgs/${e(org)}/sso/test`, { method: 'POST', body }),

    tokens: (org: string) => requestList<ApiToken>(`/orgs/${e(org)}/tokens`),
    createToken: (org: string, body: { name: string; scopes: TokenScope[]; expires_at?: string | null }) =>
      request<ApiToken>(`/orgs/${e(org)}/tokens`, { method: 'POST', body }),
    deleteToken: (org: string, id: Id) => request<void>(`/orgs/${e(org)}/tokens/${e(id)}`, { method: 'DELETE' }),

    channels: (org: string) => requestList<Channel>(`/orgs/${e(org)}/channels`),
    createChannel: (org: string, body: Omit<Channel, 'id' | 'created_at'>) => request<Channel>(`/orgs/${e(org)}/channels`, { method: 'POST', body }),
    updateChannel: (org: string, id: Id, body: Partial<Omit<Channel, 'id' | 'created_at'>>) =>
      request<Channel>(`/orgs/${e(org)}/channels/${e(id)}`, { method: 'PATCH', body }),
    deleteChannel: (org: string, id: Id) => request<void>(`/orgs/${e(org)}/channels/${e(id)}`, { method: 'DELETE' }),
    testChannel: (org: string, id: Id) => request<{ ok: boolean; message: string }>(`/orgs/${e(org)}/channels/${e(id)}/test`, { method: 'POST' }),

    // beyond contract: search helpers for the issue search bar and filters
    environments: (org: string, project?: string[]) => requestList<string>(`/orgs/${e(org)}/environments`, { query: { project } }),
    tagKeys: (org: string, project?: string[]) => requestList<TagKey>(`/orgs/${e(org)}/tags`, { query: { project } }),
    tagValues: (org: string, key: string, params: { project?: string[]; query?: string }) =>
      requestList<TagValue>(`/orgs/${e(org)}/tags/${e(key)}/values`, { query: params }),

    // beyond contract: saved searches
    savedSearches: (org: string) => requestList<SavedSearch>(`/orgs/${e(org)}/saved-searches`),
    createSavedSearch: (org: string, body: { name: string; query: string; sort: string; is_shared: boolean }) =>
      request<SavedSearch>(`/orgs/${e(org)}/saved-searches`, { method: 'POST', body }),
    deleteSavedSearch: (org: string, id: Id) => request<void>(`/orgs/${e(org)}/saved-searches/${e(id)}`, { method: 'DELETE' }),
  },

  projects: {
    list: (org: string) => requestList<Project>(`/orgs/${e(org)}/projects`),
    create: (org: string, body: { name: string; slug?: string; platform?: string; team_id?: Id | null }) =>
      request<Project>(`/orgs/${e(org)}/projects`, { method: 'POST', body }),
    get: (org: string, project: string) => request<Project>(`/projects/${e(org)}/${e(project)}`),
    update: (
      org: string,
      project: string,
      body: Partial<Pick<Project, 'name' | 'slug' | 'platform'>> & { team_id?: Id | null; settings?: Partial<Project['settings']> },
    ) => request<Project>(`/projects/${e(org)}/${e(project)}`, { method: 'PATCH', body }),
    remove: (org: string, project: string) => request<void>(`/projects/${e(org)}/${e(project)}`, { method: 'DELETE' }),

    keys: (org: string, project: string) => requestList<ProjectKey>(`/projects/${e(org)}/${e(project)}/keys`),
    createKey: (org: string, project: string, body: { label: string }) =>
      request<ProjectKey>(`/projects/${e(org)}/${e(project)}/keys`, { method: 'POST', body }),
    updateKey: (org: string, project: string, id: Id, body: Partial<Pick<ProjectKey, 'label' | 'is_active' | 'rate_limit_per_min'>>) =>
      request<ProjectKey>(`/projects/${e(org)}/${e(project)}/keys/${e(id)}`, { method: 'PATCH', body }),
    deleteKey: (org: string, project: string, id: Id) => request<void>(`/projects/${e(org)}/${e(project)}/keys/${e(id)}`, { method: 'DELETE' }),

    stats: (org: string, project: string, params: { stat: 'received' | 'rejected'; since?: number; until?: number; resolution?: string }) =>
      request<[number, number][]>(`/projects/${e(org)}/${e(project)}/stats`, { query: params }),

    // beyond contract: source map bundles uploaded by the CLI
    bundles: (org: string, project: string) => requestList<ArtifactBundle>(`/projects/${e(org)}/${e(project)}/artifact-bundles`),
    deleteBundle: (org: string, project: string, id: Id) => request<void>(`/projects/${e(org)}/${e(project)}/artifact-bundles/${e(id)}`, { method: 'DELETE' }),

    alertRules: (org: string, project: string) => requestList<AlertRule>(`/projects/${e(org)}/${e(project)}/alert-rules`),
    alertRule: (org: string, project: string, id: Id) => request<AlertRule>(`/projects/${e(org)}/${e(project)}/alert-rules/${e(id)}`),
    createAlertRule: (org: string, project: string, body: AlertRuleInput) =>
      request<AlertRule>(`/projects/${e(org)}/${e(project)}/alert-rules`, { method: 'POST', body }),
    updateAlertRule: (org: string, project: string, id: Id, body: Partial<AlertRuleInput>) =>
      request<AlertRule>(`/projects/${e(org)}/${e(project)}/alert-rules/${e(id)}`, { method: 'PATCH', body }),
    deleteAlertRule: (org: string, project: string, id: Id) => request<void>(`/projects/${e(org)}/${e(project)}/alert-rules/${e(id)}`, { method: 'DELETE' }),

    feedback: (org: string, project: string, params: { cursor?: string; query?: string } = {}) =>
      request<Page<Feedback>>(`/projects/${e(org)}/${e(project)}/feedback`, { query: params }),

    monitors: (org: string, project: string) => requestList<Monitor>(`/projects/${e(org)}/${e(project)}/monitors`),
    monitor: (org: string, project: string, slug: string) => request<Monitor>(`/projects/${e(org)}/${e(project)}/monitors/${e(slug)}`),
    // beyond contract: check-in history page
    checkins: (org: string, project: string, slug: string, params: { cursor?: string } = {}) =>
      request<Page<Checkin>>(`/projects/${e(org)}/${e(project)}/monitors/${e(slug)}/checkins`, { query: params }),
    createMonitor: (org: string, project: string, body: Partial<Monitor>) =>
      request<Monitor>(`/projects/${e(org)}/${e(project)}/monitors`, { method: 'POST', body }),
    updateMonitor: (org: string, project: string, slug: string, body: Partial<Monitor>) =>
      request<Monitor>(`/projects/${e(org)}/${e(project)}/monitors/${e(slug)}`, { method: 'PATCH', body }),
    deleteMonitor: (org: string, project: string, slug: string) => request<void>(`/projects/${e(org)}/${e(project)}/monitors/${e(slug)}`, { method: 'DELETE' }),
  },

  issues: {
    list: (org: string, params: IssueQueryParams, signal?: AbortSignal) =>
      request<Page<Issue>>(`/orgs/${e(org)}/issues`, {
        query: { ...params, project: params.project, environment: params.environment },
        signal,
      }),
    bulkUpdate: (org: string, ids: Id[], body: IssueUpdate) =>
      request<{ updated: number }>(`/orgs/${e(org)}/issues`, { method: 'PUT', query: { id: ids }, repeat: ['id'], body }),
    bulkDelete: (org: string, ids: Id[]) => request<void>(`/orgs/${e(org)}/issues`, { method: 'DELETE', query: { id: ids }, repeat: ['id'] }),
    // beyond contract: resolve "PROJ-12" to an issue
    byShortId: (org: string, shortId: string) => request<Issue>(`/orgs/${e(org)}/issues/by-short-id/${e(shortId)}`),

    get: (id: Id) => request<IssueDetail>(`/issues/${e(id)}`),
    update: (id: Id, body: IssueUpdate) => request<IssueDetail>(`/issues/${e(id)}`, { method: 'PATCH', body }),
    events: (id: Id, params: { query?: string; cursor?: string; limit?: number }) => request<Page<EventSummary>>(`/issues/${e(id)}/events`, { query: params }),
    event: (id: Id, which: 'latest' | 'oldest' | 'recommended' | string) => request<RuptaEvent>(`/issues/${e(id)}/events/${e(which)}`),
    tags: (id: Id) => requestList<TagDistribution>(`/issues/${e(id)}/tags`),
    tagValues: (id: Id, key: string, params: { cursor?: string } = {}) =>
      request<Page<TagValue> & { key: string; total_values: number }>(`/issues/${e(id)}/tags/${e(key)}/values`, { query: params }),
    activity: (id: Id) => requestList<Activity>(`/issues/${e(id)}/activity`),
    addComment: (id: Id, text: string) => request<Activity>(`/issues/${e(id)}/comments`, { method: 'POST', body: { text } }),
    updateComment: (id: Id, commentId: Id, text: string) => request<Activity>(`/issues/${e(id)}/comments/${e(commentId)}`, { method: 'PATCH', body: { text } }),
    deleteComment: (id: Id, commentId: Id) => request<void>(`/issues/${e(id)}/comments/${e(commentId)}`, { method: 'DELETE' }),
    stats: (id: Id, period: '24h' | '30d') => request<[number, number][]>(`/issues/${e(id)}/stats`, { query: { period } }),
    feedback: (id: Id) => requestList<Feedback>(`/issues/${e(id)}/feedback`),
    attachments: (id: Id) => requestList<Attachment>(`/issues/${e(id)}/attachments`),
    attachmentUrl: (attachmentId: Id) => `/api/v1/attachments/${e(attachmentId)}/download`,
  },

  events: {
    get: (org: string, project: string, eventId: string) => request<RuptaEvent>(`/orgs/${e(org)}/events/${e(project)}/${e(eventId)}`),
  },

  releases: {
    list: (org: string, params: { project?: string[]; query?: string; sort?: string; environment?: string[] }) =>
      requestList<Release>(`/orgs/${e(org)}/releases`, { query: params }),
    get: (org: string, version: string, params: { project?: string[]; period?: string } = {}) =>
      request<ReleaseDetail>(`/orgs/${e(org)}/releases/${e(version)}`, { query: params }),
  },
}

export type Api = typeof api
