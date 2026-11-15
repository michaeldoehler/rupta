// Generates the in-memory "database" behind the mock API: organizations,
// members, teams, projects, issues, releases, monitors, alerts, feedback,
// audit log. Deterministic for a given seed.

import type {
  Activity,
  AlertRule,
  ApiToken,
  ArtifactBundle,
  AuditEntry,
  Channel,
  Checkin,
  CheckinStatus,
  Feedback,
  InstanceSsoInfo,
  IssueStatus,
  IssueSubstatus,
  Level,
  Monitor,
  PendingInvite,
  Priority,
  Project,
  ProjectKey,
  ReleaseHealthPoint,
  Role,
  SavedSearch,
  SsoConfig,
  Team,
  User,
  UserPreferences,
} from '@/api/types'
import { Rng } from '../rng'
import { TEMPLATES, type Template } from './templates'

export const HOUR = 3_600_000
export const DAY = 24 * HOUR

export interface DbUser extends User {
  password: string
  preferences: UserPreferences
  created_at: string
  last_login_at: string | null
}

export interface DbOrg {
  id: string
  slug: string
  name: string
  created_at: string
  settings: Record<string, unknown>
  domain: string | null
  icon_url: string | null
}

/**
 * Stands in for a fetched favicon in the mock: a small mark drawn from the
 * domain, as a data URL, so the demo works offline.
 */
export function mockFavicon(domain: string): string {
  let h = 0
  for (const c of domain) h = (h * 31 + c.charCodeAt(0)) >>> 0
  const hue = h % 360
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><circle cx="12" cy="16" r="9" fill="hsl(${hue} 70% 45%)"/><rect x="14" y="7" width="13" height="18" rx="4" fill="hsl(${(hue + 40) % 360} 80% 55%)" fill-opacity=".85"/></svg>`
  return `data:image/svg+xml,${encodeURIComponent(svg)}`
}

export interface DbMember {
  org_id: string
  user_id: string
  role: Role
  created_at: string
  sso: boolean
}

export interface DbTeam extends Omit<Team, 'members' | 'projects'> {
  org_id: string
  member_ids: string[]
}

export interface DbProject extends Omit<Project, 'team' | 'stats'> {
  org_id: string
  team_id: string | null
  prefix: string
}

export interface DbIssue {
  id: string
  project_id: string
  short_id: string
  template: number
  title: string
  culprit: string
  level: Level
  status: IssueStatus
  substatus: IssueSubstatus
  type: string
  platform: string
  first_seen: number
  last_seen: number
  count: number
  user_count: number
  assignee: { type: 'user' | 'team'; id: string } | null
  priority: Priority
  bookmarked_by: Set<string>
  seen_by: Set<string>
  first_release: string | null
  last_release: string | null
  resolved_in_release: string | null
  ignore_until: number | null
  ignore_count: number | null
  /** Dominant tag values, used for filtering in the mock. */
  tags: Record<string, string>
  handled: boolean
  /** Shape of the event volume: 0 flat, 1 rising, -1 falling, 2 spike. */
  trend: number
}

export interface DbRelease {
  id: string
  org_id: string
  version: string
  project_ids: string[]
  ref: string | null
  url: string | null
  date_created: number
  date_released: number | null
  sessions: number
  users: number
  crash_free_sessions: number
  crash_free_users: number
  adoption: number
  new_issues: number
  deploys: { id: string; environment: string; name: string | null; url: string | null; date_started: number | null; date_finished: number }[]
  health: ReleaseHealthPoint[]
}

export interface World {
  now: number
  users: DbUser[]
  orgs: DbOrg[]
  members: DbMember[]
  invites: (PendingInvite & { org_id: string; token: string })[]
  teams: DbTeam[]
  projects: DbProject[]
  keys: (ProjectKey & { project_id: string })[]
  issues: DbIssue[]
  activity: Map<string, Activity[]>
  releases: DbRelease[]
  monitors: (Omit<Monitor, 'project' | 'recent_checkins'> & { project_id: string; checkins: Checkin[] })[]
  rules: (Omit<AlertRule, 'project'> & { project_id: string })[]
  channels: (Channel & { org_id: string })[]
  feedback: (Omit<Feedback, 'project' | 'issue'> & { project_id: string; issue_id: string | null })[]
  audit: (AuditEntry & { org_id: string })[]
  orgTokens: (ApiToken & { org_id: string })[]
  userTokens: (ApiToken & { user_id: string })[]
  savedSearches: (SavedSearch & { org_id: string; user_id: string | null })[]
  sso: Map<string, SsoConfig>
  /** The instance-wide provider (RUPTA_OIDC_*); `provider: null` when not configured. */
  instanceSso: InstanceSsoInfo
  bundles: (ArtifactBundle & { project_id: string })[]
  nextId: number
}

const FIRST = ['Jane', 'Marcus', 'Priya', 'Tomás', 'Aiko', 'Lena', 'Omar', 'Sofia', 'Kwame', 'Ingrid', 'Diego', 'Mei', 'Noah', 'Fatima', 'Lukas', 'Chloé']
const LAST = [
  'Cooper',
  'Okafor',
  'Raman',
  'García',
  'Tanaka',
  'Fischer',
  'Haddad',
  'Rossi',
  'Mensah',
  'Lindqvist',
  'Herrera',
  'Chen',
  'Becker',
  'Zahra',
  'Novak',
  'Martin',
]

const ENVS = ['production', 'production', 'production', 'staging', 'development']
const BROWSERS = ['Chrome 131', 'Chrome 130', 'Safari 18.1', 'Firefox 132', 'Edge 131', 'Mobile Safari 18.1', 'Chrome Mobile 131', 'Samsung Internet 26']
const OSES_WEB = ['Windows 11', 'macOS 15.1', 'iOS 18.1', 'Android 15', 'Linux', 'Windows 10']
const OSES_SERVER = ['Linux 6.8', 'Linux 6.1', 'Debian 12', 'Alpine 3.20']
const DEVICES = ['Desktop', 'iPhone 15', 'Pixel 8', 'Galaxy S24', 'MacBook Pro', 'iPad Pro']
const SERVERS = ['web-1', 'web-2', 'web-3', 'worker-1', 'worker-2', 'api-7f9c4d', 'api-2b81e0']

export function iso(ms: number) {
  return new Date(ms).toISOString()
}

const PROJECT_DEFS: {
  slug: string
  name: string
  platform: string
  tpl: Template['platform']
  team: string
  prefix: string
  issues: number
  releasePrefix: string
  org: string
}[] = [
  {
    slug: 'web-app',
    name: 'Web App',
    platform: 'javascript-react',
    tpl: 'javascript',
    team: 'frontend',
    prefix: 'WEB',
    issues: 190,
    releasePrefix: 'web-app',
    org: 'acme',
  },
  {
    slug: 'storefront',
    name: 'Storefront',
    platform: 'javascript-nextjs',
    tpl: 'javascript',
    team: 'frontend',
    prefix: 'STORE',
    issues: 120,
    releasePrefix: 'storefront',
    org: 'acme',
  },
  { slug: 'api', name: 'API', platform: 'python-django', tpl: 'python', team: 'backend', prefix: 'API', issues: 150, releasePrefix: 'api', org: 'acme' },
  { slug: 'billing', name: 'Billing', platform: 'php-laravel', tpl: 'php', team: 'backend', prefix: 'BILL', issues: 90, releasePrefix: 'billing', org: 'acme' },
  { slug: 'worker', name: 'Worker', platform: 'node', tpl: 'node', team: 'platform', prefix: 'WRK', issues: 60, releasePrefix: 'worker', org: 'acme' },
  { slug: 'gateway', name: 'Ingest Gateway', platform: 'go', tpl: 'go', team: 'platform', prefix: 'GW', issues: 40, releasePrefix: 'gateway', org: 'acme' },
  { slug: 'blog', name: 'Blog', platform: 'php-symfony', tpl: 'php', team: 'writers', prefix: 'BLOG', issues: 24, releasePrefix: 'blog', org: 'studio' },
]

function variantTitle(t: Template, rng: Rng): string {
  // Small variations so similar templates make distinct issues.
  const base = t.message ? t.value : `${t.type}: ${t.value.split('\n')[0]}`
  if (!rng.chance(0.55)) return base
  const variants: Record<string, string[]> = {
    javascript: ["reading 'length'", "reading 'id'", "reading 'items'", "reading 'name'", "reading 'then'"],
    python: ["'order_id'", "'sku'", "'email'", "'currency'"],
    php: ['"subtotal"', '"shipping"', '"currency"', '"discount"'],
  }
  const v = variants[t.platform]
  if (!v) return base
  if (base.includes("reading 'map'")) return base.replace("reading 'map'", rng.pick(v))
  if (base.includes("'customer_id'")) return base.replace("'customer_id'", rng.pick(v))
  if (base.includes('"total"')) return base.replace('"total"', rng.pick(v))
  return base
}

export function createWorld(opts: { seed?: number; scale?: number; now?: number } = {}): World {
  const rng = new Rng(opts.seed ?? 20261114)
  const now = opts.now ?? Date.now()
  let nextId = 1000
  const id = () => String(nextId++)

  // ---- users, orgs, members, teams -----------------------------------------
  const users: DbUser[] = FIRST.map((first, i) => ({
    id: String(i + 1),
    email: `${first
      .toLowerCase()
      .normalize('NFD')
      .replace(/[^a-z]/g, '')}.${LAST[i]
      .toLowerCase()
      .normalize('NFD')
      .replace(/[^a-z]/g, '')}@acme.dev`,
    name: `${first} ${LAST[i]}`,
    avatar_url: null,
    is_superuser: i === 0,
    password: 'password',
    preferences: {
      theme: 'system',
      timezone: 'browser',
      clock_24h: true,
      notifications: { issue_alerts: true, workflow: 'participating', weekly_report: true, deploys: false },
    },
    created_at: iso(now - (400 - i * 13) * DAY),
    last_login_at: iso(now - rng.int(1, 200) * HOUR),
  }))
  users[0].email = 'jane@acme.dev'

  const orgs: DbOrg[] = [
    {
      id: '1',
      slug: 'acme',
      name: 'Acme',
      created_at: iso(now - 400 * DAY),
      settings: { require_2fa: false, allow_join_requests: false, default_role: 'member', events_member_admin: true },
      domain: 'acme.dev',
      icon_url: mockFavicon('acme.dev'),
    },
    { id: '2', slug: 'studio', name: 'Side Studio', created_at: iso(now - 120 * DAY), settings: {}, domain: null, icon_url: null },
  ]
  const roles: Role[] = [
    'owner',
    'admin',
    'admin',
    'member',
    'member',
    'member',
    'member',
    'member',
    'member',
    'member',
    'viewer',
    'member',
    'member',
    'viewer',
    'member',
    'member',
  ]
  const members: DbMember[] = users.map((u, i) => ({ org_id: '1', user_id: u.id, role: roles[i], created_at: u.created_at, sso: i % 3 === 1 }))
  members.push({ org_id: '2', user_id: '1', role: 'owner', created_at: iso(now - 120 * DAY), sso: false })
  members.push({ org_id: '2', user_id: '6', role: 'member', created_at: iso(now - 90 * DAY), sso: false })

  const teams: DbTeam[] = [
    { id: '11', org_id: '1', slug: 'frontend', name: 'Frontend', created_at: iso(now - 380 * DAY), member_ids: ['1', '3', '5', '9', '12'] },
    { id: '12', org_id: '1', slug: 'backend', name: 'Backend', created_at: iso(now - 380 * DAY), member_ids: ['1', '2', '4', '7', '13', '15'] },
    { id: '13', org_id: '1', slug: 'platform', name: 'Platform', created_at: iso(now - 300 * DAY), member_ids: ['2', '8', '10', '16'] },
    { id: '14', org_id: '1', slug: 'mobile', name: 'Mobile', created_at: iso(now - 200 * DAY), member_ids: ['6', '11', '14'] },
    { id: '21', org_id: '2', slug: 'writers', name: 'Writers', created_at: iso(now - 110 * DAY), member_ids: ['1', '6'] },
  ]

  const invites: World['invites'] = [
    {
      id: id(),
      org_id: '1',
      email: 'ravi.patel@acme.dev',
      role: 'member',
      created_at: iso(now - 2 * DAY),
      expires_at: iso(now + 5 * DAY),
      token: 'invite-demo',
    },
    {
      id: id(),
      org_id: '1',
      email: 'contractor@agency.io',
      role: 'viewer',
      created_at: iso(now - 6 * DAY),
      expires_at: iso(now + DAY),
      token: 'invite-demo-2',
    },
  ]

  // ---- projects & keys ------------------------------------------------------
  const projects: DbProject[] = []
  const keys: World['keys'] = []
  PROJECT_DEFS.forEach((p, i) => {
    const pid = String(i + 1)
    const team = teams.find((t) => t.slug === p.team)
    projects.push({
      id: pid,
      org_id: p.org === 'acme' ? '1' : '2',
      slug: p.slug,
      name: p.name,
      platform: p.platform,
      team_id: team?.id ?? null,
      created_at: iso(now - (380 - i * 30) * DAY),
      prefix: p.prefix,
      settings: {
        allowed_origins: p.tpl === 'javascript' ? ['https://app.acme.dev', 'https://*.acme.dev'] : ['*'],
        rate_limit_per_min: p.slug === 'web-app' ? 6000 : null,
        retention_days: null,
        filters: {
          browser_extensions: p.tpl === 'javascript',
          localhost: true,
          legacy_browsers: false,
          web_crawlers: true,
          error_messages: p.slug === 'web-app' ? ['ResizeObserver loop *'] : [],
          releases: [],
        },
        scrubbing: { enabled: true, extra_fields: p.slug === 'billing' ? ['iban', 'tax_id'] : [], safe_fields: [], scrub_ip_addresses: p.slug === 'api' },
        in_app_include: [],
        in_app_exclude: [],
      },
    })
    const pub = rng.hex(32)
    keys.push({
      id: id(),
      project_id: pid,
      label: 'Default',
      public_key: pub,
      secret_key: rng.hex(32),
      is_active: true,
      rate_limit_per_min: null,
      created_at: iso(now - (380 - i * 30) * DAY),
      dsn: `https://${pub}@rupta.acme.dev/${pid}`,
    })
    if (i === 0) {
      const pub2 = rng.hex(32)
      keys.push({
        id: id(),
        project_id: pid,
        label: 'Mobile web',
        public_key: pub2,
        secret_key: rng.hex(32),
        is_active: false,
        rate_limit_per_min: 600,
        created_at: iso(now - 40 * DAY),
        dsn: `https://${pub2}@rupta.acme.dev/${pid}`,
      })
    }
  })

  // ---- releases -------------------------------------------------------------
  const releases: DbRelease[] = []
  PROJECT_DEFS.forEach((p, i) => {
    const pid = String(i + 1)
    const count = p.org === 'acme' ? 9 : 4
    let major = 1 + (i % 3)
    let minor = rng.int(2, 14)
    let patch = 0
    for (let r = 0; r < count; r++) {
      const age = (count - r) * rng.int(6, 11) * DAY - rng.int(0, 20) * HOUR
      if (rng.chance(0.3)) {
        minor += 1
        patch = 0
      } else patch += 1
      if (minor > 20) {
        major += 1
        minor = 0
      }
      const version = `${p.releasePrefix}@${major}.${minor}.${patch}`
      const latest = r === count - 1
      const created = now - Math.max(age, 3 * HOUR)
      const sessions = rng.int(4000, 180000) * (latest ? 1 : 2)
      const crashFree = latest && i === 0 ? 97.82 : 99.1 + rng.float() * 0.89
      const health: ReleaseHealthPoint[] = []
      const points = 48
      const span = Math.min(now - created, 14 * DAY)
      for (let h = 0; h < points; h++) {
        const ts = now - span + (span / points) * h
        const total = Math.round((sessions / points) * (0.6 + 0.8 * Math.sin((h / points) * Math.PI) + rng.float() * 0.3))
        const crashed = Math.round(total * (1 - crashFree / 100) * (0.5 + rng.float()))
        const errored = Math.round(total * 0.02 * rng.float())
        health.push({ ts: Math.round(ts / 1000), total, crashed, errored, abnormal: Math.round(crashed * 0.1) })
      }
      releases.push({
        id: id(),
        org_id: p.org === 'acme' ? '1' : '2',
        version,
        project_ids: [pid],
        ref: rng.hex(40),
        url: null,
        date_created: created,
        date_released: created + rng.int(1, 8) * HOUR,
        sessions,
        users: Math.round(sessions / rng.int(3, 8)),
        crash_free_sessions: Number(crashFree.toFixed(2)),
        crash_free_users: Number(Math.min(99.99, crashFree + 0.4).toFixed(2)),
        adoption: latest ? rng.int(45, 92) : r === count - 2 ? rng.int(5, 30) : rng.int(0, 3),
        new_issues: 0,
        deploys: [
          { id: id(), environment: 'staging', name: null, url: null, date_started: created + HOUR, date_finished: created + HOUR + 4 * 60_000 },
          ...(latest && i === 1
            ? []
            : [
                {
                  id: id(),
                  environment: 'production',
                  name: `Deploy #${rng.int(200, 900)}`,
                  url: null,
                  date_started: created + 3 * HOUR,
                  date_finished: created + 3 * HOUR + 6 * 60_000,
                },
              ]),
        ],
        health,
      })
    }
  })
  const releasesFor = (pid: string) => releases.filter((r) => r.project_ids.includes(pid)).sort((a, b) => a.date_created - b.date_created)

  // ---- issues ---------------------------------------------------------------
  const scale = opts.scale ?? 1
  const issues: DbIssue[] = []
  const activity = new Map<string, Activity[]>()
  PROJECT_DEFS.forEach((p, pi) => {
    const pid = String(pi + 1)
    const tpls = TEMPLATES.map((t, ti) => ({ t, ti })).filter(
      ({ t }) => t.platform === p.tpl || (p.platform === 'javascript-nextjs' && t.platform === 'node' && rng.chance(0.3)),
    )
    const projectReleases = releasesFor(pid)
    const n = Math.round(p.issues * scale)
    for (let k = 0; k < n; k++) {
      const { t, ti } = rng.weighted(tpls.map((x) => [x, x.t.weight ?? 1] as const))
      const firstSeen = now - rng.powerLaw(1, 90 * 24, 1.3) * HOUR - rng.int(0, 3600) * 1000
      const recentness = rng.weighted([
        [0.02, 4],
        [0.2, 3],
        [1, 2],
        [3, 1],
      ] as const)
      const lastSeen = Math.min(now - rng.int(5, 600) * 1000, firstSeen + (now - firstSeen) * (1 - rng.float() * recentness * 0.3))
      const count = rng.powerLaw(1, k < 8 ? 250000 : 40000, 1.6)
      const userCount = Math.max(t.platform === 'go' ? 0 : 1, Math.round(count * (0.05 + rng.float() * 0.5)))
      const statusRoll = rng.float()
      let status: IssueStatus = 'unresolved'
      let substatus: IssueSubstatus = 'ongoing'
      if (statusRoll < 0.13) {
        status = 'resolved'
        substatus = null
      } else if (statusRoll < 0.22) {
        status = 'ignored'
        substatus = rng.pick(['archived_until_escalating', 'archived_forever', 'archived_until_condition_met'] as const)
      } else if (now - firstSeen < 7 * DAY) substatus = 'new'
      else if (rng.chance(0.08)) substatus = 'regressed'
      else if (rng.chance(0.06)) substatus = 'escalating'
      const firstRel = [...projectReleases].reverse().find((r) => r.date_created <= firstSeen) ?? projectReleases[0]
      const lastRel = [...projectReleases].reverse().find((r) => r.date_created <= lastSeen) ?? firstRel
      if (firstRel) firstRel.new_issues += 1
      const assigned = rng.chance(0.32)
      const orgMembers = members.filter((m) => m.org_id === (p.org === 'acme' ? '1' : '2'))
      const browser = t.platform === 'javascript' ? rng.pick(BROWSERS) : ''
      const issueId = String(5000 + issues.length)
      const issue: DbIssue = {
        id: issueId,
        project_id: pid,
        short_id: `${p.slug.toUpperCase()}-${k + 1}`,
        template: ti,
        title: variantTitle(t, rng),
        culprit: t.culprit,
        level: rng.chance(0.08) && t.level === 'error' ? 'warning' : t.level,
        status,
        substatus,
        type: t.message ? 'default' : 'error',
        platform: t.platform === 'javascript' ? 'javascript' : t.platform,
        first_seen: firstSeen,
        last_seen: Math.max(firstSeen, lastSeen),
        count,
        user_count: userCount,
        assignee: assigned
          ? rng.chance(0.75)
            ? { type: 'user', id: rng.pick(orgMembers).user_id }
            : { type: 'team', id: rng.pick(teams.filter((tm) => tm.org_id === (p.org === 'acme' ? '1' : '2'))).id }
          : null,
        priority: t.level === 'fatal' ? 'high' : count > 5000 ? 'high' : count < 50 ? 'low' : 'medium',
        bookmarked_by: new Set(rng.chance(0.05) ? ['1'] : []),
        seen_by: new Set(rng.chance(0.6) ? ['1'] : []),
        first_release: firstRel?.version ?? null,
        last_release: lastRel?.version ?? null,
        resolved_in_release: status === 'resolved' && rng.chance(0.5) ? (lastRel?.version ?? null) : null,
        ignore_until: null,
        ignore_count: null,
        tags: {
          environment: rng.pick(ENVS),
          release: lastRel?.version ?? '',
          ...(browser ? { browser, 'browser.name': browser.replace(/ [\d.]+$/, '') } : {}),
          os: t.platform === 'javascript' ? rng.pick(OSES_WEB) : rng.pick(OSES_SERVER),
          device: t.platform === 'javascript' ? rng.pick(DEVICES) : '',
          server_name: t.platform === 'javascript' ? '' : rng.pick(SERVERS),
          transaction: t.transaction,
          'error.type': t.type ?? '',
          handled: t.handled ? 'yes' : 'no',
        },
        handled: t.handled,
        trend:
          substatus === 'escalating'
            ? 1
            : substatus === 'regressed'
              ? 2
              : rng.weighted([
                  [0, 6],
                  [1, 1],
                  [-1, 2],
                  [2, 1],
                ] as const),
      }
      issues.push(issue)
      const acts: Activity[] = [{ id: id(), kind: 'first_seen', user: null, data: {}, created_at: iso(firstSeen) }]
      if (issue.assignee) {
        const actor = users[rng.int(0, 4)]
        const a = issue.assignee
        const assigneeName = a.type === 'user' ? users.find((u) => u.id === a.id)?.name : `#${teams.find((t) => t.id === a.id)?.slug}`
        acts.push({
          id: id(),
          kind: 'assigned',
          user: actor,
          data: { assignee: { ...a, name: assigneeName } },
          created_at: iso(Math.min(issue.last_seen, firstSeen + rng.int(1, 48) * HOUR)),
        })
      }
      if (status === 'resolved')
        acts.push({
          id: id(),
          kind: issue.resolved_in_release ? 'set_resolved_in_release' : 'set_resolved',
          user: users[rng.int(0, 6)],
          data: { version: issue.resolved_in_release },
          created_at: iso(lastSeen + HOUR),
        })
      if (substatus === 'regressed')
        acts.push({ id: id(), kind: 'set_regression', user: null, data: { version: issue.last_release }, created_at: iso(lastSeen - 2 * DAY) })
      activity.set(issueId, acts)
    }
  })
  // A comment thread on the very first issues, for the detail screens.
  const commenters = [users[1], users[2], users[0]]
  issues.slice(0, 12).forEach((iss, i) => {
    const acts = activity.get(iss.id) ?? []
    acts.push({
      id: id(),
      kind: 'comment',
      user: commenters[i % 3],
      data: {
        text:
          i % 2 === 0
            ? 'Started after the **catalog refactor** shipped. The API sometimes returns `{ "items": null }` for empty categories — we should default to an empty array.\n\n- [x] reproduce locally\n- [ ] fix in `useProducts`'
            : 'Looks related to the connection pool limits we changed last week. See the deploy notes.',
      },
      created_at: iso(iss.last_seen - 3 * HOUR),
    })
    if (i % 2 === 0)
      acts.push({
        id: id(),
        kind: 'comment',
        user: commenters[(i + 1) % 3],
        data: { text: 'I can take this one. PR coming today.' },
        created_at: iso(iss.last_seen - HOUR),
      })
  })

  // Make the most recent issues "look" fresh and busy for the screenshots.
  issues.sort((a, b) => b.last_seen - a.last_seen)

  // ---- monitors -------------------------------------------------------------
  const monitorDefs: [string, string, string, Monitor['schedule'], number, number][] = [
    ['nightly-backup', 'Nightly database backup', '5', { type: 'crontab', value: '0 3 * * *' }, 0.02, 1_200_000],
    ['invoice-run', 'Monthly invoice run', '4', { type: 'crontab', value: '0 6 1 * *' }, 0, 3_400_000],
    ['cleanup-sessions', 'Clean up expired sessions', '3', { type: 'interval', value: 1, unit: 'hour' }, 0.04, 8_000],
    ['sync-exchange-rates', 'Sync exchange rates', '3', { type: 'crontab', value: '*/30 * * * *' }, 0.18, 2_400],
    ['send-digest-emails', 'Send digest emails', '5', { type: 'crontab', value: '0 8 * * 1-5' }, 0.08, 95_000],
    ['reindex-search', 'Reindex product search', '3', { type: 'interval', value: 6, unit: 'hour' }, 0.3, 610_000],
    ['rotate-logs', 'Rotate gateway logs', '6', { type: 'crontab', value: '15 0 * * *' }, 0, 3_000],
    ['export-reports', 'Export analytics reports', '4', { type: 'crontab', value: '30 2 * * *' }, 0.12, 180_000],
  ]
  const monitors: World['monitors'] = monitorDefs.map(([slug, name, pid, schedule, failRate, dur], mi) => {
    const intervalMs =
      schedule.type === 'interval'
        ? Number(schedule.value) * (schedule.unit === 'hour' ? HOUR : 60_000)
        : String(schedule.value).startsWith('*/30')
          ? 30 * 60_000
          : String(schedule.value).includes('* * 1-5') || String(schedule.value).match(/^\d+ \d+ \* \* \*$/)
            ? DAY
            : 30 * DAY
    const checkins: Checkin[] = []
    const n = 60
    for (let c = 0; c < n; c++) {
      const ts = now - (n - c) * intervalMs + rng.int(0, 30) * 1000
      if (ts < now - 120 * DAY) continue
      const roll = rng.float()
      let status: CheckinStatus = 'ok'
      if (mi === 5 && c > n - 4) status = c === n - 1 ? 'timeout' : 'error'
      else if (mi === 3 && c === n - 2) status = 'missed'
      else if (roll < failRate) status = rng.pick(['error', 'missed', 'timeout'] as const)
      checkins.push({
        id: rng.uuid(),
        status,
        environment: 'production',
        duration_ms: status === 'missed' ? null : Math.round(dur * (0.6 + rng.float() * 0.8)),
        created_at: iso(ts),
      })
    }
    const last = checkins[checkins.length - 1]
    const health = last ? (last.status === 'ok' ? 'ok' : last.status === 'in_progress' ? 'ok' : last.status) : 'unknown'
    return {
      id: id(),
      slug,
      name,
      project_id: pid,
      schedule,
      timezone: mi === 4 ? 'Europe/Berlin' : 'UTC',
      checkin_margin: 5,
      max_runtime: Math.ceil(dur / 60_000) * 2 + 5,
      status: mi === 6 ? 'disabled' : 'active',
      health: mi === 6 ? 'unknown' : (health as Monitor['health']),
      last_checkin: last?.created_at ?? null,
      next_checkin: last ? iso(new Date(last.created_at).getTime() + intervalMs) : null,
      created_at: iso(now - 200 * DAY),
      checkins: checkins.reverse(),
    }
  })

  // ---- channels & alert rules ----------------------------------------------
  const channels: World['channels'] = [
    {
      id: id(),
      org_id: '1',
      kind: 'slack',
      name: '#alerts-prod',
      config: { webhook_url: 'https://hooks.slack.com/services/T000/B000/XXXX' },
      created_at: iso(now - 200 * DAY),
    },
    {
      id: id(),
      org_id: '1',
      kind: 'email',
      name: 'On-call list',
      config: { to: ['oncall@acme.dev', 'platform-leads@acme.dev'] },
      created_at: iso(now - 180 * DAY),
    },
    {
      id: id(),
      org_id: '1',
      kind: 'webhook',
      name: 'Incident bot',
      config: { url: 'https://bots.acme.dev/rupta', has_secret: true },
      created_at: iso(now - 90 * DAY),
    },
    {
      id: id(),
      org_id: '1',
      kind: 'teams',
      name: 'Platform channel',
      config: { webhook_url: 'https://acme.webhook.office.com/webhookb2/…' },
      created_at: iso(now - 30 * DAY),
    },
  ]
  const ruleBase = { created_by: users[0].name, created_at: iso(now - 100 * DAY), updated_at: iso(now - 10 * DAY), environment: 'production' as string | null }
  const rules: World['rules'] = [
    {
      ...ruleBase,
      id: id(),
      project_id: '1',
      name: 'New issue in production',
      enabled: true,
      conditions: [{ type: 'first_seen' }],
      filters: [{ type: 'level', match: 'gte', level: 'error' }],
      actions: [{ type: 'channel', channel_id: channels[0].id }],
      frequency_minutes: 30,
      last_fired: iso(now - 2 * HOUR),
    },
    {
      ...ruleBase,
      id: id(),
      project_id: '1',
      name: 'Error spike',
      enabled: true,
      conditions: [{ type: 'event_frequency', count: 500, interval: '1h' }],
      filters: [],
      actions: [
        { type: 'channel', channel_id: channels[0].id },
        { type: 'email', user_ids: [], team_ids: ['11'] },
      ],
      frequency_minutes: 60,
      last_fired: iso(now - 3 * DAY),
    },
    {
      ...ruleBase,
      id: id(),
      project_id: '3',
      name: 'Regressions',
      enabled: true,
      conditions: [{ type: 'regression' }, { type: 'reappeared' }],
      filters: [],
      actions: [{ type: 'email', user_ids: ['1', '2'], team_ids: ['12'] }],
      frequency_minutes: 1440,
      last_fired: null,
    },
    {
      ...ruleBase,
      id: id(),
      project_id: '4',
      name: 'Checkout failures affecting many users',
      enabled: true,
      conditions: [{ type: 'user_frequency', count: 100, interval: '1h' }],
      filters: [{ type: 'tag', key: 'transaction', value: 'POST /checkout/summary' }],
      actions: [{ type: 'channel', channel_id: channels[2].id }],
      frequency_minutes: 30,
      last_fired: iso(now - 9 * DAY),
    },
    {
      ...ruleBase,
      id: id(),
      project_id: '6',
      name: 'Gateway panics',
      enabled: false,
      conditions: [{ type: 'first_seen' }, { type: 'regression' }],
      filters: [{ type: 'level', match: 'eq', level: 'fatal' }],
      actions: [{ type: 'channel', channel_id: channels[3].id }],
      frequency_minutes: 5,
      last_fired: null,
    },
  ]

  // ---- feedback ---------------------------------------------------------------
  const messages = [
    'The checkout button does nothing after I enter my card. Tried twice.',
    'Page went blank when I opened my orders. Refreshing fixed it.',
    'I got an error saying "Something went wrong" while saving my profile.',
    'Search results stopped loading after page 3.',
    'The invoice PDF link leads to an error page.',
    'App crashed when switching categories quickly.',
    'Could not apply my coupon WINTER25, it shows an error.',
    'Loading spinner forever on the dashboard this morning.',
    'Prices show NaN in the cart on Safari.',
    'Thanks for the quick fix yesterday! Works now.',
  ]
  const feedback: World['feedback'] = []
  for (let f = 0; f < 46; f++) {
    const iss = rng.pick(issues.filter((x) => ['1', '2', '3', '4'].includes(x.project_id)).slice(0, 80))
    const anon = rng.chance(0.2)
    const fn = rng.pick(FIRST)
    feedback.push({
      id: id(),
      project_id: iss.project_id,
      event_id: rng.uuid(),
      issue_id: rng.chance(0.85) ? iss.id : null,
      name: anon ? null : `${fn} ${rng.pick(LAST)}`,
      email: anon
        ? null
        : `${fn
            .toLowerCase()
            .normalize('NFD')
            .replace(/[^a-z]/g, '')}${rng.int(1, 99)}@example.com`,
      message: rng.pick(messages),
      url: `https://app.acme.dev${iss.tags.transaction.startsWith('/') ? iss.tags.transaction.replace(':category', 'shoes') : '/'}`,
      created_at: iso(now - rng.powerLaw(1, 60 * 24, 1.2) * HOUR),
    })
  }
  feedback.sort((a, b) => b.created_at.localeCompare(a.created_at))

  // ---- audit log --------------------------------------------------------------
  const auditActions: [string, string, (r: Rng) => Record<string, unknown>][] = [
    ['member.invite', 'member', (r) => ({ email: `${r.pick(FIRST).toLowerCase()}@acme.dev`, role: 'member' })],
    ['member.role_change', 'member', () => ({ from: 'member', to: 'admin' })],
    ['member.remove', 'member', () => ({})],
    ['project.create', 'project', () => ({ slug: 'worker' })],
    ['project.update', 'project', () => ({ fields: ['settings.filters'] })],
    ['project_key.create', 'project_key', () => ({ label: 'Mobile web' })],
    ['project_key.disable', 'project_key', () => ({})],
    ['alert_rule.create', 'alert_rule', () => ({ name: 'Error spike' })],
    ['alert_rule.update', 'alert_rule', () => ({ enabled: false })],
    ['api_token.create', 'api_token', () => ({ name: 'CI uploads', scopes: ['project:releases'] })],
    ['api_token.delete', 'api_token', () => ({})],
    ['sso.update', 'sso_config', () => ({ provider: 'Zitadel' })],
    ['auth.login', 'user', () => ({ method: 'password' })],
    ['auth.login', 'user', () => ({ method: 'sso' })],
    ['auth.login_failed', 'user', () => ({ reason: 'invalid_password' })],
    ['issue.delete', 'issue', () => ({ count: 3 })],
    ['team.create', 'team', () => ({ slug: 'mobile' })],
    ['channel.create', 'channel', () => ({ kind: 'slack' })],
    ['org.update', 'organization', () => ({ fields: ['name'] })],
  ]
  const audit: World['audit'] = []
  for (let a = 0; a < 140; a++) {
    const [action, target, data] = rng.pick(auditActions)
    const actorUser = rng.pick(users.slice(0, 6))
    const viaToken = action.startsWith('api_token') ? false : rng.chance(0.08)
    audit.push({
      id: String(90000 - a),
      org_id: '1',
      actor: viaToken ? { type: 'token', id: '77', name: 'CI uploads' } : { type: 'user', id: actorUser.id, name: actorUser.name },
      action,
      target_type: target,
      target_id: String(rng.int(1, 500)),
      data: data(rng),
      ip: `203.0.113.${rng.int(2, 250)}`,
      created_at: iso(now - a * rng.int(1, 9) * HOUR - rng.int(0, 3600) * 1000),
    })
  }

  // ---- tokens, saved searches, bundles ---------------------------------------
  const orgTokens: World['orgTokens'] = [
    {
      id: '77',
      org_id: '1',
      name: 'CI uploads',
      token_hint: 'rpt_…3f9a',
      scopes: ['project:releases', 'project:read'],
      created_at: iso(now - 150 * DAY),
      last_used_at: iso(now - 5 * HOUR),
      expires_at: null,
    },
    {
      id: '78',
      org_id: '1',
      name: 'Grafana dashboard',
      token_hint: 'rpt_…b21c',
      scopes: ['org:read', 'project:read'],
      created_at: iso(now - 60 * DAY),
      last_used_at: iso(now - 20 * 60_000),
      expires_at: iso(now + 300 * DAY),
    },
  ]
  const userTokens: World['userTokens'] = [
    {
      id: '81',
      user_id: '1',
      name: 'Local scripts',
      token_hint: 'rpt_…77d0',
      scopes: ['project:read', 'issue:write'],
      created_at: iso(now - 30 * DAY),
      last_used_at: iso(now - 3 * DAY),
      expires_at: null,
    },
  ]
  const savedSearches: World['savedSearches'] = [
    {
      id: id(),
      org_id: '1',
      user_id: null,
      name: 'Unhandled in production',
      query: 'is:unresolved handled:no environment:production',
      sort: 'freq',
      is_shared: true,
      created_at: iso(now - 50 * DAY),
    },
    {
      id: id(),
      org_id: '1',
      user_id: '1',
      name: 'My open issues',
      query: 'is:unresolved assigned:me',
      sort: 'last_seen',
      is_shared: false,
      created_at: iso(now - 20 * DAY),
    },
    { id: id(), org_id: '1', user_id: null, name: 'Fatal crashes', query: 'level:fatal', sort: 'last_seen', is_shared: true, created_at: iso(now - 10 * DAY) },
    {
      id: id(),
      org_id: '1',
      user_id: '1',
      name: 'Safari only',
      query: 'is:unresolved browser:"Safari 18.1"',
      sort: 'users',
      is_shared: false,
      created_at: iso(now - 4 * DAY),
    },
  ]
  const bundles: World['bundles'] = []
  releasesFor('1')
    .slice(-5)
    .forEach((r, bi) =>
      bundles.push({
        id: id(),
        project_id: '1',
        checksum: rng.hex(40),
        release: r.version,
        dist: null,
        file_count: 24 + bi * 2,
        size: 3_400_000 + bi * 120_000,
        created_at: iso(r.date_created),
      }),
    )
  releasesFor('2')
    .slice(-2)
    .forEach((r) =>
      bundles.push({
        id: id(),
        project_id: '2',
        checksum: rng.hex(40),
        release: null,
        dist: null,
        file_count: 61,
        size: 8_900_000,
        created_at: iso(r.date_created),
      }),
    )

  return {
    now,
    users,
    orgs,
    members,
    invites,
    teams,
    projects,
    keys,
    issues,
    activity,
    releases,
    monitors,
    rules,
    channels,
    feedback,
    audit,
    orgTokens,
    userTokens,
    savedSearches,
    sso: new Map(),
    instanceSso: {
      local_login: true,
      provider: {
        provider_id: 'oidc',
        provider_name: 'Zitadel',
        issuer: 'https://auth.acme.dev',
        client_id: '2930413377723@rupta',
        has_client_secret: true,
        scopes: 'openid email profile urn:zitadel:iam:org:project:roles',
        role_claim: 'urn:zitadel:iam:org:project:roles',
        role_mapping: { 'rupta-admin': 'admin', 'rupta-member': 'member', 'rupta-viewer': 'viewer' },
        default_role: 'viewer',
        jit_provisioning: true,
        allowed_audiences: [],
        redirect_uri: 'https://errors.acme.dev/auth/sso/oidc/callback',
        default_org: 'acme',
      },
    },
    bundles,
    nextId,
  }
}

/** Event volume per bucket for an issue (deterministic). */
export function issueSeries(issue: DbIssue, now: number, buckets: number, bucketMs: number): [number, number][] {
  const rng = new Rng(`${issue.id}:${buckets}:${bucketMs}`)
  const out: [number, number][] = []
  const end = Math.floor(now / bucketMs) * bucketMs
  const start = end - (buckets - 1) * bucketMs
  const span = Math.max(1, issue.last_seen - issue.first_seen)
  const perMs = issue.count / span
  let remaining = issue.count
  for (let b = 0; b < buckets; b++) {
    const ts = start + b * bucketMs
    if (ts + bucketMs < issue.first_seen || ts > issue.last_seen + bucketMs) {
      out.push([Math.floor(ts / 1000), 0])
      continue
    }
    const x = b / (buckets - 1)
    let shape = 1
    if (issue.trend === 1) shape = 0.2 + 1.8 * x * x
    else if (issue.trend === -1) shape = 1.6 - 1.2 * x
    else if (issue.trend === 2) shape = Math.abs(b - Math.floor(buckets * 0.7)) <= 1 ? 4 : 0.6
    const daily = 0.75 + 0.5 * Math.sin((((ts / HOUR) % 24) / 24) * 2 * Math.PI)
    const raw = perMs * bucketMs * shape * daily * (0.6 + rng.float() * 0.8)
    // stochastic rounding keeps small issues visible instead of all zeros
    let v = Math.floor(raw) + (rng.float() < raw - Math.floor(raw) ? 1 : 0)
    if (v === 0 && issue.last_seen >= ts && issue.last_seen < ts + bucketMs) v = 1
    v = Math.min(v, remaining)
    remaining -= v
    out.push([Math.floor(ts / 1000), v])
  }
  return out
}
