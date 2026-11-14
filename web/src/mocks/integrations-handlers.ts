// Mock implementation of the issue tracker integrations API
// (docs/integrations.md): integrations, targets, project defaults, issue
// links and the ticket draft.

import { delay, http, HttpResponse, type JsonBodyType } from 'msw'
import type { IntegrationKind } from '@/api/integrations'
import { Rng } from './rng'
import { iso, type DbOrg, type World } from './fixtures/world'

export interface IntegrationsContext {
  world: () => World
  withOrg: (params: Record<string, unknown>) => { error: Response } | { org: DbOrg }
  canWrite: (orgId: string) => boolean
  isAdmin: (orgId: string) => boolean
  actor: () => { id: string; email: string; name: string; avatar_url: string | null; is_superuser: boolean } | null
  nid: () => string
}

interface DbIntegration {
  id: string
  org_id: string
  kind: IntegrationKind
  name: string
  enabled: boolean
  config: Record<string, string>
  token: string
  webhook_secret: string | null
  account: { id: string; name: string } | null
  created_by: string | null
  created_at: number
  updated_at: number
  last_error: string | null
  last_error_at: number | null
}

interface DbDefaults {
  project_id: string
  integration_id: string
  target: string
  target_name: string | null
  issue_type: string | null
  project: string | null
  labels: string[]
  incoming_sync: boolean
  outgoing_transition: boolean
  outgoing_comment: boolean
  updated_at: number
}

interface DbLink {
  id: string
  issue_id: string
  integration_id: string
  external_id: string
  external_key: string
  url: string
  title: string | null
  status: string | null
  status_category: 'open' | 'done'
  assignee: string | null
  created_by: string | null
  created_at: number
  synced_at: number | null
}

const BASE = '/api/v1'
const json = (body: unknown, status = 200) => HttpResponse.json(body as JsonBodyType, { status })
const err = (status: number, code: string, message: string, details: Record<string, unknown> = {}) =>
  HttpResponse.json({ error: { code, message, details } }, { status })
const p = (params: Record<string, unknown>, k: string) => String(params[k] ?? '')

interface State {
  world: World
  integrations: DbIntegration[]
  defaults: DbDefaults[]
  links: DbLink[]
}

let state: State | null = null

const TARGETS: Record<IntegrationKind, unknown[]> = {
  jira_cloud: [
    {
      id: 'OPS',
      name: 'Operations',
      key: 'OPS',
      issue_types: [
        { id: '10001', name: 'Bug' },
        { id: '10002', name: 'Task' },
        { id: '10003', name: 'Story' },
      ],
      projects: [],
      labels: [],
      states: [],
    },
    {
      id: 'WEB',
      name: 'Web storefront',
      key: 'WEB',
      issue_types: [
        { id: '10001', name: 'Bug' },
        { id: '10002', name: 'Task' },
      ],
      projects: [],
      labels: [],
      states: [],
    },
    {
      id: 'PAY',
      name: 'Payments',
      key: 'PAY',
      issue_types: [
        { id: '10001', name: 'Bug' },
        { id: '10004', name: 'Incident' },
      ],
      projects: [],
      labels: [],
      states: [],
    },
  ],
  jira_server: [
    {
      id: 'PLAT',
      name: 'Platform',
      key: 'PLAT',
      issue_types: [
        { id: '1', name: 'Bug' },
        { id: '3', name: 'Task' },
      ],
      projects: [],
      labels: [],
      states: [],
    },
  ],
  linear: [
    {
      id: '5c1f0a3e-7d1b-4b8e-9a77-0e7e1f2a9c11',
      name: 'Engineering',
      key: 'ENG',
      issue_types: [],
      projects: [
        { id: 'p1', name: 'Checkout revamp' },
        { id: 'p2', name: 'Reliability' },
      ],
      labels: [
        { id: 'l1', name: 'Bug' },
        { id: 'l2', name: 'rupta' },
        { id: 'l3', name: 'Frontend' },
      ],
      states: [
        { id: 's1', name: 'Backlog', category: 'backlog' },
        { id: 's2', name: 'Todo', category: 'unstarted' },
        { id: 's3', name: 'In Progress', category: 'started' },
        { id: 's4', name: 'Done', category: 'completed' },
        { id: 's5', name: 'Canceled', category: 'canceled' },
      ],
    },
  ],
  github: [
    { id: 'acme/web-app', name: 'acme/web-app', key: null, issue_types: [], projects: [], labels: [], states: [] },
    { id: 'acme/api', name: 'acme/api', key: null, issue_types: [], projects: [], labels: [], states: [] },
  ],
  gitlab: [{ id: 'acme/billing', name: 'acme/billing', key: null, issue_types: [], projects: [], labels: [], states: [] }],
}

function keyFor(kind: IntegrationKind, target: string, n: number) {
  if (kind === 'github' || kind === 'gitlab') return { key: `#${n}`, id: `${target}#${n}` }
  if (kind === 'linear') return { key: `ENG-${n}`, id: `ENG-${n}` }
  return { key: `${target}-${n}`, id: `${target}-${n}` }
}

function urlFor(i: DbIntegration, target: string, key: string) {
  const n = key.replace(/^.*?(\d+)$/, '$1')
  if (i.kind === 'github') return `https://github.com/${target}/issues/${n}`
  if (i.kind === 'gitlab') return `${i.config.base_url ?? 'https://gitlab.com'}/${target}/-/issues/${n}`
  if (i.kind === 'linear') return `https://linear.app/acme/issue/${key}`
  return `${i.config.base_url ?? 'https://acme.atlassian.net'}/browse/${key}`
}

function seed(world: World): State {
  const acme = world.orgs.find((o) => o.slug === 'acme')!
  const now = world.now
  const integrations: DbIntegration[] = [
    {
      id: '9001',
      org_id: acme.id,
      kind: 'jira_cloud',
      name: 'Jira',
      enabled: true,
      config: { base_url: 'https://acme.atlassian.net', email: 'rupta-bot@acme.dev' },
      token: 'ATATT3xFfGF0abcd',
      webhook_secret: 'whsec',
      account: { id: '5f1c', name: 'rupta bot' },
      created_by: '1',
      created_at: now - 40 * 86_400_000,
      updated_at: now - 3 * 86_400_000,
      last_error: null,
      last_error_at: null,
    },
    {
      id: '9002',
      org_id: acme.id,
      kind: 'github',
      name: 'GitHub',
      enabled: true,
      config: {},
      token: 'github_pat_11ABCwxyz',
      webhook_secret: 'whsec',
      account: { id: '1042', name: 'acme-bot' },
      created_by: '1',
      created_at: now - 20 * 86_400_000,
      updated_at: now - 86_400_000,
      last_error: 'POST /repos/acme/api/issues/88/comments: 403 Forbidden — Resource not accessible by personal access token',
      last_error_at: now - 2 * 3_600_000,
    },
    {
      id: '9003',
      org_id: acme.id,
      kind: 'linear',
      name: 'Linear',
      enabled: false,
      config: {},
      token: 'lin_api_9fQx',
      webhook_secret: null,
      account: { id: 'u1', name: 'Jane Cooper' },
      created_by: '1',
      created_at: now - 9 * 86_400_000,
      updated_at: now - 9 * 86_400_000,
      last_error: null,
      last_error_at: null,
    },
  ]
  const pid = (slug: string) => world.projects.find((x) => x.slug === slug && x.org_id === acme.id)?.id ?? ''
  const defaults: DbDefaults[] = [
    {
      project_id: pid('web-app'),
      integration_id: '9001',
      target: 'WEB',
      target_name: 'Web storefront',
      issue_type: 'Bug',
      project: null,
      labels: ['rupta'],
      incoming_sync: true,
      outgoing_transition: true,
      outgoing_comment: true,
      updated_at: now - 5 * 86_400_000,
    },
    {
      project_id: pid('api'),
      integration_id: '9002',
      target: 'acme/api',
      target_name: 'acme/api',
      issue_type: null,
      project: null,
      labels: ['bug'],
      incoming_sync: true,
      outgoing_transition: true,
      outgoing_comment: false,
      updated_at: now - 2 * 86_400_000,
    },
  ]
  // A few of the busiest issues already have tickets.
  const top = [...world.issues]
    .filter((i) => i.status === 'unresolved')
    .sort((a, b) => b.count - a.count)
    .slice(0, 12)
  const links: DbLink[] = []
  top.forEach((issue, n) => {
    const rng = new Rng(`link:${issue.id}`)
    const proj = world.projects.find((x) => x.id === issue.project_id)
    const github = proj?.slug === 'api'
    const integ = github ? integrations[1] : integrations[0]
    const target = github ? 'acme/api' : proj?.slug === 'billing' ? 'PAY' : 'WEB'
    const k = keyFor(integ.kind, target, 40 + n * 7)
    const done = n % 5 === 3
    links.push({
      id: String(9100 + n),
      issue_id: issue.id,
      integration_id: integ.id,
      external_id: k.id,
      external_key: k.key,
      url: urlFor(integ, target, k.key),
      title: `[${issue.short_id}] ${issue.title}`.slice(0, 120),
      status: github ? (done ? 'closed' : 'open') : done ? 'Done' : rng.pick(['To Do', 'In Progress', 'In Review']),
      status_category: done ? 'done' : 'open',
      assignee: rng.chance(0.6) ? rng.pick(['Mei Chen', 'Jonas Weber', 'Ines Duarte']) : null,
      created_by: '1',
      created_at: now - (n + 1) * 7_200_000,
      synced_at: now - rng.int(2, 90) * 60_000,
    })
  })
  // Ticket history on the busiest linked issue, for the activity tab.
  const first = links[0]
  if (first) {
    first.status = first.integration_id === '9002' ? 'open' : 'In Progress'
    first.status_category = 'open'
    const acts = world.activity.get(first.issue_id) ?? []
    const u = world.users[0]
    const actor = u ? { id: u.id, email: u.email, name: u.name, avatar_url: u.avatar_url, is_superuser: u.is_superuser } : null
    const integ = integrations.find((i) => i.id === first.integration_id)
    // The server's shape (store::ticket_json).
    const t = { integration_id: first.integration_id, integration: integ?.name, kind: integ?.kind, key: first.external_key, url: first.url }
    const add = (kind: string, user: typeof actor, data: Record<string, unknown>, at: number) =>
      acts.push({ id: `t${at}`, kind, user, data, created_at: iso(at) } as never)
    add('ticket_linked', actor, { ...t, mode: 'create' }, first.created_at)
    add('ticket_status_change', null, { ...t, status: 'In Progress', status_category: 'open' }, first.created_at + 1_200_000)
    add(
      'ticket_sync',
      null,
      { ...t, ok: false, error: 'the tracker answered 400: Field "resolution" is required for the transition Done' },
      first.created_at + 2_400_000,
    )
    add('ticket_sync', null, { ...t, ok: true, action: 'comment' }, first.created_at + 3_000_000)
    acts.sort((a, b) => Date.parse(a.created_at) - Date.parse(b.created_at))
    world.activity.set(first.issue_id, acts)
  }
  return { world, integrations, defaults, links }
}

/** Seeds the integration data (and its ticket activity) for this world. */
export function seedIntegrations(world: World) {
  st(world)
}

function st(world: World): State {
  if (!state || state.world !== world) state = seed(world)
  return state
}

function out(i: DbIntegration, secret?: string | null) {
  return {
    id: i.id,
    kind: i.kind,
    name: i.name,
    enabled: i.enabled,
    config: i.config,
    token_hint: `****${i.token.slice(-4)}`,
    has_webhook_secret: Boolean(i.webhook_secret),
    webhook_url: `https://rupta.acme.dev/api/integrations/${i.kind}/${i.id}/webhook`,
    account: i.account,
    created_by: i.created_by,
    created_at: iso(i.created_at),
    updated_at: iso(i.updated_at),
    last_error: i.last_error,
    last_error_at: i.last_error_at ? iso(i.last_error_at) : null,
    ...(secret !== undefined ? { webhook_secret: secret } : {}),
  }
}

const ref = (i: DbIntegration) => ({ id: i.id, kind: i.kind, name: i.name, enabled: i.enabled })

function linkOut(s: State, l: DbLink) {
  const i = s.integrations.find((x) => x.id === l.integration_id)!
  return { ...l, integration: ref(i), created_at: iso(l.created_at), synced_at: l.synced_at ? iso(l.synced_at) : null, integration_id: undefined }
}

function newSecret(seedText: string) {
  return `whsec_${new Rng(`secret:${seedText}:${Date.now()}`).hex(32)}`
}

const REQUIRED: Record<IntegrationKind, string[]> = { jira_cloud: ['base_url', 'email'], jira_server: ['base_url'], linear: [], github: [], gitlab: [] }

export function integrationHandlers(ctx: IntegrationsContext) {
  const addActivity = (issueId: string, kind: string, data: Record<string, unknown>) => {
    const w = ctx.world()
    const acts = w.activity.get(issueId) ?? []
    const a = ctx.actor()
    acts.push({ id: ctx.nid(), kind, user: a, data, created_at: iso(Date.now()) } as never)
    w.activity.set(issueId, acts)
  }
  const orgOf = (params: Record<string, unknown>) => {
    const r = ctx.withOrg(params)
    if ('error' in r) return r
    return { org: r.org, s: st(ctx.world()) }
  }
  const projectOf = (params: Record<string, unknown>) => {
    const r = orgOf(params)
    if ('error' in r) return r
    const project = r.s.world.projects.find((x) => x.org_id === r.org.id && (x.slug === p(params, 'project') || x.id === p(params, 'project')))
    if (!project) return { error: err(404, 'not_found', 'Project not found.') }
    return { ...r, project }
  }
  const issueOf = (id: string) => {
    const s = st(ctx.world())
    const issue = s.world.issues.find((i) => i.id === id)
    return issue ? { s, issue } : null
  }

  return [
    http.get(`${BASE}/orgs/:org/integrations`, async ({ params }) => {
      await delay(60)
      const r = orgOf(params)
      if ('error' in r) return r.error
      return json(r.s.integrations.filter((i) => i.org_id === r.org.id).map((i) => out(i)))
    }),
    http.post(`${BASE}/orgs/:org/integrations`, async ({ params, request }) => {
      await delay(250)
      const r = orgOf(params)
      if ('error' in r) return r.error
      if (!ctx.isAdmin(r.org.id)) return err(403, 'forbidden', 'Only admins and owners manage integrations.')
      const body = (await request.json()) as {
        kind: IntegrationKind
        name: string
        enabled?: boolean
        config?: Record<string, string>
        token?: string
        webhook_secret?: string
        validate?: boolean
      }
      if (!REQUIRED[body.kind]) return err(422, 'validation_failed', 'kind: must be one of jira_cloud, jira_server, linear, github, gitlab', { field: 'kind' })
      for (const f of REQUIRED[body.kind])
        if (!body.config?.[f]) return err(422, 'validation_failed', `config.${f}: required for ${body.kind}`, { field: `config.${f}` })
      if (!body.token) return err(422, 'validation_failed', 'token: required', { field: 'token' })
      if (body.validate !== false && /bad|invalid/i.test(body.token))
        return err(
          422,
          'validation_failed',
          `token: ${body.kind === 'github' ? 'GitHub' : body.kind === 'linear' ? 'Linear' : body.kind === 'gitlab' ? 'GitLab' : 'Jira'} refused the token (401 Unauthorized)`,
          { field: 'token' },
        )
      const now = Date.now()
      const generated = body.kind === 'linear' ? null : (body.webhook_secret ?? newSecret(body.name))
      const i: DbIntegration = {
        id: ctx.nid(),
        org_id: r.org.id,
        kind: body.kind,
        name: body.name,
        enabled: body.enabled ?? true,
        config: body.config ?? {},
        token: body.token,
        webhook_secret: body.kind === 'linear' ? (body.webhook_secret ?? null) : generated,
        account: body.validate === false ? null : { id: 'acct-1', name: body.config?.email ?? (body.kind === 'github' ? 'acme-bot' : 'Jane Cooper') },
        created_by: ctx.actor()?.id ?? null,
        created_at: now,
        updated_at: now,
        last_error: null,
        last_error_at: null,
      }
      r.s.integrations.push(i)
      return json(out(i, body.webhook_secret ? null : generated), 201)
    }),
    http.get(`${BASE}/orgs/:org/integrations/:id`, ({ params }) => {
      const r = orgOf(params)
      if ('error' in r) return r.error
      const i = r.s.integrations.find((x) => x.id === p(params, 'id') && x.org_id === r.org.id)
      return i ? json(out(i)) : err(404, 'not_found', 'Integration not found.')
    }),
    http.patch(`${BASE}/orgs/:org/integrations/:id`, async ({ params, request }) => {
      await delay(150)
      const r = orgOf(params)
      if ('error' in r) return r.error
      if (!ctx.isAdmin(r.org.id)) return err(403, 'forbidden', 'Only admins and owners manage integrations.')
      const i = r.s.integrations.find((x) => x.id === p(params, 'id') && x.org_id === r.org.id)
      if (!i) return err(404, 'not_found', 'Integration not found.')
      const body = (await request.json()) as {
        name?: string
        enabled?: boolean
        config?: Record<string, string>
        token?: string
        webhook_secret?: string
        rotate_webhook_secret?: boolean
        validate?: boolean
      }
      if (body.token && body.validate !== false && /bad|invalid/i.test(body.token))
        return err(422, 'validation_failed', 'token: the tracker refused the token (401 Unauthorized)', { field: 'token' })
      if (body.name !== undefined) i.name = body.name
      if (body.enabled !== undefined) i.enabled = body.enabled
      if (body.config) i.config = body.config
      if (body.token) i.token = body.token
      if (body.webhook_secret) i.webhook_secret = body.webhook_secret
      let secret: string | null | undefined
      if (body.rotate_webhook_secret) {
        secret = newSecret(i.name)
        i.webhook_secret = secret
      }
      i.updated_at = Date.now()
      return json(out(i, secret))
    }),
    http.delete(`${BASE}/orgs/:org/integrations/:id`, ({ params }) => {
      const r = orgOf(params)
      if ('error' in r) return r.error
      const id = p(params, 'id')
      r.s.integrations = r.s.integrations.filter((x) => x.id !== id)
      r.s.links = r.s.links.filter((l) => l.integration_id !== id)
      r.s.defaults = r.s.defaults.filter((d) => d.integration_id !== id)
      return new HttpResponse(null, { status: 204 })
    }),
    http.post(`${BASE}/orgs/:org/integrations/:id/test`, async ({ params }) => {
      await delay(400)
      const r = orgOf(params)
      if ('error' in r) return r.error
      const i = r.s.integrations.find((x) => x.id === p(params, 'id'))
      if (!i) return err(404, 'not_found', 'Integration not found.')
      if (i.kind === 'github') return json({ ok: false, account: i.account, error: 'The token cannot comment on acme/api issues.', reason: 'forbidden' })
      if (/fail/.test(i.config.base_url ?? '')) return json({ ok: false, account: null, error: 'connection refused', reason: 'unreachable' })
      i.last_error = null
      return json({ ok: true, account: i.account ?? { id: 'acct-1', name: 'rupta bot' }, error: null, reason: null })
    }),
    http.get(`${BASE}/orgs/:org/integrations/:id/targets`, async ({ params }) => {
      await delay(200)
      const r = orgOf(params)
      if ('error' in r) return r.error
      const i = r.s.integrations.find((x) => x.id === p(params, 'id'))
      if (!i) return err(404, 'not_found', 'Integration not found.')
      if (/fail/.test(i.config.base_url ?? '')) return err(502, 'integration_error', 'Jira did not answer.', { reason: 'unreachable' })
      return json(TARGETS[i.kind])
    }),

    // ---- project defaults ----------------------------------------------------
    http.get(`${BASE}/projects/:org/:project/integrations`, async ({ params }) => {
      await delay(60)
      const r = projectOf(params)
      if ('error' in r) return r.error
      return json(
        r.s.defaults
          .filter((d) => d.project_id === r.project.id)
          .map((d) => ({
            ...d,
            integration: ref(r.s.integrations.find((x) => x.id === d.integration_id)!),
            updated_at: iso(d.updated_at),
            project_id: undefined,
            integration_id: undefined,
          })),
      )
    }),
    http.put(`${BASE}/projects/:org/:project/integrations/:integrationId`, async ({ params, request }) => {
      const r = projectOf(params)
      if ('error' in r) return r.error
      const i = r.s.integrations.find((x) => x.id === p(params, 'integrationId'))
      if (!i) return err(404, 'not_found', 'Integration not found.')
      const body = (await request.json()) as Partial<DbDefaults>
      if (!body.target) return err(422, 'validation_failed', 'target: required', { field: 'target' })
      const d: DbDefaults = {
        project_id: r.project.id,
        integration_id: i.id,
        target: body.target,
        target_name: body.target_name ?? null,
        issue_type: body.issue_type ?? null,
        project: body.project ?? null,
        labels: body.labels ?? [],
        incoming_sync: body.incoming_sync ?? true,
        outgoing_transition: body.outgoing_transition ?? true,
        outgoing_comment: body.outgoing_comment ?? true,
        updated_at: Date.now(),
      }
      r.s.defaults = [...r.s.defaults.filter((x) => !(x.project_id === r.project.id && x.integration_id === i.id)), d]
      return json({ ...d, integration: ref(i), updated_at: iso(d.updated_at) })
    }),
    http.delete(`${BASE}/projects/:org/:project/integrations/:integrationId`, ({ params }) => {
      const r = projectOf(params)
      if ('error' in r) return r.error
      r.s.defaults = r.s.defaults.filter((x) => !(x.project_id === r.project.id && x.integration_id === p(params, 'integrationId')))
      return new HttpResponse(null, { status: 204 })
    }),

    // ---- issue links -----------------------------------------------------------
    http.get(`${BASE}/issues/:id/links`, async ({ params }) => {
      await delay(60)
      const r = issueOf(p(params, 'id'))
      if (!r) return err(404, 'not_found', 'Issue not found.')
      return json(r.s.links.filter((l) => l.issue_id === r.issue.id).map((l) => linkOut(r.s, l)))
    }),
    http.get(`${BASE}/issues/:id/ticket-draft`, ({ params }) => {
      const r = issueOf(p(params, 'id'))
      if (!r) return err(404, 'not_found', 'Issue not found.')
      const i = r.issue
      const proj = r.s.world.projects.find((x) => x.id === i.project_id)
      return json({
        title: `[${i.short_id}] ${i.title}`.slice(0, 255),
        description: [
          `**[${i.short_id}](https://rupta.acme.dev/organizations/acme/issues/${i.id})** in ${proj?.slug ?? 'project'}`,
          '',
          `**${i.title}**`,
          '',
          `- Culprit: \`${i.culprit}\``,
          `- Level: ${i.level}`,
          `- Events: ${i.count.toLocaleString('en-US')}, users: ${i.user_count.toLocaleString('en-US')}`,
          `- First seen: ${iso(i.first_seen)}, last seen: ${iso(i.last_seen)}`,
          i.first_release ? `- First release: ${i.first_release}` : '',
          '',
          '**Top tags**',
          '',
          ...Object.entries(i.tags)
            .slice(0, 5)
            .map(([k, v]) => `- ${k}: ${v}`),
          '',
          '**Stack trace (in-app frames)**',
          '',
          '```',
          `${i.culprit}`,
          '```',
        ]
          .filter((l, n, a) => !(l === '' && a[n - 1] === ''))
          .join('\n'),
      })
    }),
    http.post(`${BASE}/issues/:id/links`, async ({ params, request }) => {
      await delay(300)
      const r = issueOf(p(params, 'id'))
      if (!r) return err(404, 'not_found', 'Issue not found.')
      const body = (await request.json()) as { integration_id: string; mode?: 'create' | 'link'; reference?: string; target?: string; title?: string }
      const integ = r.s.integrations.find((x) => x.id === body.integration_id)
      if (!integ) return err(404, 'not_found', 'Integration not found.')
      if (!integ.enabled) return err(422, 'validation_failed', 'integration_id: the integration is disabled')
      const d = r.s.defaults.find((x) => x.project_id === r.issue.project_id && x.integration_id === integ.id)
      let key: string
      let id: string
      let target: string
      let title: string | null
      if (body.mode === 'link') {
        const ref = (body.reference ?? '').trim()
        const m = /([A-Z][A-Z0-9]+-\d+)|#(\d+)|\/issues\/(\d+)|browse\/([A-Z][A-Z0-9]+-\d+)/.exec(ref)
        if (!m) return err(422, 'validation_failed', `reference: not a ticket of ${integ.name}`, { field: 'reference' })
        if (/^OPS-404$/.test(ref)) return err(502, 'integration_error', `${integ.name} answered 404 Not Found for OPS-404`, { reason: 'not_found' })
        target = d?.target ?? 'OPS'
        key = m[1] ?? m[4] ?? `#${m[2] ?? m[3]}`
        id = integ.kind === 'github' || integ.kind === 'gitlab' ? `${target}${key}` : key
        title = `Existing ticket ${key}`
      } else {
        target = body.target ?? d?.target ?? ''
        if (!target) return err(422, 'validation_failed', 'target: the project has no default target for this integration; choose one')
        const k = keyFor(integ.kind, target, 100 + r.s.links.length)
        key = k.key
        id = k.id
        title = body.title ?? `[${r.issue.short_id}] ${r.issue.title}`
      }
      if (r.s.links.some((l) => l.issue_id === r.issue.id && l.integration_id === integ.id && l.external_id === id))
        return err(409, 'conflict', `${key} is already linked to this issue.`)
      const link: DbLink = {
        id: ctx.nid(),
        issue_id: r.issue.id,
        integration_id: integ.id,
        external_id: id,
        external_key: key,
        url: urlFor(integ, target, key),
        title,
        status: integ.kind === 'github' || integ.kind === 'gitlab' ? 'open' : 'To Do',
        status_category: 'open',
        assignee: null,
        created_by: ctx.actor()?.id ?? null,
        created_at: Date.now(),
        synced_at: Date.now(),
      }
      r.s.links.push(link)
      addActivity(r.issue.id, 'ticket_linked', {
        integration_id: integ.id,
        integration: integ.name,
        kind: integ.kind,
        key,
        url: link.url,
        mode: body.mode ?? 'create',
      })
      return json(linkOut(r.s, link), 201)
    }),
    http.post(`${BASE}/issues/:id/links/:linkId/refresh`, async ({ params }) => {
      await delay(300)
      const r = issueOf(p(params, 'id'))
      if (!r) return err(404, 'not_found', 'Issue not found.')
      const l = r.s.links.find((x) => x.id === p(params, 'linkId'))
      if (!l) return err(404, 'not_found', 'Link not found.')
      l.synced_at = Date.now()
      return json(linkOut(r.s, l))
    }),
    http.delete(`${BASE}/issues/:id/links/:linkId`, ({ params }) => {
      const r = issueOf(p(params, 'id'))
      if (!r) return err(404, 'not_found', 'Issue not found.')
      const l = r.s.links.find((x) => x.id === p(params, 'linkId'))
      r.s.links = r.s.links.filter((x) => x.id !== p(params, 'linkId'))
      const integ = l && r.s.integrations.find((x) => x.id === l.integration_id)
      if (l)
        addActivity(r.issue.id, 'ticket_unlinked', {
          integration_id: l.integration_id,
          integration: integ?.name,
          kind: integ?.kind,
          key: l.external_key,
          url: l.url,
        })
      return new HttpResponse(null, { status: 204 })
    }),
  ]
}
