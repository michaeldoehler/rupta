// Maps API responses to the view types in types.ts, and view-level request
// shapes to what the server expects. The normalisers are tolerant: they
// accept the server's shapes (docs/api/openapi.json) and the richer shapes
// of earlier drafts, so small server changes stay local to this file.

import type {
  Activity,
  AlertAction,
  AlertFilter,
  AlertRule,
  Assignee,
  Attachment,
  AuditEntry,
  Checkin,
  Feedback,
  Invitation,
  Issue,
  IssueDetail,
  IssueUpdate,
  Level,
  Member,
  Monitor,
  PendingInvite,
  ProjectRef,
  Release,
  ReleaseDetail,
  ReleaseHealthPoint,
  ReleaseRef,
  Role,
  SavedSearch,
  TagDistribution,
  Team,
} from './types'

type Raw = Record<string, unknown>
const obj = (v: unknown): Raw => (v && typeof v === 'object' ? (v as Raw) : {})
const str = (v: unknown): string | null => (v === null || v === undefined ? null : String(v))
const num = (v: unknown, d = 0): number => {
  const n = typeof v === 'number' ? v : Number(v)
  return Number.isFinite(n) ? n : d
}

export function series(v: unknown): [number, number][] {
  return Array.isArray(v) ? v.map((p) => [num((p as unknown[])[0]), num((p as unknown[])[1])] as [number, number]) : []
}

function releaseRef(v: unknown): ReleaseRef | null {
  if (!v) return null
  if (typeof v === 'string') return { version: v, date: null }
  const o = obj(v)
  return { version: String(o.version), date: str(o.date) }
}

function assignee(v: unknown): Assignee | null {
  if (!v) return null
  const o = obj(v)
  return { type: o.type === 'team' ? 'team' : 'user', id: String(o.id), name: String(o.name ?? ''), email: str(o.email) ?? undefined }
}

export function normIssue(raw: unknown): Issue {
  const o = obj(raw)
  return {
    ...(o as unknown as Issue),
    count: num(o.count),
    user_count: num(o.user_count),
    substatus: (str(o.substatus) as Issue['substatus']) ?? null,
    assignee: assignee(o.assignee),
    metadata: obj(o.metadata),
    stats: series(o.stats),
    is_bookmarked: Boolean(o.is_bookmarked),
    has_seen: o.has_seen === undefined ? true : Boolean(o.has_seen),
  }
}

export function normIssueDetail(raw: unknown): IssueDetail {
  const o = obj(raw)
  const base = normIssue(raw)
  return {
    ...base,
    first_release: releaseRef(o.first_release),
    last_release: releaseRef(o.last_release),
    // `resolved_in_mode: "next_release"` has no version yet.
    resolved_in_release: str(o.resolved_in_release) ?? (o.resolved_in_mode === 'next_release' || o.substatus === 'in_next_release' ? 'next' : null),
    ignore_until: str(o.ignore_until),
    ignore_count: o.ignore_count === null || o.ignore_count === undefined ? null : num(o.ignore_count),
    permalink: str(o.permalink) ?? undefined,
  }
}

/** `GET /issues/{id}/stats`: `{events, users, ...}` or a bare series. */
export function normIssueStats(raw: unknown): [number, number][] {
  return Array.isArray(raw) ? series(raw) : series(obj(raw).events)
}

/** View-level issue update to the server's body (`assignee` as "user:{id}"). */
export function issueUpdateBody(u: IssueUpdate): Record<string, unknown> {
  const { assignee: a, resolved_in_next_release, ...rest } = u
  const body: Record<string, unknown> = { ...rest }
  if (a !== undefined) body.assignee = a ? `${a.type}:${a.id}` : null
  if (resolved_in_next_release) {
    body.status = 'resolved'
    body.resolved_in = 'next_release'
  }
  return body
}

export function normMember(raw: unknown): Member {
  const o = obj(raw)
  if (o.user) return o as unknown as Member
  return {
    user: { id: String(o.user_id), email: String(o.email ?? ''), name: String(o.name ?? ''), avatar_url: str(o.avatar_url), is_superuser: false },
    role: o.role as Role,
    created_at: String(o.created_at ?? ''),
    teams: (o.teams as string[]) ?? [],
    sso: Boolean(o.sso ?? o.sso_linked),
  }
}

export function normTeam(raw: unknown): Team {
  const o = obj(raw)
  const members = ((o.members as unknown[]) ?? []).map((m) => {
    const x = obj(m)
    return { id: String(x.id ?? x.user_id), email: String(x.email ?? ''), name: String(x.name ?? ''), avatar_url: str(x.avatar_url), is_superuser: false }
  })
  return {
    id: String(o.id),
    slug: String(o.slug),
    name: String(o.name),
    created_at: String(o.created_at ?? ''),
    members,
    projects: (o.projects as Team['projects']) ?? [],
  }
}

export function normInvite(raw: unknown): PendingInvite {
  const o = obj(raw)
  return {
    id: String(o.id),
    email: String(o.email),
    role: o.role as Role,
    created_at: String(o.created_at ?? ''),
    expires_at: String(o.expires_at ?? ''),
    accept_url: str(o.accept_url),
    email_sent: o.email_sent === undefined || o.email_sent === null ? null : Boolean(o.email_sent),
  }
}

export function normInvitation(raw: unknown): Invitation {
  const o = obj(raw)
  if (o.org) return o as unknown as Invitation
  return {
    org: { slug: String(o.org_slug ?? ''), name: String(o.org_name ?? '') },
    email: String(o.email),
    role: o.role as Role,
    invited_by: str(o.invited_by),
    expires_at: String(o.expires_at ?? ''),
    user_exists: Boolean(o.user_exists),
  }
}

export function normAudit(raw: unknown): AuditEntry {
  const o = obj(raw)
  const a = obj(o.actor)
  const actor: AuditEntry['actor'] =
    'name' in a && 'type' in a
      ? (a as unknown as AuditEntry['actor'])
      : a.user_id
        ? { type: 'user', id: String(a.user_id), name: String(a.email ?? `user ${a.user_id}`) }
        : a.token_id
          ? { type: 'token', id: String(a.token_id), name: `token ${a.token_id}` }
          : { type: 'system', id: null, name: 'system' }
  return { ...(o as unknown as AuditEntry), actor, data: obj(o.data) }
}

export function normFeedback(raw: unknown, projects: ProjectRef[] = []): Feedback {
  const o = obj(raw)
  const pid = String(o.project_id ?? obj(o.project).id ?? '')
  const project = (o.project as ProjectRef | undefined) ?? projects.find((p) => p.id === pid) ?? { id: pid, slug: `project ${pid}`, name: '', platform: null }
  const issue = o.issue !== undefined ? (o.issue as Feedback['issue']) : o.issue_id ? { id: String(o.issue_id), short_id: '', title: '' } : null
  return { ...(o as unknown as Feedback), project, issue }
}

export function normAttachment(raw: unknown): Attachment {
  const o = obj(raw)
  return { ...(o as unknown as Attachment), size: num(o.size), download_url: str(o.download_url) }
}

export function normActivity(raw: unknown): Activity {
  const o = obj(raw)
  return { ...(o as unknown as Activity), data: obj(o.data) }
}

export function normTagSummary(raw: unknown): TagDistribution {
  const o = obj(raw)
  return {
    key: String(o.key),
    name: String(o.name ?? o.key),
    total_values: num(o.total_values),
    unique_values: num(o.unique_values),
    top_values: ((o.top_values as unknown[]) ?? []).map((v) => ({ ...obj(v), value: String(obj(v).value), count: num(obj(v).count) })),
  }
}

export function normSavedSearch(raw: unknown): SavedSearch {
  const o = obj(raw)
  return { ...(o as unknown as SavedSearch), is_shared: Boolean(o.is_shared ?? o.shared) }
}

export function normRelease(raw: unknown): Release {
  const o = obj(raw)
  const h = obj(o.health)
  const hasHealth = Object.keys(h).length > 0
  return {
    version: String(o.version),
    short_version: str(o.short_version),
    ref: str(o.ref),
    url: str(o.url),
    date_created: String(o.date_created),
    date_released: str(o.date_released),
    first_event: str(o.first_event),
    last_event: str(o.last_event),
    projects: ((o.projects as unknown[]) ?? []).map((p) => {
      const x = obj(p)
      return { id: String(x.id), slug: String(x.slug), name: String(x.name ?? x.slug), platform: str(x.platform), new_issues: num(x.new_issues) }
    }),
    new_issues: num(o.new_issues),
    crash_free_sessions: hasHealth ? ((h.crash_free_sessions as number | null) ?? null) : ((o.crash_free_sessions as number | null) ?? null),
    crash_free_users: hasHealth ? ((h.crash_free_users as number | null) ?? null) : ((o.crash_free_users as number | null) ?? null),
    adoption: hasHealth ? ((h.adoption as number | null) ?? null) : ((o.adoption as number | null) ?? null),
    sessions: hasHealth ? num(h.total_sessions) : num(o.sessions),
    last_deploy: (o.last_deploy as Release['last_deploy']) ?? null,
    stats: o.stats ? series(o.stats) : undefined,
  }
}

export function normReleaseDetail(raw: unknown): ReleaseDetail {
  const o = obj(raw)
  let health: ReleaseHealthPoint[] = []
  if (Array.isArray(o.health)) health = o.health as ReleaseHealthPoint[]
  else if (Array.isArray(o.sessions))
    // server: daily [[unix_seconds, sessions, crashed]]
    health = (o.sessions as unknown[][]).map((p) => ({ ts: num(p[0]), total: num(p[1]), crashed: num(p[2]), errored: 0, abnormal: 0 }))
  return {
    ...normRelease(raw),
    deploys: (o.deploys as ReleaseDetail['deploys']) ?? [],
    health,
    // `health.total_users` on current servers
    users: o.users !== undefined ? num(o.users) : obj(o.health).total_users !== undefined ? num(obj(o.health).total_users) : null,
    commit_count: o.commit_count === undefined ? undefined : num(o.commit_count),
  }
}

export function normCheckin(raw: unknown): Checkin {
  const o = obj(raw)
  return { ...(o as unknown as Checkin), duration_ms: o.duration_ms === null || o.duration_ms === undefined ? null : num(o.duration_ms) }
}

export function normMonitor(raw: unknown, project: ProjectRef): Monitor {
  const o = obj(raw)
  const checkins = (o.recent_checkins as unknown[]) ?? (o.checkins as unknown[]) ?? []
  return {
    ...(o as unknown as Monitor),
    project: (o.project as ProjectRef) ?? project,
    schedule: obj(o.schedule) as Monitor['schedule'],
    recent_checkins: checkins.map(normCheckin),
  }
}

function normAction(a: unknown): AlertAction {
  const o = obj(a)
  if (o.type === 'channel') return { type: 'channel', channel_id: String(o.channel_id ?? '') }
  if (o.type === 'create_ticket')
    return {
      type: 'create_ticket',
      integration_id: String(o.integration_id ?? ''),
      ...(o.target ? { target: String(o.target) } : {}),
      ...(o.issue_type ? { issue_type: String(o.issue_type) } : {}),
      ...(o.project ? { project: String(o.project) } : {}),
      ...(Array.isArray(o.labels) ? { labels: o.labels.map(String) } : {}),
    }
  return { type: 'email', user_ids: ((o.user_ids as unknown[]) ?? []).map(String), team_ids: ((o.team_ids as unknown[]) ?? []).map(String) }
}

function normFilter(f: unknown): AlertFilter {
  const o = obj(f)
  if (o.type === 'level') return { type: 'level', level: (o.level as Level) ?? 'error', match: o.match === 'eq' ? 'eq' : 'gte' }
  if (o.type === 'environment') return { type: 'environment', value: String(o.value ?? '') }
  return { type: 'tag', key: String(o.key ?? ''), value: String(o.value ?? '') }
}

export function normRule(raw: unknown, project: ProjectRef): AlertRule {
  const o = obj(raw)
  return {
    ...(o as unknown as AlertRule),
    project: (o.project as ProjectRef) ?? project,
    conditions: (o.conditions as AlertRule['conditions']) ?? [],
    filters: ((o.filters as unknown[]) ?? []).map(normFilter),
    actions: ((o.actions as unknown[]) ?? []).map(normAction),
    created_by: typeof o.created_by === 'string' ? o.created_by : (str(obj(o.created_by).name) ?? null),
    last_fired: str(o.last_fired),
  }
}
