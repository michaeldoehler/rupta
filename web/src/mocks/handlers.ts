// Mock implementation of the REST API v1 for `VITE_MOCK=1`, unit tests and
// end-to-end tests. Implements the contract with an in-memory world.

import { delay, http, HttpResponse, type JsonBodyType } from 'msw'
import type {
  Activity,
  AlertRule,
  Assignee,
  Feedback,
  Issue,
  IssueDetail,
  IssueUpdate,
  Monitor,
  Project,
  Role,
  Session,
  SsoConfig,
  TagDistribution,
  User,
  UserProfile,
} from '@/api/types'
import { parseQuery, parseRelative, type FilterToken } from '@/lib/search/parse'
import { Rng } from './rng'
import { buildEvent, eventIds, eventSummary } from './fixtures/events'
import { telemetryHandlers } from './telemetry-handlers'
import { createWorld, DAY, HOUR, iso, issueSeries, type DbIssue, type DbOrg, type DbProject, type World } from './fixtures/world'

const BASE = '/api/v1'

// ---------------------------------------------------------------------------
// State

export interface MockAuthState {
  setupDone: boolean
  userId: string | null
}

const STORE_KEY = 'rupta-mock-auth'

function loadAuth(): MockAuthState {
  try {
    const raw = globalThis.localStorage?.getItem(STORE_KEY)
    if (raw) return JSON.parse(raw) as MockAuthState
  } catch {
    /* ignore */
  }
  return { setupDone: true, userId: '1' }
}

let world: World = createWorld()
let auth: MockAuthState = loadAuth()
let latency = 0

function saveAuth() {
  try {
    globalThis.localStorage?.setItem(STORE_KEY, JSON.stringify(auth))
  } catch {
    /* ignore */
  }
}

/** Resets the mock world (tests, `?mock=fresh`, `?mock_scale=`). */
export function resetMock(opts: { auth?: Partial<MockAuthState>; scale?: number; latency?: number; now?: number } = {}) {
  world = createWorld({ scale: opts.scale, now: opts.now })
  auth = { setupDone: true, userId: '1', ...opts.auth }
  if (opts.latency !== undefined) latency = opts.latency
  saveAuth()
}

export function setMockLatency(ms: number) {
  latency = ms
}

export function getWorld() {
  return world
}

// ---------------------------------------------------------------------------
// Helpers

function err(status: number, code: string, message: string, details: Record<string, unknown> = {}) {
  return HttpResponse.json({ error: { code, message, details } }, { status })
}

const json = (body: JsonBodyType, status = 200) => HttpResponse.json(body, { status })

async function wait() {
  if (latency > 0) await delay(latency)
}

function currentUser() {
  return auth.userId ? world.users.find((u) => u.id === auth.userId) : undefined
}

function publicUser(u: World['users'][number]): User {
  return { id: u.id, email: u.email, name: u.name, avatar_url: u.avatar_url, is_superuser: u.is_superuser }
}

function nid() {
  world.nextId += 1
  return String(world.nextId)
}

function orgBySlug(slug: string) {
  return world.orgs.find((o) => o.slug === slug)
}

function roleIn(orgId: string): Role | null {
  const m = world.members.find((x) => x.org_id === orgId && x.user_id === auth.userId)
  return m?.role ?? null
}

function session(): Session | null {
  const u = currentUser()
  if (!u) return null
  return {
    user: publicUser(u),
    csrf_token: 'mock-csrf-token',
    orgs: world.members
      .filter((m) => m.user_id === u.id)
      .map((m) => {
        const o = world.orgs.find((x) => x.id === m.org_id)!
        return { id: o.id, slug: o.slug, name: o.name, role: m.role }
      }),
  }
}

function projectRef(p: DbProject) {
  return { id: p.id, slug: p.slug, name: p.name, platform: p.platform }
}

function projectOut(p: DbProject): Project {
  const team = world.teams.find((t) => t.id === p.team_id)
  const rng = new Rng(`pstats:${p.id}`)
  const now = world.now
  return {
    id: p.id,
    slug: p.slug,
    name: p.name,
    platform: p.platform,
    team: team ? { id: team.id, slug: team.slug, name: team.name } : null,
    created_at: p.created_at,
    settings: p.settings,
    stats: Array.from({ length: 24 }, (_, h) => [Math.floor((now - (23 - h) * HOUR) / 1000), rng.int(20, 400)] as [number, number]),
  }
}

function assigneeOut(a: DbIssue['assignee']): Assignee | null {
  if (!a) return null
  if (a.type === 'user') {
    const u = world.users.find((x) => x.id === a.id)
    return u ? { type: 'user', id: u.id, name: u.name, email: u.email } : null
  }
  const t = world.teams.find((x) => x.id === a.id)
  return t ? { type: 'team', id: t.id, name: `#${t.slug}` } : null
}

const PERIOD_MS: Record<string, number> = { '1h': HOUR, '24h': DAY, '7d': 7 * DAY, '14d': 14 * DAY, '30d': 30 * DAY, '90d': 90 * DAY }

/** Sparkline like the server: hourly up to 48 hours, daily beyond. */
function periodSeries(i: DbIssue, period: string) {
  const ms = PERIOD_MS[period] ?? DAY
  return ms < DAY
    ? issueSeries(i, world.now, 12, ms / 12)
    : ms <= 2 * DAY
      ? issueSeries(i, world.now, Math.round(ms / HOUR), HOUR)
      : issueSeries(i, world.now, Math.round(ms / DAY), DAY)
}

function issueOut(i: DbIssue, period = '24h'): Issue {
  const p = world.projects.find((x) => x.id === i.project_id)!
  const t = i.title
  const colon = t.indexOf(': ')
  return {
    id: i.id,
    short_id: i.short_id,
    title: t,
    culprit: i.culprit,
    level: i.level,
    status: i.status,
    substatus: i.substatus,
    type: i.type,
    platform: i.platform,
    project: projectRef(p),
    first_seen: iso(i.first_seen),
    last_seen: iso(i.last_seen),
    count: i.count,
    user_count: i.user_count,
    assignee: assigneeOut(i.assignee),
    metadata:
      i.type === 'error' && colon > 0
        ? { type: t.slice(0, colon), value: t.slice(colon + 2), filename: i.culprit.match(/\(([^)]+)\)/)?.[1], function: i.culprit.split('(')[0] }
        : { value: t },
    stats: periodSeries(i, period),
    priority: i.priority,
    is_bookmarked: auth.userId ? i.bookmarked_by.has(auth.userId) : false,
    has_seen: auth.userId ? i.seen_by.has(auth.userId) : false,
  }
}

function releaseRef(version: string | null) {
  if (!version) return null
  const r = world.releases.find((x) => x.version === version)
  return r ? { version, date: iso(r.date_created) } : { version, date: iso(world.now) }
}

function issueDetail(i: DbIssue): IssueDetail {
  const { stats, ...rest } = issueOut(i)
  void stats
  const s24 = issueSeries(i, world.now, 24, HOUR).reduce((a, [, c]) => a + c, 0)
  const s30 = issueSeries(i, world.now, 30, DAY).reduce((a, [, c]) => a + c, 0)
  void releaseRef
  return {
    ...rest,
    // the server sends release versions as plain strings
    first_release: i.first_release as never,
    last_release: i.last_release as never,
    resolved_in_release: i.resolved_in_release,
    ignore_until: i.ignore_until ? iso(i.ignore_until) : null,
    ignore_count: i.ignore_count,
    permalink: `/acme/issues/${i.id}`,
    counts: { '24h': s24, '30d': s30 },
  }
}

function page<T>(items: T[], url: URL, defLimit = 50) {
  const limit = Math.min(100, Number(url.searchParams.get('limit') ?? defLimit) || defLimit)
  const offset = Number(url.searchParams.get('cursor') ?? 0) || 0
  const data = items.slice(offset, offset + limit)
  return { data, next_cursor: offset + limit < items.length ? String(offset + limit) : null }
}

function listParam(url: URL, key: string): string[] {
  return url.searchParams
    .getAll(key)
    .flatMap((v) => v.split(','))
    .map((v) => v.trim())
    .filter(Boolean)
}

// ---------------------------------------------------------------------------
// Issue search (subset of the server's query language)

function matchFilter(i: DbIssue, f: FilterToken, me: string): boolean {
  const v = f.values.map((x) => x.toLowerCase())
  const any = (pred: (x: string) => boolean) => v.some(pred)
  const tag = (k: string) => (i.tags[k] ?? '').toLowerCase()
  const cmp = (n: number, raw: string) => {
    const x = Number(raw)
    switch (f.op) {
      case '>':
        return n > x
      case '>=':
        return n >= x
      case '<':
        return n < x
      case '<=':
        return n <= x
      default:
        return n === x
    }
  }
  let r: boolean
  switch (f.key) {
    case 'is':
      r = any((x) => {
        switch (x) {
          case 'unresolved':
          case 'resolved':
          case 'ignored':
            return i.status === x
          case 'archived':
            return i.status === 'ignored'
          case 'new':
          case 'ongoing':
          case 'regressed':
          case 'escalating':
            return i.status === 'unresolved' && i.substatus === x
          case 'assigned':
            return i.assignee !== null
          case 'unassigned':
            return i.assignee === null
          case 'bookmarked':
            return i.bookmarked_by.has(me)
          case 'unseen':
            return !i.seen_by.has(me)
          case 'for_review':
            return i.status === 'unresolved' && i.substatus === 'new'
          default:
            return false
        }
      })
      break
    case 'level':
      r = any((x) => i.level === x)
      break
    case 'priority':
      r = any((x) => i.priority === x)
      break
    case 'assigned':
      r = any((x) => {
        if (x === 'me') return i.assignee?.type === 'user' && i.assignee.id === me
        if (x === 'none') return i.assignee === null
        if (x === 'my_teams') return i.assignee?.type === 'team' && world.teams.some((t) => t.id === i.assignee!.id && t.member_ids.includes(me))
        if (x.startsWith('#')) return i.assignee?.type === 'team' && world.teams.find((t) => t.id === i.assignee!.id)?.slug === x.slice(1)
        return i.assignee?.type === 'user' && world.users.find((u) => u.id === i.assignee!.id)?.email === x
      })
      break
    case 'release':
      r = any((x) => (x === 'latest' ? true : (i.last_release ?? '').toLowerCase() === x || (i.last_release ?? '').toLowerCase().endsWith(`@${x}`)))
      break
    case 'first_release':
      r = any((x) => (i.first_release ?? '').toLowerCase() === x)
      break
    case 'platform':
      r = any((x) => i.platform === x)
      break
    case 'has':
      r = any((x) => (x === 'release' ? Boolean(i.last_release) : x === 'user' ? i.user_count > 0 : Boolean(i.tags[x])))
      break
    case 'first_seen':
    case 'last_seen': {
      const ms = parseRelative(f.values[0] ?? '')
      const at = f.key === 'first_seen' ? i.first_seen : i.last_seen
      r = ms === null ? true : f.values[0].startsWith('+') ? at < world.now - ms : at >= world.now - ms
      break
    }
    case 'times_seen':
      r = cmp(i.count, f.values[0])
      break
    case 'users_seen':
      r = cmp(i.user_count, f.values[0])
      break
    case 'error.value':
      r = any((x) => i.title.toLowerCase().includes(x.replace(/\*/g, '')))
      break
    default: {
      const key = f.key === 'browser.name' ? 'browser.name' : f.key
      r = any((x) =>
        x.includes('*')
          ? new RegExp(
              `^${x
                .split('*')
                .map((s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
                .join('.*')}$`,
            ).test(tag(key))
          : tag(key) === x || tag(key).startsWith(`${x} `),
      )
    }
  }
  return f.negated ? !r : r
}

function searchIssues(orgId: string, url: URL): DbIssue[] {
  const projects = listParam(url, 'project')
  const envs = listParam(url, 'environment')
  const q = url.searchParams.get('query') ?? ''
  const { filters, text } = parseQuery(q)
  const me = auth.userId ?? ''
  const orgProjects = new Set(world.projects.filter((p) => p.org_id === orgId).map((p) => p.id))
  let out = world.issues.filter((i) => orgProjects.has(i.project_id))
  if (projects.length) out = out.filter((i) => projects.includes(i.project_id))
  if (envs.length) out = out.filter((i) => envs.includes(i.tags.environment))
  const period = url.searchParams.get('period')
  if (period && PERIOD_MS[period]) out = out.filter((i) => i.last_seen >= world.now - PERIOD_MS[period])
  for (const f of filters) out = out.filter((i) => matchFilter(i, f, me))
  for (const t of text) {
    const needle = t.toLowerCase()
    out = out.filter((i) => i.title.toLowerCase().includes(needle) || i.culprit.toLowerCase().includes(needle) || i.short_id.toLowerCase() === needle)
  }
  const sort = url.searchParams.get('sort') ?? 'last_seen'
  const prio = { high: 3, medium: 2, low: 1 }
  const sorters: Record<string, (a: DbIssue, b: DbIssue) => number> = {
    last_seen: (a, b) => b.last_seen - a.last_seen,
    first_seen: (a, b) => b.first_seen - a.first_seen,
    freq: (a, b) => b.count - a.count,
    users: (a, b) => b.user_count - a.user_count,
    priority: (a, b) => prio[b.priority] - prio[a.priority] || b.last_seen - a.last_seen,
  }
  return [...out].sort(sorters[sort] ?? sorters.last_seen)
}

function applyIssueUpdate(i: DbIssue, body: IssueUpdate) {
  const me = auth.userId ?? ''
  const acts = world.activity.get(i.id) ?? []
  const actor = currentUser()
  const add = (kind: string, data: Record<string, unknown> = {}) =>
    acts.push({ id: nid(), kind, user: actor ? publicUser(actor) : null, data, created_at: iso(Date.now()) })
  if (body.status) {
    i.status = body.status
    if (body.status === 'resolved') {
      i.substatus = null
      // The server's body says `resolved_in: "next_release"`.
      const next = body.resolved_in_next_release || (body as { resolved_in?: string }).resolved_in === 'next_release'
      if (next) i.resolved_in_release = 'next'
      else if (body.resolved_in_release) i.resolved_in_release = body.resolved_in_release
      add(body.resolved_in_release || next ? 'set_resolved_in_release' : 'set_resolved', {
        version: body.resolved_in_release ?? null,
      })
    } else if (body.status === 'ignored') {
      i.substatus = body.ignore_duration || body.ignore_count ? 'archived_until_condition_met' : (body.substatus ?? 'archived_until_escalating')
      i.ignore_until = body.ignore_duration ? Date.now() + body.ignore_duration * 60_000 : null
      i.ignore_count = body.ignore_count ?? null
      add('set_ignored', { duration: body.ignore_duration, count: body.ignore_count })
    } else {
      i.substatus = 'ongoing'
      i.resolved_in_release = null
      add('set_unresolved')
    }
  }
  if (body.assignee !== undefined) {
    // server format: "user:{id}", "team:{id}", "me" or null
    const raw = body.assignee as unknown as string | null
    const [type, id] = raw === 'me' ? ['user', me] : raw ? raw.split(':') : [null, null]
    i.assignee = type ? { type: type === 'team' ? 'team' : 'user', id: id! } : null
    add(body.assignee ? 'assigned' : 'unassigned', { assignee: assigneeOut(i.assignee) })
  }
  if (body.priority) {
    i.priority = body.priority
    add('set_priority', { priority: body.priority })
  }
  if (body.is_bookmarked !== undefined) {
    if (body.is_bookmarked) i.bookmarked_by.add(me)
    else i.bookmarked_by.delete(me)
  }
  if (body.has_seen) i.seen_by.add(me)
  world.activity.set(i.id, acts)
}

function issueTags(i: DbIssue): TagDistribution[] {
  const rng = new Rng(`tags:${i.id}`)
  const total = i.count
  const dist = (key: string, name: string, values: string[]): TagDistribution => {
    const vs = [...new Set(values.filter(Boolean))]
    let remaining = total
    const top = vs.slice(0, 6).map((value, idx) => {
      const share = idx === vs.length - 1 ? remaining : Math.round(remaining * (idx === 0 ? 0.45 + rng.float() * 0.35 : 0.3 + rng.float() * 0.4))
      remaining -= share
      return { value, count: Math.max(0, share), first_seen: iso(i.first_seen), last_seen: iso(i.last_seen) }
    })
    return {
      key,
      name,
      total_values: total,
      unique_values: vs.length + (vs.length > 5 ? rng.int(0, 20) : 0),
      top_values: top.sort((a, b) => b.count - a.count),
    }
  }
  const js = i.platform === 'javascript'
  const out: TagDistribution[] = [
    dist('environment', 'Environment', [i.tags.environment, 'staging', 'development']),
    dist('release', 'Release', [i.last_release ?? '', i.first_release ?? '']),
    dist('handled', 'Handled', [i.handled ? 'yes' : 'no']),
    dist('level', 'Level', [i.level]),
  ]
  if (js) {
    out.push(dist('browser', 'Browser', [i.tags.browser, 'Chrome 131', 'Safari 18.1', 'Firefox 132', 'Edge 131', 'Mobile Safari 18.1']))
    out.push(dist('os', 'OS', [i.tags.os, 'Windows 11', 'macOS 15.1', 'iOS 18.1', 'Android 15']))
    out.push(dist('device', 'Device', [i.tags.device, 'Desktop', 'iPhone 15', 'Pixel 8']))
    out.push(dist('url', 'URL', ['https://app.acme.dev/catalog/shoes', 'https://app.acme.dev/catalog/bags', 'https://app.acme.dev/catalog/sale']))
  } else {
    out.push(dist('server_name', 'Server', [i.tags.server_name, 'web-1', 'web-2', 'worker-1']))
    out.push(dist('runtime', 'Runtime', [{ python: 'CPython 3.13.0', php: 'php 8.4.1', node: 'node v22.11.0', go: 'go go1.23.3' }[i.platform] ?? 'unknown']))
  }
  out.push(dist('transaction', 'Transaction', [i.tags.transaction]))
  out.push(dist('customer_tier', 'customer_tier', ['free', 'pro', 'business']))
  out.push(
    dist(
      'user',
      'User',
      Array.from({ length: 6 }, () => `id:${rng.int(1000, 99999)}`),
    ),
  )
  return out
}

function releaseOut(r: World['releases'][number], projectFilter: string[]) {
  const projects = r.project_ids.filter((pid) => !projectFilter.length || projectFilter.includes(pid)).map((pid) => world.projects.find((p) => p.id === pid)!)
  const last = [...r.deploys].sort((a, b) => b.date_finished - a.date_finished)[0]
  return {
    version: r.version,
    ref: r.ref,
    url: r.url,
    date_created: iso(r.date_created),
    date_released: r.date_released ? iso(r.date_released) : null,
    first_event: iso(r.date_created + HOUR),
    last_event: iso(Math.min(world.now, r.date_created + 10 * DAY)),
    short_version: r.version.slice(r.version.indexOf('@') + 1),
    projects: projects.map((p) => ({ id: p.id, slug: p.slug, name: p.name, new_issues: r.new_issues })),
    new_issues: r.new_issues,
    // server shape: health object
    health: {
      total_sessions: r.sessions,
      crashed_sessions: Math.round(r.sessions * (1 - r.crash_free_sessions / 100)),
      errored_sessions: Math.round(r.sessions * 0.01),
      abnormal_sessions: 0,
      crash_free_sessions: r.crash_free_sessions,
      crash_free_users: null,
      adoption: r.adoption,
    },
    last_deploy: last ? { ...last, date_started: last.date_started ? iso(last.date_started) : null, date_finished: iso(last.date_finished) } : null,
    stats: r.health.map((h) => [h.ts, h.total] as [number, number]),
  }
}

function monitorOut(m: World['monitors'][number]): Monitor {
  const { checkins, project_id, ...rest } = m
  const p = world.projects.find((x) => x.id === project_id)!
  void p
  return { ...rest, checkins: checkins.slice(0, 40) } as unknown as Monitor
}

function ruleOut(r: World['rules'][number]): AlertRule {
  const { project_id, ...rest } = r
  return { ...rest, project: projectRef(world.projects.find((p) => p.id === project_id)!) }
}

function feedbackOut(f: World['feedback'][number]): Feedback {
  const { project_id, issue_id, ...rest } = f
  const iss = issue_id ? world.issues.find((i) => i.id === issue_id) : undefined
  return {
    ...rest,
    project: projectRef(world.projects.find((p) => p.id === project_id)!),
    issue: iss ? { id: iss.id, short_id: iss.short_id, title: iss.title } : null,
  }
}

function audit(orgId: string, action: string, targetType: string, targetId: string, data: Record<string, unknown> = {}) {
  const u = currentUser()
  world.audit.unshift({
    id: nid(),
    org_id: orgId,
    actor: u ? { type: 'user', id: u.id, name: u.name } : { type: 'system', id: null, name: 'system' },
    action,
    target_type: targetType,
    target_id: targetId,
    data,
    ip: '127.0.0.1',
    created_at: iso(Date.now()),
  })
}

type Params = Record<string, string | readonly string[] | undefined>
const p = (params: Params, k: string) => String(params[k] ?? '')

type Failure = { error: Response }

function withOrg(params: Params): Failure | { org: DbOrg } {
  const org = orgBySlug(p(params, 'org'))
  if (!org) return { error: err(404, 'not_found', 'Organization not found.') }
  if (!roleIn(org.id)) return { error: err(403, 'forbidden', 'You are not a member of this organization.') }
  return { org }
}

function withProject(params: Params): Failure | { org: DbOrg; project: DbProject } {
  const r = withOrg(params)
  if ('error' in r) return r
  const project = world.projects.find((x) => x.org_id === r.org.id && x.slug === p(params, 'project'))
  if (!project) return { error: err(404, 'not_found', 'Project not found.') }
  return { org: r.org, project }
}

function findIssue(id: string) {
  return world.issues.find((i) => i.id === id)
}

/** Preferences in the server's shape (`clock`, `timezone: null` for the browser's zone). */
function serverPrefs(u: World['users'][number]) {
  const p = u.preferences
  const { deploys: _local, ...notifications } = p.notifications ?? {}
  return {
    timezone: !p.timezone || p.timezone === 'browser' ? null : p.timezone,
    clock: p.clock_24h === false ? '12h' : '24h',
    theme: p.theme ?? 'system',
    notifications,
  }
}

function userProfile(u: World['users'][number]): UserProfile {
  return {
    ...publicUser(u),
    preferences: serverPrefs(u) as unknown as UserProfile['preferences'],
    created_at: u.created_at,
    last_login_at: u.last_login_at,
    has_password: true,
    identities: u.id === '1' ? [{ provider: 'Zitadel', email: u.email }] : [],
  }
}

function requireAuth() {
  return currentUser() ? null : err(401, 'unauthorized', 'You are not signed in.')
}

// ---------------------------------------------------------------------------

export const handlers = [
  // ---- auth -----------------------------------------------------------------
  http.get(`${BASE}/auth/config`, async () => {
    await wait()
    return json({
      local_login: true,
      sso: [
        { id: 'zitadel', name: 'Zitadel' },
        { id: 'google', name: 'Google Workspace' },
      ],
      setup_required: !auth.setupDone,
    })
  }),
  http.get(`${BASE}/auth/session`, async () => {
    await wait()
    const s = session()
    return s ? json(s as unknown as JsonBodyType) : err(401, 'unauthorized', 'No session.')
  }),
  http.post(`${BASE}/auth/login`, async ({ request }) => {
    await wait()
    const body = (await request.json()) as { email: string; password: string }
    const u = world.users.find((x) => x.email.toLowerCase() === body.email?.toLowerCase())
    if (!u || body.password !== u.password) return err(401, 'invalid_credentials', 'Email or password is incorrect.')
    auth.userId = u.id
    saveAuth()
    return json(session() as unknown as JsonBodyType)
  }),
  http.post(`${BASE}/auth/logout`, async () => {
    auth.userId = null
    saveAuth()
    return new HttpResponse(null, { status: 204 })
  }),
  http.post(`${BASE}/auth/setup`, async ({ request }) => {
    await wait()
    if (auth.setupDone) return err(409, 'already_set_up', 'This instance is already set up.')
    const body = (await request.json()) as { email: string; name: string; password: string; org_name: string }
    const fields: Record<string, string> = {}
    if (!/^[^@\s]+@[^@\s]+$/.test(body.email ?? '')) fields.email = 'Enter a valid email address.'
    if ((body.password ?? '').length < 10) fields.password = 'Use at least 10 characters.'
    if (!body.org_name?.trim()) fields.org_name = 'Enter a name.'
    if (Object.keys(fields).length) return err(422, 'validation_failed', 'Some fields are invalid.', { fields })
    const u = world.users[0]
    u.email = body.email
    u.name = body.name || body.email
    u.password = body.password
    world.orgs[0].name = body.org_name
    auth = { setupDone: true, userId: u.id }
    saveAuth()
    return json(session() as unknown as JsonBodyType)
  }),

  http.get(`${BASE}/invites/:token`, async ({ params }) => {
    await wait()
    const inv = world.invites.find((i) => i.token === p(params, 'token'))
    if (!inv) return err(404, 'not_found', 'This invitation is invalid or has expired.')
    const org = world.orgs.find((o) => o.id === inv.org_id)!
    return json({ org_slug: org.slug, org_name: org.name, email: inv.email, role: inv.role, expires_at: inv.expires_at, user_exists: false })
  }),
  http.post(`${BASE}/invites/:token/accept`, async ({ params, request }) => {
    await wait()
    const inv = world.invites.find((i) => i.token === p(params, 'token'))
    if (!inv) return err(404, 'not_found', 'This invitation is invalid or has expired.')
    const body = (await request.json()) as { name?: string; password?: string }
    if ((body.password ?? '').length < 10)
      return err(422, 'validation_failed', 'Some fields are invalid.', { fields: { password: 'Use at least 10 characters.' } })
    const uid = nid()
    world.users.push({
      id: uid,
      email: inv.email,
      name: body.name || inv.email,
      avatar_url: null,
      is_superuser: false,
      password: body.password!,
      preferences: {},
      created_at: iso(Date.now()),
      last_login_at: iso(Date.now()),
    })
    world.members.push({ org_id: inv.org_id, user_id: uid, role: inv.role, created_at: iso(Date.now()), sso: false })
    world.invites = world.invites.filter((i) => i !== inv)
    auth.userId = uid
    saveAuth()
    return json(session() as unknown as JsonBodyType)
  }),

  // Everything below requires a session.
  http.all(`${BASE}/*`, async ({ request }) => {
    const path = new URL(request.url).pathname
    if (path.startsWith(`${BASE}/auth/`) || path.startsWith(`${BASE}/invites/`)) return undefined
    return requireAuth() ?? undefined
  }),

  // ---- user -----------------------------------------------------------------
  http.get(`${BASE}/user`, async () => {
    await wait()
    return json(userProfile(currentUser()!) as unknown as JsonBodyType)
  }),
  http.patch(`${BASE}/user`, async ({ request }) => {
    const u = currentUser()!
    const body = (await request.json()) as {
      name?: string
      avatar_url?: string | null
      current_password?: string
      new_password?: string
      preferences?: { timezone?: string | null; clock?: '24h' | '12h'; theme?: 'system' | 'light' | 'dark'; notifications?: Record<string, unknown> }
    }
    if (body.preferences) {
      const p = body.preferences
      u.preferences = {
        ...u.preferences,
        ...(p.timezone !== undefined ? { timezone: p.timezone ?? 'browser' } : {}),
        ...(p.clock ? { clock_24h: p.clock === '24h' } : {}),
        ...(p.theme ? { theme: p.theme } : {}),
        notifications: { ...u.preferences.notifications, ...(p.notifications as object) },
      }
    }
    if (body.new_password !== undefined) {
      if (body.current_password !== u.password) return err(403, 'forbidden', 'The current password is incorrect.', { field: 'current_password' })
      if (body.new_password.length < 10) return err(422, 'validation_failed', 'new_password: use at least 10 characters', { field: 'new_password' })
      u.password = body.new_password
    }
    if (body.name !== undefined) u.name = body.name
    return json(userProfile(u) as unknown as JsonBodyType)
  }),
  http.get(`${BASE}/scopes`, () =>
    json(
      [
        'org:read',
        'org:write',
        'org:admin',
        'member:read',
        'member:write',
        'project:read',
        'project:write',
        'project:admin',
        'project:releases',
        'issue:write',
        'release:write',
      ].map((scope) => ({ scope, description: scope })),
    ),
  ),
  http.get(`${BASE}/user/tokens`, () => json(world.userTokens.filter((t) => t.user_id === auth.userId) as unknown as JsonBodyType)),
  http.post(`${BASE}/user/tokens`, async ({ request }) => {
    const body = (await request.json()) as { name: string; scopes: string[]; expires_in_days?: number | null }
    const token = `rpt_${new Rng(Date.now()).hex(40)}`
    const t = {
      id: nid(),
      user_id: auth.userId!,
      name: body.name,
      token_hint: `rpt_…${token.slice(-4)}`,
      scopes: body.scopes as never,
      created_at: iso(Date.now()),
      last_used_at: null,
      expires_at: body.expires_in_days ? iso(Date.now() + body.expires_in_days * DAY) : null,
    }
    world.userTokens.push(t)
    return json({ ...t, token }, 201)
  }),
  http.delete(`${BASE}/user/tokens/:id`, ({ params }) => {
    world.userTokens = world.userTokens.filter((t) => t.id !== p(params, 'id'))
    return new HttpResponse(null, { status: 204 })
  }),

  // ---- orgs -----------------------------------------------------------------
  http.get(`${BASE}/orgs`, () => json(world.orgs.filter((o) => roleIn(o.id)) as unknown as JsonBodyType)),
  http.post(`${BASE}/orgs`, async ({ request }) => {
    const body = (await request.json()) as { name: string; slug?: string }
    const slug = (body.slug || body.name)
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '')
    if (world.orgs.some((o) => o.slug === slug)) return err(409, 'conflict', 'An organization with this slug exists.', { fields: { slug: 'Already taken.' } })
    const o = { id: nid(), slug, name: body.name, created_at: iso(Date.now()), settings: {} }
    world.orgs.push(o)
    world.members.push({ org_id: o.id, user_id: auth.userId!, role: 'owner', created_at: o.created_at, sso: false })
    return json(o, 201)
  }),
  http.get(`${BASE}/orgs/:org`, ({ params }) => {
    const r = withOrg(params)
    return 'error' in r ? r.error : json(r.org as unknown as JsonBodyType)
  }),
  http.patch(`${BASE}/orgs/:org`, async ({ params, request }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const body = (await request.json()) as { name?: string; slug?: string; settings?: Record<string, unknown> }
    if (body.name) r.org.name = body.name
    if (body.slug) r.org.slug = body.slug
    if (body.settings) r.org.settings = { ...r.org.settings, ...body.settings }
    audit(r.org.id, 'org.update', 'organization', r.org.id, { fields: Object.keys(body) })
    return json(r.org as unknown as JsonBodyType)
  }),

  http.get(`${BASE}/orgs/:org/members`, async ({ params }) => {
    await wait()
    const r = withOrg(params)
    if ('error' in r) return r.error
    // server shape: Page_Member with flat members
    const out = world.members
      .filter((m) => m.org_id === r.org.id)
      .map((m) => {
        const u = world.users.find((x) => x.id === m.user_id)!
        return {
          user_id: u.id,
          email: u.email,
          name: u.name,
          avatar_url: u.avatar_url,
          role: m.role,
          created_at: m.created_at,
          sso_linked: m.sso,
          teams: world.teams.filter((t) => t.org_id === r.org.id && t.member_ids.includes(m.user_id)).map((t) => t.slug),
        }
      })
    return json({ data: out, next_cursor: null } as unknown as JsonBodyType)
  }),
  http.post(`${BASE}/orgs/:org/members`, async ({ params, request }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const body = (await request.json()) as { email: string; role: Role }
    if (!/^[^@\s]+@[^@\s]+$/.test(body.email ?? ''))
      return err(422, 'validation_failed', 'Enter a valid email address.', { fields: { email: 'Enter a valid email address.' } })
    if (world.invites.some((i) => i.org_id === r.org.id && i.email === body.email))
      return err(409, 'conflict', 'This address already has a pending invitation.')
    const inv = {
      id: nid(),
      org_id: r.org.id,
      email: body.email,
      role: body.role,
      created_at: iso(Date.now()),
      expires_at: iso(Date.now() + 7 * DAY),
      token: `t${nid()}`,
    }
    world.invites.push(inv)
    audit(r.org.id, 'member.invite', 'member', inv.id, { email: body.email, role: body.role })
    // without SMTP the server answers with a link to share
    return json(
      {
        id: inv.id,
        email: inv.email,
        role: inv.role,
        created_at: inv.created_at,
        expires_at: inv.expires_at,
        email_sent: false,
        accept_url: `${location.origin}/accept-invite/${inv.token}`,
      },
      201,
    )
  }),
  http.patch(`${BASE}/orgs/:org/members/:user`, async ({ params, request }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const m = world.members.find((x) => x.org_id === r.org.id && x.user_id === p(params, 'user'))
    if (!m) return err(404, 'not_found', 'Member not found.')
    const body = (await request.json()) as { role: Role }
    if (m.role === 'owner' && body.role !== 'owner' && world.members.filter((x) => x.org_id === r.org.id && x.role === 'owner').length === 1)
      return err(409, 'last_owner', 'An organization needs at least one owner.')
    audit(r.org.id, 'member.role_change', 'member', m.user_id, { from: m.role, to: body.role })
    m.role = body.role
    return json({ ok: true })
  }),
  http.delete(`${BASE}/orgs/:org/members/:user`, ({ params }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    world.members = world.members.filter((x) => !(x.org_id === r.org.id && x.user_id === p(params, 'user')))
    audit(r.org.id, 'member.remove', 'member', p(params, 'user'))
    return new HttpResponse(null, { status: 204 })
  }),
  http.get(`${BASE}/orgs/:org/invites`, ({ params }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    return json(
      world.invites
        .filter((i) => i.org_id === r.org.id)
        .map(({ token, org_id, ...rest }) => (void org_id, { ...rest, accept_url: `${location.origin}/accept-invite/${token}` })) as unknown as JsonBodyType,
    )
  }),
  http.delete(`${BASE}/orgs/:org/invites/:id`, ({ params }) => {
    world.invites = world.invites.filter((i) => i.id !== p(params, 'id'))
    return new HttpResponse(null, { status: 204 })
  }),

  http.get(`${BASE}/orgs/:org/teams`, async ({ params }) => {
    await wait()
    const r = withOrg(params)
    if ('error' in r) return r.error
    const out = world.teams
      .filter((t) => t.org_id === r.org.id)
      .map((t) => ({
        id: t.id,
        slug: t.slug,
        name: t.name,
        created_at: t.created_at,
        members: t.member_ids.map((id) => {
          const u = world.users.find((x) => x.id === id)!
          return { user_id: u.id, email: u.email, name: u.name }
        }),
      }))
    return json(out as unknown as JsonBodyType)
  }),
  http.post(`${BASE}/orgs/:org/teams`, async ({ params, request }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const body = (await request.json()) as { name: string; slug?: string }
    const slug = (body.slug || body.name).toLowerCase().replace(/[^a-z0-9]+/g, '-')
    if (world.teams.some((t) => t.org_id === r.org.id && t.slug === slug))
      return err(409, 'conflict', 'A team with this slug exists.', { fields: { name: 'Already taken.' } })
    const t = { id: nid(), org_id: r.org.id, slug, name: body.name, created_at: iso(Date.now()), member_ids: [] as string[] }
    world.teams.push(t)
    audit(r.org.id, 'team.create', 'team', t.id, { slug })
    return json({ ...t, members: [], projects: [] }, 201)
  }),
  http.delete(`${BASE}/orgs/:org/teams/:team`, ({ params }) => {
    world.teams = world.teams.filter((t) => t.slug !== p(params, 'team'))
    return new HttpResponse(null, { status: 204 })
  }),
  http.put(`${BASE}/orgs/:org/teams/:team/members/:user`, ({ params }) => {
    const t = world.teams.find((x) => x.slug === p(params, 'team'))
    if (t && !t.member_ids.includes(p(params, 'user'))) t.member_ids.push(p(params, 'user'))
    return new HttpResponse(null, { status: 204 })
  }),
  http.delete(`${BASE}/orgs/:org/teams/:team/members/:user`, ({ params }) => {
    const t = world.teams.find((x) => x.slug === p(params, 'team'))
    if (t) t.member_ids = t.member_ids.filter((id) => id !== p(params, 'user'))
    return new HttpResponse(null, { status: 204 })
  }),

  http.get(`${BASE}/orgs/:org/audit-log`, async ({ params, request }) => {
    await wait()
    const r = withOrg(params)
    if ('error' in r) return r.error
    const url = new URL(request.url)
    const action = url.searchParams.get('action')
    let items = world.audit.filter((a) => a.org_id === r.org.id)
    if (action) items = items.filter((a) => a.action.startsWith(action))
    // server shape: actor {user_id, email, token_id}
    const out = items.map(({ org_id, actor, ...rest }) => {
      void org_id
      const u = actor.type === 'user' ? world.users.find((x) => x.id === actor.id) : undefined
      return {
        ...rest,
        actor: { user_id: actor.type === 'user' ? actor.id : null, email: u?.email ?? null, token_id: actor.type === 'token' ? actor.id : null },
      }
    })
    return json(page(out, url) as unknown as JsonBodyType)
  }),

  http.get(`${BASE}/orgs/:org/sso`, ({ params }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    return json((world.sso.get(r.org.id) ?? null) as unknown as JsonBodyType)
  }),
  http.put(`${BASE}/orgs/:org/sso`, async ({ params, request }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const body = (await request.json()) as Partial<SsoConfig>
    const fields: Record<string, string> = {}
    if (!body.issuer?.startsWith('https://')) fields.issuer = 'Use the https:// URL of the issuer.'
    if (!body.client_id) fields.client_id = 'Required.'
    const prev = world.sso.get(r.org.id)
    if (!body.client_secret && !prev?.has_client_secret) fields.client_secret = 'Required.'
    if (Object.keys(fields).length) return err(422, 'validation_failed', 'Some fields are invalid.', { fields })
    const { client_secret, ...rest } = body
    const cfg: SsoConfig = {
      enabled: true,
      provider_name: 'SSO',
      scopes: 'openid email profile',
      role_claim: null,
      role_mapping: {},
      default_role: 'member',
      jit_provisioning: true,
      enforce: false,
      issuer: '',
      client_id: '',
      ...prev,
      ...rest,
      has_client_secret: Boolean(client_secret) || Boolean(prev?.has_client_secret),
      owner_local_login: body.owner_local_login ?? prev?.owner_local_login ?? true,
      provider_id: `org-${r.org.slug}`,
      redirect_uri: `${location.origin}/auth/sso/org-${r.org.slug}/callback`,
    }
    world.sso.set(r.org.id, cfg)
    audit(r.org.id, 'sso.update', 'sso_config', r.org.id, { provider: cfg.provider_name })
    return json(cfg as unknown as JsonBodyType)
  }),
  http.delete(`${BASE}/orgs/:org/sso`, ({ params }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    world.sso.delete(r.org.id)
    return new HttpResponse(null, { status: 204 })
  }),
  // beyond the API (the page falls back to a browser check when this is missing)
  http.post(`${BASE}/orgs/:org/sso/test`, async ({ request }) => {
    await delay(600)
    const body = (await request.json()) as Partial<SsoConfig>
    if (!body.issuer?.startsWith('https://')) return json({ ok: false, message: 'The issuer URL must start with https://.' })
    if (body.issuer.includes('invalid')) return json({ ok: false, message: `Could not fetch ${body.issuer}/.well-known/openid-configuration (HTTP 404).` })
    return json({
      ok: true,
      message: 'Discovery document loaded. Endpoints and signing keys are reachable.',
      issuer: body.issuer,
      authorization_endpoint: `${body.issuer.replace(/\/$/, '')}/oauth/v2/authorize`,
    })
  }),

  http.get(`${BASE}/orgs/:org/tokens`, ({ params }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    return json(world.orgTokens.filter((t) => t.org_id === r.org.id) as unknown as JsonBodyType)
  }),
  http.post(`${BASE}/orgs/:org/tokens`, async ({ params, request }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const body = (await request.json()) as { name: string; scopes: string[]; expires_in_days?: number | null }
    if (!body.name?.trim()) return err(422, 'validation_failed', 'Enter a name.', { fields: { name: 'Enter a name.' } })
    if (!body.scopes?.length) return err(422, 'validation_failed', 'Choose at least one scope.', { fields: { scopes: 'Choose at least one scope.' } })
    const token = `rpt_${new Rng(Date.now()).hex(40)}`
    const t = {
      id: nid(),
      org_id: r.org.id,
      name: body.name,
      token_hint: `rpt_…${token.slice(-4)}`,
      scopes: body.scopes as never,
      created_at: iso(Date.now()),
      last_used_at: null,
      expires_at: body.expires_in_days ? iso(Date.now() + body.expires_in_days * DAY) : null,
    }
    world.orgTokens.push(t)
    audit(r.org.id, 'api_token.create', 'api_token', t.id, { name: t.name, scopes: t.scopes })
    return json({ ...t, token }, 201)
  }),
  http.delete(`${BASE}/orgs/:org/tokens/:id`, ({ params }) => {
    world.orgTokens = world.orgTokens.filter((t) => t.id !== p(params, 'id'))
    return new HttpResponse(null, { status: 204 })
  }),

  http.get(`${BASE}/orgs/:org/channels`, async ({ params }) => {
    await wait()
    const r = withOrg(params)
    if ('error' in r) return r.error
    return json(world.channels.filter((c) => c.org_id === r.org.id) as unknown as JsonBodyType)
  }),
  http.post(`${BASE}/orgs/:org/channels`, async ({ params, request }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const body = (await request.json()) as { kind: 'email'; name: string; config: Record<string, string> }
    const c = { id: nid(), org_id: r.org.id, created_at: iso(Date.now()), ...body }
    world.channels.push(c)
    return json(c, 201)
  }),
  http.patch(`${BASE}/orgs/:org/channels/:id`, async ({ params, request }) => {
    const c = world.channels.find((x) => x.id === p(params, 'id'))
    if (!c) return err(404, 'not_found', 'Channel not found.')
    Object.assign(c, await request.json())
    return json(c as unknown as JsonBodyType)
  }),
  http.delete(`${BASE}/orgs/:org/channels/:id`, ({ params }) => {
    world.channels = world.channels.filter((c) => c.id !== p(params, 'id'))
    return new HttpResponse(null, { status: 204 })
  }),
  http.post(`${BASE}/orgs/:org/channels/:id/test`, async ({ params }) => {
    await delay(400)
    const c = world.channels.find((x) => x.id === p(params, 'id'))
    if (!c) return err(404, 'not_found', 'Channel not found.')
    return c.kind === 'teams'
      ? json({ ok: false, message: 'The webhook answered HTTP 410 Gone. The connector may have been removed.' })
      : json({ ok: true, message: `Test notification sent to ${c.name}.` })
  }),

  // Event query: only the group_by form, which the UI uses for value suggestions.
  http.get(`${BASE}/orgs/:org/events`, ({ params, request }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const url = new URL(request.url)
    const col = url.searchParams.get('group_by')
    if (!col) return json({ data: [] })
    const key = { user_email: 'user.email', user_id: 'user.id', exception_type: 'error.type' }[col] ?? col
    const projects = listParam(url, 'project')
    const orgProjects = new Set(world.projects.filter((x) => x.org_id === r.org.id && (!projects.length || projects.includes(x.id))).map((x) => x.id))
    const counts = new Map<string, number>()
    for (const i of world.issues) {
      if (!orgProjects.has(i.project_id)) continue
      const v = key === 'release' ? i.last_release : key === 'platform' ? i.platform : key === 'level' ? i.level : i.tags[key]
      if (v) counts.set(v, (counts.get(v) ?? 0) + i.count)
      if (key === 'environment') for (const e of ['staging', 'development']) counts.set(e, (counts.get(e) ?? 0) + 1)
    }
    const data = [...counts.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, Number(url.searchParams.get('limit') ?? 50))
      .map(([value, count]) => ({ [col]: value, count }))
    return json({ data } as unknown as JsonBodyType)
  }),

  http.get(`${BASE}/orgs/:org/saved-searches`, ({ params }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    return json(world.savedSearches.filter((s) => s.org_id === r.org.id && (s.user_id === null || s.user_id === auth.userId)) as unknown as JsonBodyType)
  }),
  http.post(`${BASE}/orgs/:org/saved-searches`, async ({ params, request }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const body = (await request.json()) as { name: string; query: string; sort: 'last_seen'; shared: boolean }
    const s = {
      id: nid(),
      org_id: r.org.id,
      user_id: body.shared ? null : auth.userId,
      created_at: iso(Date.now()),
      name: body.name,
      query: body.query,
      sort: body.sort,
      is_shared: body.shared,
      shared: body.shared,
    }
    world.savedSearches.push(s)
    return json(s, 201)
  }),
  http.delete(`${BASE}/orgs/:org/saved-searches/:id`, ({ params }) => {
    world.savedSearches = world.savedSearches.filter((s) => s.id !== p(params, 'id'))
    return new HttpResponse(null, { status: 204 })
  }),

  // ---- projects -------------------------------------------------------------
  http.get(`${BASE}/orgs/:org/projects`, async ({ params }) => {
    await wait()
    const r = withOrg(params)
    if ('error' in r) return r.error
    return json(world.projects.filter((x) => x.org_id === r.org.id).map(projectOut) as unknown as JsonBodyType)
  }),
  http.post(`${BASE}/orgs/:org/projects`, async ({ params, request }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const body = (await request.json()) as { name: string; slug?: string; platform?: string; team?: string | null }
    const slug = (body.slug || body.name).toLowerCase().replace(/[^a-z0-9]+/g, '-')
    if (world.projects.some((x) => x.org_id === r.org.id && x.slug === slug))
      return err(409, 'conflict', 'A project with this slug exists.', { fields: { name: 'Already taken.' } })
    const proj: DbProject = {
      id: nid(),
      org_id: r.org.id,
      slug,
      name: body.name,
      platform: body.platform ?? null,
      team_id: body.team ?? null,
      created_at: iso(Date.now()),
      prefix: slug.toUpperCase().slice(0, 5),
      settings: {
        allowed_origins: ['*'],
        rate_limit_per_min: null,
        retention_days: null,
        filters: { browser_extensions: false, localhost: false, legacy_browsers: false, web_crawlers: false, error_messages: [], releases: [] },
        scrubbing: { enabled: true, extra_fields: [], safe_fields: [], scrub_ip_addresses: false },
        in_app_include: [],
        in_app_exclude: [],
      },
    }
    world.projects.push(proj)
    const pub = new Rng(Date.now()).hex(32)
    world.keys.push({
      id: nid(),
      project_id: proj.id,
      label: 'Default',
      public_key: pub,
      secret_key: new Rng(pub).hex(32),
      is_active: true,
      rate_limit_per_min: null,
      created_at: proj.created_at,
      dsn: `${location.origin.replace('://', `://${pub}@`)}/${proj.id}`,
    })
    audit(r.org.id, 'project.create', 'project', proj.id, { slug })
    return json(projectOut(proj) as unknown as JsonBodyType, 201)
  }),
  http.get(`${BASE}/projects/:org/:project`, ({ params }) => {
    const r = withProject(params)
    return 'error' in r ? r.error : json(projectOut(r.project) as unknown as JsonBodyType)
  }),
  http.patch(`${BASE}/projects/:org/:project`, async ({ params, request }) => {
    const r = withProject(params)
    if ('error' in r) return r.error
    const body = (await request.json()) as Partial<DbProject> & { settings?: Partial<DbProject['settings']> }
    if (body.name !== undefined) r.project.name = body.name
    if (body.slug !== undefined) r.project.slug = body.slug
    if (body.platform !== undefined) r.project.platform = body.platform
    if (body.team_id !== undefined) r.project.team_id = body.team_id
    if (body.settings) r.project.settings = { ...r.project.settings, ...body.settings } as DbProject['settings']
    audit(r.org.id, 'project.update', 'project', r.project.id, { fields: Object.keys(body) })
    return json(projectOut(r.project) as unknown as JsonBodyType)
  }),
  http.delete(`${BASE}/projects/:org/:project`, ({ params }) => {
    const r = withProject(params)
    if ('error' in r) return r.error
    world.projects = world.projects.filter((x) => x !== r.project)
    return new HttpResponse(null, { status: 204 })
  }),
  http.get(`${BASE}/projects/:org/:project/keys`, async ({ params }) => {
    await wait()
    const r = withProject(params)
    if ('error' in r) return r.error
    return json(world.keys.filter((k) => k.project_id === r.project.id).map(({ project_id, ...k }) => (void project_id, k)) as unknown as JsonBodyType)
  }),
  http.post(`${BASE}/projects/:org/:project/keys`, async ({ params, request }) => {
    const r = withProject(params)
    if ('error' in r) return r.error
    const body = (await request.json()) as { label: string }
    const pub = new Rng(Date.now()).hex(32)
    const k = {
      id: nid(),
      project_id: r.project.id,
      label: body.label || 'Key',
      public_key: pub,
      secret_key: new Rng(pub).hex(32),
      is_active: true,
      rate_limit_per_min: null,
      created_at: iso(Date.now()),
      dsn: `https://${pub}@rupta.acme.dev/${r.project.id}`,
    }
    world.keys.push(k)
    return json(k, 201)
  }),
  http.patch(`${BASE}/projects/:org/:project/keys/:id`, async ({ params, request }) => {
    const k = world.keys.find((x) => x.id === p(params, 'id'))
    if (!k) return err(404, 'not_found', 'Key not found.')
    Object.assign(k, await request.json())
    return json(k as unknown as JsonBodyType)
  }),
  http.delete(`${BASE}/projects/:org/:project/keys/:id`, ({ params }) => {
    world.keys = world.keys.filter((k) => k.id !== p(params, 'id'))
    return new HttpResponse(null, { status: 204 })
  }),
  http.get(`${BASE}/projects/:org/:project/stats`, ({ params }) => {
    const r = withProject(params)
    if ('error' in r) return r.error
    const rng = new Rng(`stats:${r.project.id}`)
    return json(Array.from({ length: 24 }, (_, h) => [Math.floor((world.now - (23 - h) * HOUR) / 1000), rng.int(50, 900)]))
  }),
  http.get(`${BASE}/projects/:org/:project/artifact-bundles`, async ({ params }) => {
    await wait()
    const r = withProject(params)
    if ('error' in r) return r.error
    return json(world.bundles.filter((b) => b.project_id === r.project.id).map(({ project_id, ...b }) => (void project_id, b)) as unknown as JsonBodyType)
  }),
  http.delete(`${BASE}/projects/:org/:project/artifact-bundles/:id`, ({ params }) => {
    world.bundles = world.bundles.filter((b) => b.id !== p(params, 'id'))
    return new HttpResponse(null, { status: 204 })
  }),

  http.get(`${BASE}/projects/:org/:project/alert-rules`, async ({ params }) => {
    await wait()
    const r = withProject(params)
    if ('error' in r) return r.error
    return json(world.rules.filter((x) => x.project_id === r.project.id).map(ruleOut) as unknown as JsonBodyType)
  }),
  http.get(`${BASE}/projects/:org/:project/alert-rules/:id`, ({ params }) => {
    const rule = world.rules.find((x) => x.id === p(params, 'id'))
    return rule ? json(ruleOut(rule) as unknown as JsonBodyType) : err(404, 'not_found', 'Alert rule not found.')
  }),
  http.post(`${BASE}/projects/:org/:project/alert-rules`, async ({ params, request }) => {
    const r = withProject(params)
    if ('error' in r) return r.error
    const body = (await request.json()) as Omit<AlertRule, 'id' | 'project'>
    if (!body.name?.trim()) return err(422, 'validation_failed', 'Enter a name.', { fields: { name: 'Enter a name.' } })
    if (!body.conditions?.length) return err(422, 'validation_failed', 'Add at least one condition.', { fields: { conditions: 'Add at least one condition.' } })
    if (!body.actions?.length) return err(422, 'validation_failed', 'Add at least one action.', { fields: { actions: 'Add at least one action.' } })
    const u = currentUser()!
    const rule = {
      ...body,
      id: nid(),
      project_id: r.project.id,
      created_by: u.name,
      created_at: iso(Date.now()),
      updated_at: iso(Date.now()),
      last_fired: null,
    }
    world.rules.push(rule)
    audit(r.org.id, 'alert_rule.create', 'alert_rule', rule.id, { name: rule.name })
    return json(ruleOut(rule) as unknown as JsonBodyType, 201)
  }),
  http.patch(`${BASE}/projects/:org/:project/alert-rules/:id`, async ({ params, request }) => {
    const rule = world.rules.find((x) => x.id === p(params, 'id'))
    if (!rule) return err(404, 'not_found', 'Alert rule not found.')
    Object.assign(rule, await request.json(), { updated_at: iso(Date.now()) })
    return json(ruleOut(rule) as unknown as JsonBodyType)
  }),
  http.delete(`${BASE}/projects/:org/:project/alert-rules/:id`, ({ params }) => {
    world.rules = world.rules.filter((x) => x.id !== p(params, 'id'))
    return new HttpResponse(null, { status: 204 })
  }),

  http.get(`${BASE}/projects/:org/:project/feedback`, async ({ params, request }) => {
    await wait()
    const r = withProject(params)
    if ('error' in r) return r.error
    const url = new URL(request.url)
    const q = (url.searchParams.get('query') ?? '').toLowerCase()
    const items = world.feedback
      .filter((f) => f.project_id === r.project.id && (!q || f.message.toLowerCase().includes(q) || (f.email ?? '').includes(q)))
      .map(feedbackOut)
    return json(page(items, url) as unknown as JsonBodyType)
  }),

  http.get(`${BASE}/projects/:org/:project/monitors`, async ({ params }) => {
    await wait()
    const r = withProject(params)
    if ('error' in r) return r.error
    return json(world.monitors.filter((m) => m.project_id === r.project.id).map(monitorOut) as unknown as JsonBodyType)
  }),
  http.get(`${BASE}/projects/:org/:project/monitors/:slug`, async ({ params }) => {
    await wait()
    const r = withProject(params)
    if ('error' in r) return r.error
    const m = world.monitors.find((x) => x.project_id === r.project.id && x.slug === p(params, 'slug'))
    return m ? json(monitorOut(m) as unknown as JsonBodyType) : err(404, 'not_found', 'Monitor not found.')
  }),
  http.get(`${BASE}/projects/:org/:project/monitors/:slug/checkins`, ({ params, request }) => {
    const m = world.monitors.find((x) => x.slug === p(params, 'slug'))
    if (!m) return err(404, 'not_found', 'Monitor not found.')
    return json(page(m.checkins, new URL(request.url)) as unknown as JsonBodyType)
  }),
  http.patch(`${BASE}/projects/:org/:project/monitors/:slug`, async ({ params, request }) => {
    const m = world.monitors.find((x) => x.slug === p(params, 'slug'))
    if (!m) return err(404, 'not_found', 'Monitor not found.')
    Object.assign(m, await request.json())
    return json(monitorOut(m) as unknown as JsonBodyType)
  }),

  // ---- issues ---------------------------------------------------------------
  http.get(`${BASE}/orgs/:org/issues`, async ({ params, request }) => {
    await wait()
    const r = withOrg(params)
    if ('error' in r) return r.error
    const url = new URL(request.url)
    const period = url.searchParams.get('period') ?? '24h'
    const found = searchIssues(r.org.id, url)
    const pg = page(found, url)
    return json({ data: pg.data.map((i) => issueOut(i, period)), next_cursor: pg.next_cursor, total: found.length } as unknown as JsonBodyType)
  }),
  http.put(`${BASE}/orgs/:org/issues`, async ({ params, request }) => {
    await wait()
    const r = withOrg(params)
    if ('error' in r) return r.error
    if (roleIn(r.org.id) === 'viewer') return err(403, 'forbidden', 'Viewers cannot change issues.')
    const ids = new URL(request.url).searchParams.getAll('id')
    const body = (await request.json()) as IssueUpdate
    let n = 0
    for (const id of ids) {
      const i = findIssue(id)
      if (i) {
        applyIssueUpdate(i, body)
        n += 1
      }
    }
    return json({ updated: n })
  }),
  http.delete(`${BASE}/orgs/:org/issues`, async ({ params, request }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const ids = new Set(new URL(request.url).searchParams.getAll('id'))
    world.issues = world.issues.filter((i) => !ids.has(i.id))
    audit(r.org.id, 'issue.delete', 'issue', [...ids].join(','), { count: ids.size })
    return new HttpResponse(null, { status: 204 })
  }),
  http.get(`${BASE}/orgs/:org/issues/by-short-id/:sid`, ({ params }) => {
    const r = withOrg(params)
    if ('error' in r) return r.error
    const sid = p(params, 'sid').toUpperCase()
    const i = world.issues.find((x) => x.short_id === sid && world.projects.find((pp) => pp.id === x.project_id)?.org_id === r.org.id)
    return i ? json(issueOut(i) as unknown as JsonBodyType) : err(404, 'not_found', `No issue ${sid}.`)
  }),
  http.get(`${BASE}/issues/:id`, async ({ params }) => {
    await wait()
    const i = findIssue(p(params, 'id'))
    if (!i) return err(404, 'not_found', 'This issue does not exist or was deleted.')
    return json(issueDetail(i) as unknown as JsonBodyType)
  }),
  http.patch(`${BASE}/issues/:id`, async ({ params, request }) => {
    await wait()
    const i = findIssue(p(params, 'id'))
    if (!i) return err(404, 'not_found', 'Issue not found.')
    applyIssueUpdate(i, (await request.json()) as IssueUpdate)
    return json(issueDetail(i) as unknown as JsonBodyType)
  }),
  http.get(`${BASE}/issues/:id/events`, async ({ params, request }) => {
    await wait()
    const i = findIssue(p(params, 'id'))
    if (!i) return err(404, 'not_found', 'Issue not found.')
    const proj = world.projects.find((x) => x.id === i.project_id)!
    const url = new URL(request.url)
    const q = (url.searchParams.get('query') ?? '').toLowerCase()
    const ids = eventIds(i)
    let all = ids.map((_, idx) => eventSummary(i, proj, idx)).reverse()
    if (q) {
      const { filters, text } = parseQuery(q)
      all = all.filter(
        (e) =>
          filters.every((f) => {
            const val =
              (f.key === 'environment'
                ? e.environment
                : f.key === 'release'
                  ? e.release
                  : f.key === 'browser'
                    ? e.browser
                    : f.key === 'os'
                      ? e.os
                      : f.key === 'user.email'
                        ? e.user?.email
                        : f.key === 'url'
                          ? e.url
                          : null) ?? ''
            const hit = f.values.some((v) => val.toLowerCase().includes(v.toLowerCase()))
            return f.negated ? !hit : hit
          }) && text.every((t) => JSON.stringify(e).toLowerCase().includes(t)),
      )
    }
    return json(page(all, url) as unknown as JsonBodyType)
  }),
  http.get(`${BASE}/issues/:id/events/:which`, async ({ params }) => {
    await wait()
    const i = findIssue(p(params, 'id'))
    if (!i) return err(404, 'not_found', 'Issue not found.')
    const proj = world.projects.find((x) => x.id === i.project_id)!
    const ids = eventIds(i)
    const which = p(params, 'which')
    let idx: number
    if (which === 'latest') idx = ids.length - 1
    else if (which === 'oldest') idx = 0
    else if (which === 'recommended') idx = Math.max(0, ids.length - 2)
    else idx = ids.indexOf(which)
    if (idx < 0) return err(404, 'not_found', 'Event not found. It may have been deleted by the retention policy.')
    return json(buildEvent(i, proj, idx) as unknown as JsonBodyType)
  }),
  http.get(`${BASE}/issues/:id/tags`, async ({ params }) => {
    await wait()
    const i = findIssue(p(params, 'id'))
    if (!i) return err(404, 'not_found', 'Issue not found.')
    return json(issueTags(i) as unknown as JsonBodyType)
  }),
  http.get(`${BASE}/issues/:id/tags/:key/values`, async ({ params, request }) => {
    await wait()
    const i = findIssue(p(params, 'id'))
    if (!i) return err(404, 'not_found', 'Issue not found.')
    const d = issueTags(i).find((t) => t.key === p(params, 'key'))
    if (!d) return err(404, 'not_found', 'Tag not found.')
    return json({ ...page(d.top_values, new URL(request.url)), key: d.key, total_values: d.total_values } as unknown as JsonBodyType)
  }),
  http.get(`${BASE}/issues/:id/activity`, async ({ params }) => {
    await wait()
    const acts = world.activity.get(p(params, 'id')) ?? []
    return json([...acts].reverse() as unknown as JsonBodyType)
  }),
  http.post(`${BASE}/issues/:id/comments`, async ({ params, request }) => {
    const body = (await request.json()) as { text: string }
    if (!body.text?.trim()) return err(422, 'validation_failed', 'A comment cannot be empty.')
    const u = currentUser()!
    const a: Activity = { id: nid(), kind: 'comment', user: publicUser(u), data: { text: body.text }, created_at: iso(Date.now()) }
    const acts = world.activity.get(p(params, 'id')) ?? []
    acts.push(a)
    world.activity.set(p(params, 'id'), acts)
    return json(a as unknown as JsonBodyType, 201)
  }),
  http.patch(`${BASE}/issues/:id/comments/:cid`, async ({ params, request }) => {
    const acts = world.activity.get(p(params, 'id')) ?? []
    const a = acts.find((x) => x.id === p(params, 'cid'))
    if (!a) return err(404, 'not_found', 'Comment not found.')
    a.data = { ...a.data, text: ((await request.json()) as { text: string }).text, edited: true }
    return json(a as unknown as JsonBodyType)
  }),
  http.delete(`${BASE}/issues/:id/comments/:cid`, ({ params }) => {
    const acts = world.activity.get(p(params, 'id')) ?? []
    world.activity.set(
      p(params, 'id'),
      acts.filter((x) => x.id !== p(params, 'cid')),
    )
    return new HttpResponse(null, { status: 204 })
  }),
  http.get(`${BASE}/issues/:id/stats`, ({ params, request }) => {
    const i = findIssue(p(params, 'id'))
    if (!i) return err(404, 'not_found', 'Issue not found.')
    const period = new URL(request.url).searchParams.get('period')
    const events = period === '30d' ? issueSeries(i, world.now, 30, DAY) : issueSeries(i, world.now, 24, HOUR)
    return json({ period: period ?? '24h', resolution: period === '30d' ? 86400 : 3600, events, users: events.map(([t, c]) => [t, Math.ceil(c / 3)]) })
  }),
  http.get(`${BASE}/issues/:id/feedback`, ({ params }) =>
    json(world.feedback.filter((f) => f.issue_id === p(params, 'id')).map(feedbackOut) as unknown as JsonBodyType),
  ),
  http.get(`${BASE}/issues/:id/attachments`, ({ params }) => {
    const i = findIssue(p(params, 'id'))
    if (!i) return err(404, 'not_found', 'Issue not found.')
    const ids = eventIds(i)
    if (i.platform !== 'javascript' && i.platform !== 'python') return json([])
    return json([
      {
        id: `${i.id}01`,
        event_id: ids[ids.length - 1],
        name: i.platform === 'javascript' ? 'screenshot.png' : 'request-body.json',
        content_type: i.platform === 'javascript' ? 'image/png' : 'application/json',
        size: i.platform === 'javascript' ? 184_220 : 2_311,
        created_at: iso(i.last_seen),
      },
      { id: `${i.id}02`, event_id: ids[ids.length - 1], name: 'app-state.json', content_type: 'application/json', size: 12_904, created_at: iso(i.last_seen) },
    ])
  }),
  http.get(`${BASE}/orgs/:org/events/:project/:eventId`, ({ params }) => {
    for (const i of world.issues) {
      const idx = eventIds(i).indexOf(p(params, 'eventId'))
      if (idx >= 0)
        return json(
          buildEvent(
            i,
            world.projects.find((x) => x.id === i.project_id)!,
            idx,
          ) as unknown as JsonBodyType,
        )
    }
    return err(404, 'not_found', 'Event not found.')
  }),

  // ---- releases ---------------------------------------------------------------
  http.get(`${BASE}/orgs/:org/releases`, async ({ params, request }) => {
    await wait()
    const r = withOrg(params)
    if ('error' in r) return r.error
    const url = new URL(request.url)
    const projects = listParam(url, 'project')
    const q = (url.searchParams.get('query') ?? '').toLowerCase()
    const sort = url.searchParams.get('sort') ?? 'date'
    let list = world.releases.filter(
      (x) => x.org_id === r.org.id && (!projects.length || x.project_ids.some((pid) => projects.includes(pid))) && x.version.toLowerCase().includes(q),
    )
    const sorters: Record<string, (a: (typeof list)[number], b: (typeof list)[number]) => number> = {
      date: (a, b) => b.date_created - a.date_created,
      sessions: (a, b) => b.sessions - a.sessions,
      crash_free: (a, b) => a.crash_free_sessions - b.crash_free_sessions,
      adoption: (a, b) => b.adoption - a.adoption,
    }
    list = [...list].sort(sorters[sort] ?? sorters.date)
    return json({ data: list.map((x) => releaseOut(x, projects)), next_cursor: null } as unknown as JsonBodyType)
  }),
  http.get(`${BASE}/orgs/:org/releases/:version`, async ({ params }) => {
    await wait()
    const r = withOrg(params)
    if ('error' in r) return r.error
    const rel = world.releases.find((x) => x.org_id === r.org.id && x.version === p(params, 'version'))
    if (!rel) return err(404, 'not_found', 'Release not found.')
    // server shape: daily [[unix_seconds, sessions, crashed]] of the last 14 days
    const byDay = new Map<number, [number, number]>()
    for (const h of rel.health) {
      const d = Math.floor(h.ts / 86400) * 86400
      const cur = byDay.get(d) ?? [0, 0]
      byDay.set(d, [cur[0] + h.total, cur[1] + h.crashed])
    }
    const out = {
      ...releaseOut(rel, []),
      deploys: rel.deploys.map((d) => ({ ...d, date_started: d.date_started ? iso(d.date_started) : null, date_finished: iso(d.date_finished) })),
      project_health: [],
      sessions: [...byDay.entries()].sort((a, b) => a[0] - b[0]).map(([t, [n, c]]) => [t, n, c]),
    }
    return json(out as unknown as JsonBodyType)
  }),
  ...telemetryHandlers({
    world: () => world,
    wait,
    withOrg: (params) => withOrg(params as Params),
    canWrite: (orgId) => {
      const role = roleIn(orgId)
      return role !== null && role !== 'viewer'
    },
  }),
]
