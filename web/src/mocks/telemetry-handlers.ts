// Mock implementation of the performance, traces, logs, metrics,
// dashboards, profiles and replays endpoints (see the API section of the
// telemetry brief), on top of the issue world in handlers.ts.

import { http, HttpResponse, type JsonBodyType } from 'msw'
import type { Dashboard, LogRow } from '@/api/telemetry-types'
import { matchesLog, parseLogQuery } from '@/lib/logs/query'
import { tokenize } from '@/lib/search/parse'
import { replayIdAt } from './fixtures/ids'
import {
  buildTrace,
  createTelemetry,
  flamegraphOf,
  liveLog,
  metricSeries,
  METRICS,
  periodMs,
  recordingFor,
  replayErrors,
  RECORDING_MS,
  sampledProfile,
  summary,
  traceLogs,
  transactionRow,
  TX,
  type MockLog,
  type MockReplay,
  type Telemetry,
} from './fixtures/telemetry'
import type { DbOrg, World } from './fixtures/world'

export interface MockContext {
  world: () => World
  wait: () => Promise<void>
  withOrg: (params: Record<string, unknown>) => { error: Response } | { org: DbOrg }
  canWrite: (orgId: string) => boolean
}

const BASE = '/api/v1'
const json = (body: unknown, status = 200) => HttpResponse.json(body as JsonBodyType, { status })
const err = (status: number, code: string, message: string) => HttpResponse.json({ error: { code, message, details: {} } }, { status })
const p = (params: Record<string, unknown>, k: string) => String(params[k] ?? '')

function list(url: URL, key: string): string[] {
  return url.searchParams
    .getAll(key)
    .flatMap((v) => v.split(','))
    .map((v) => v.trim())
    .filter(Boolean)
}

let tel: Telemetry | null = null
let telWorld: World | null = null

function telemetry(world: World): Telemetry {
  if (!tel || telWorld !== world) {
    tel = createTelemetry(world)
    telWorld = world
  }
  return tel
}

/** Org projects (optionally limited by `project=`). */
function projectSet(world: World, org: DbOrg, url: URL): Set<string> {
  const wanted = list(url, 'project')
  return new Set(world.projects.filter((x) => x.org_id === org.id && (!wanted.length || wanted.includes(x.id))).map((x) => x.id))
}

function timeRange(t: Telemetry, url: URL): [number, number] {
  const start = url.searchParams.get('start')
  const end = url.searchParams.get('end')
  if (start) return [Date.parse(start), end ? Date.parse(end) : Date.now()]
  const now = Math.max(t.now, Date.now())
  return [now - periodMs(url.searchParams.get('period') ?? '14d'), now + 60_000]
}

function logOut(l: MockLog): Record<string, unknown> {
  // Typed attributes on the wire, as the server's storage keeps them plain:
  // the API returns plain values.
  return { ...l, timestamp: new Date(l.timestamp).toISOString(), severity_number: { trace: 1, debug: 5, info: 9, warn: 13, error: 17, fatal: 21 }[l.level] } 
}

function filterLogs(t: Telemetry, world: World, org: DbOrg, url: URL): MockLog[] {
  const projects = projectSet(world, org, url)
  const envs = list(url, 'environment')
  const [from, to] = timeRange(t, url)
  const q = parseLogQuery(url.searchParams.get('query') ?? '')
  let base = t.logs
  if (q.trace) {
    // Logs of a trace: the stored ones plus the lines emitted by its spans.
    const trace = buildTrace(t, world, q.trace)
    base = [...t.logs.filter((l) => l.trace_id === q.trace), ...traceLogs(t, flatSpans(trace.spans), trace.errors.length > 0)].sort((a, b) => b.timestamp - a.timestamp)
    return base.filter((l) => matchesLog(l as unknown as LogRow, q))
  }
  return base.filter(
    (l) => projects.has(l.project_id) && (!envs.length || envs.includes(l.environment)) && l.timestamp >= from && l.timestamp <= to && matchesLog(l as unknown as LogRow, q),
  )
}

/** The trace's nested spans, flat again with epoch-millisecond starts. */
function flatSpans(nodes: { start_timestamp: string; children: unknown[] }[]): Parameters<typeof traceLogs>[1] {
  const out: Parameters<typeof traceLogs>[1] = []
  const visit = (n: { start_timestamp: string; children: unknown[] }) => {
    out.push({ ...(n as unknown as Parameters<typeof traceLogs>[1][number]), start_timestamp: Date.parse(n.start_timestamp) })
    for (const c of n.children) visit(c as typeof n)
  }
  nodes.forEach(visit)
  return out
}

function niceInterval(ms: number, target = 60): number {
  const steps = [60_000, 5 * 60_000, 15 * 60_000, 30 * 60_000, 3_600_000, 3 * 3_600_000, 6 * 3_600_000, 12 * 3_600_000, 86_400_000]
  return steps.find((s) => ms / s <= target) ?? 86_400_000
}

function replayOut(world: World, r: MockReplay) {
  const errors = replayErrors(world, r)
  return {
    id: r.replay_id,
    replay_id: r.replay_id,
    project_id: r.project_id,
    started: new Date(r.started).toISOString(),
    finished: new Date(r.started + r.duration_ms).toISOString(),
    duration_ms: r.duration_ms,
    segment_count: Math.max(1, Math.round(r.duration_ms / 5000)),
    urls: r.urls,
    error_ids: errors.map((e) => e.event_id),
    trace_ids: [],
    user: { id: r.user.id, email: r.user.email, name: r.user.name, ip_address: r.user.ip },
    browser: r.browser,
    os: r.os,
    device: r.device,
    environment: r.environment,
    release: r.release,
    count_errors: errors.length,
    count_clicks: r.count_clicks,
  }
}

function sortBy<T>(rows: T[], key: keyof T, desc = true) {
  return [...rows].sort((a, b) => ((a[key] as number) - (b[key] as number)) * (desc ? -1 : 1))
}

function pageOf<T>(rows: T[], url: URL, limit = 50) {
  const offset = Number(url.searchParams.get('cursor') ?? 0) || 0
  const n = Math.min(1000, Number(url.searchParams.get('limit') ?? limit) || limit)
  const data = rows.slice(offset, offset + n)
  return { data, next_cursor: offset + n < rows.length ? String(offset + n) : null }
}

export function telemetryHandlers(ctx: MockContext) {
  const withTel = (params: Record<string, unknown>) => {
    const r = ctx.withOrg(params)
    if ('error' in r) return r
    const world = ctx.world()
    return { org: r.org, world, t: telemetry(world) }
  }

  return [
    // ---- performance ------------------------------------------------------
    http.get(`${BASE}/orgs/:org/performance/transactions`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const url = new URL(request.url)
      const projects = projectSet(r.world, r.org, url)
      const period = url.searchParams.get('period') ?? '14d'
      const envFactor = list(url, 'environment').length ? (list(url, 'environment').includes('production') ? 0.92 : 0.08) : 1
      const text = (url.searchParams.get('query') ?? '').toLowerCase()
      const tokens = tokenize(text)
      const opFilter = tokens.find((x) => x.type === 'filter' && x.key === 'op')
      const words = tokens.filter((x) => x.type === 'text').map((x) => (x.type === 'text' ? x.value : ''))
      let rows = TX.filter((d) => projects.has(r.t.projectIds.get(d.project) ?? ''))
        .filter((d) => words.every((w) => d.name.toLowerCase().includes(w)))
        .filter((d) => !opFilter || (opFilter.type === 'filter' && opFilter.values.some((v) => d.op.startsWith(v))))
        .map((d) => transactionRow(d, r.t.projectIds.get(d.project)!, period, envFactor))
      const sort = (url.searchParams.get('sort') ?? 'tpm') as 'tpm'
      rows = rows.length ? sortBy(rows, sort in rows[0] ? sort : 'tpm') : rows
      return json({ data: rows, next_cursor: null })
    }),
    http.get(`${BASE}/orgs/:org/performance/transactions/summary`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const url = new URL(request.url)
      const name = url.searchParams.get('name') ?? ''
      const projects = list(url, 'project')
      const def = TX.find((d) => d.name === name && (!projects.length || projects.includes(r.t.projectIds.get(d.project) ?? '')))
      if (!def) return err(404, 'not_found', 'No transactions with this name in the period.')
      return json(summary(r.t, r.world, def, url.searchParams.get('period') ?? '14d'))
    }),
    http.get(`${BASE}/orgs/:org/traces/:traceId`, async ({ params }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const id = p(params, 'traceId').toLowerCase().replace(/-/g, '')
      if (!/^[0-9a-f]{32}$/.test(id)) return err(400, 'invalid', 'trace_id: expected 32 hexadecimal characters.')
      if (id === '0'.repeat(32)) return err(404, 'not_found', 'No spans or transactions with this trace id. It may have been deleted by the retention policy or not sampled.')
      return json(buildTrace(r.t, r.world, id))
    }),
    http.get(`${BASE}/orgs/:org/spans`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const url = new URL(request.url)
      const groupBy = (['description', 'transaction', 'none'].includes(url.searchParams.get('group_by') ?? '') ? url.searchParams.get('group_by') : 'op') as 'op' | 'description' | 'transaction' | 'none'
      const period = url.searchParams.get('period') ?? '14d'
      const projects = projectSet(r.world, r.org, url)
      const minutes = periodMs(period) / 60_000
      // Aggregate a sample of traces per transaction, scaled to the period.
      const groups = new Map<string, { group: string | null; durations: number[]; weight: number }>()
      const tokens = tokenize(url.searchParams.get('query') ?? '')
      const matchSpan = (s: { op: string; description: string; transaction: string | null; duration_ms: number }) =>
        tokens.every((tok) => {
          if (tok.type === 'text') return s.description.toLowerCase().includes(tok.value.toLowerCase())
          if (tok.key === 'op') return tok.values.some((v) => s.op === v || s.op.startsWith(`${v}.`))
          if (tok.key === 'description') return tok.values.some((v) => s.description.toLowerCase().includes(v.toLowerCase()))
          if (tok.key === 'duration') return tok.op === '>' || tok.op === '>=' ? s.duration_ms >= Number(tok.values[0]) : s.duration_ms <= Number(tok.values[0])
          return true
        })
      const samples: Record<string, unknown>[] = []
      for (const def of TX) {
        if (!projects.has(r.t.projectIds.get(def.project) ?? '') && !def.browser) continue
        for (let i = 0; i < 3; i++) {
          const trace = buildTrace(r.t, r.world, `${(i + 1).toString(16).padStart(2, '0')}${def.name.length.toString(16).padStart(2, '0')}${'ab'.repeat(14)}`.slice(0, 32))
          for (const s of flatSpans(trace.spans)) {
            if (!projects.has(s.project_id) || !matchSpan(s)) continue
            if (groupBy === 'none') {
              if (samples.length < 100) samples.push({ span_id: s.span_id, trace_id: s.trace_id, project_id: s.project_id, op: s.op, description: s.description, transaction: s.transaction ?? def.name, status: s.status, duration_ms: s.duration_ms, timestamp: new Date(s.start_timestamp).toISOString() })
              continue
            }
            const key = groupBy === 'op' ? s.op : groupBy === 'transaction' ? def.name : s.description
            const g = groups.get(key) ?? { group: key, durations: [], weight: 0 }
            g.durations.push(s.duration_ms)
            g.weight += def.tpm / 3
            groups.set(key, g)
          }
        }
      }
      if (groupBy === 'none') return json({ group_by: 'none', groups: [], spans: samples })
      const rows = [...groups.values()]
        .map((g) => {
          const d = [...g.durations].sort((a, b) => a - b)
          const count = Math.round(g.weight * minutes)
          const avg = d.reduce((s, x) => s + x, 0) / d.length
          return { group: g.group, count, p50: Math.round(d[Math.floor(d.length * 0.5)] * 10) / 10, p95: Math.round(d[Math.min(d.length - 1, Math.floor(d.length * 0.95))] * 10) / 10, avg: Math.round(avg * 10) / 10, sum_duration_ms: Math.round(avg * count) }
        })
        .sort((a, b) => b.sum_duration_ms - a.sum_duration_ms)
        .slice(0, 100)
      return json({ group_by: groupBy, groups: rows, spans: [] })
    }),

    // ---- logs ----------------------------------------------------------------
    http.get(`${BASE}/orgs/:org/logs`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const url = new URL(request.url)
      let rows = filterLogs(r.t, r.world, r.org, url)
      if (url.searchParams.get('sort') === 'timestamp') rows = [...rows].reverse()
      const pg = pageOf(rows, url, 100)
      return json({ data: pg.data.map(logOut), next_cursor: pg.next_cursor })
    }),
    http.get(`${BASE}/orgs/:org/logs/histogram`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const url = new URL(request.url)
      const rows = filterLogs(r.t, r.world, r.org, url)
      const [from, to] = timeRange(r.t, url)
      const interval = niceInterval(to - from)
      const start = Math.floor(from / interval) * interval
      const buckets = new Map<number, Record<string, number>>()
      for (let ts = start; ts <= to; ts += interval) buckets.set(ts, {})
      for (const l of rows) {
        const ts = Math.floor(l.timestamp / interval) * interval
        const b = buckets.get(ts) ?? {}
        b[l.level] = (b[l.level] ?? 0) + 1
        buckets.set(ts, b)
      }
      const totals: Record<string, number> = {}
      for (const l of rows) totals[l.level] = (totals[l.level] ?? 0) + 1
      return json({
        interval: interval / 1000,
        buckets: [...buckets.entries()].sort((a, b) => a[0] - b[0]).map(([ts, counts]) => ({ ts: Math.floor(ts / 1000), total: Object.values(counts).reduce((s, n) => s + n, 0), counts })),
        totals,
      })
    }),
    http.get(`${BASE}/orgs/:org/logs/stream`, async ({ params, request }) => {
      const r = withTel(params)
      if ('error' in r) return r.error
      const url = new URL(request.url)
      const q = parseLogQuery(url.searchParams.get('query') ?? '')
      const projects = projectSet(r.world, r.org, url)
      const enc = new TextEncoder()
      let seq = 0
      let timer: ReturnType<typeof setInterval> | undefined
      const stream = new ReadableStream({
        start(controller) {
          controller.enqueue(enc.encode('retry: 2000\nevent: ready\ndata: {}\n\n'))
          timer = setInterval(() => {
            const n = 1 + (seq % 4)
            const rows: MockLog[] = []
            for (let i = 0; i < n; i++) {
              const l = liveLog(r.t, seq++, Date.now() - (n - i) * 40)
              if (projects.has(l.project_id) && matchesLog(l as unknown as LogRow, q)) rows.push(l)
            }
            try {
              if (rows.length) controller.enqueue(enc.encode(`event: logs\ndata: ${JSON.stringify(rows.map(logOut))}\n\n`))
              else controller.enqueue(enc.encode(': keep-alive\n\n'))
            } catch {
              clearInterval(timer)
            }
          }, 1000)
          request.signal.addEventListener('abort', () => {
            clearInterval(timer)
            try {
              controller.close()
            } catch {
              /* already closed */
            }
          })
        },
        cancel() {
          clearInterval(timer)
        },
      })
      return new HttpResponse(stream, { headers: { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' } })
    }),

    // ---- metrics -------------------------------------------------------------
    http.get(`${BASE}/orgs/:org/metrics/names`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const url = new URL(request.url)
      const projects = projectSet(r.world, r.org, url)
      return json({
        data: METRICS.filter((m) => projects.has(r.t.projectIds.get(m.project) ?? '')).map((m) => ({
          name: m.name,
          type: m.type,
          unit: m.unit,
          last_seen: new Date(r.t.now - 60_000).toISOString(),
          // beyond the brief: attribute keys, for group-by suggestions
          attributes: Object.keys(m.attributes),
        })),
      })
    }),
    http.get(`${BASE}/orgs/:org/metrics/query`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const url = new URL(request.url)
      const def = METRICS.find((m) => m.name === url.searchParams.get('name'))
      if (!def) return err(404, 'not_found', 'Unknown metric.')
      const op = url.searchParams.get('op') ?? (def.type === 'counter' ? 'sum' : 'avg')
      return json(metricSeries(r.t, def, op, url.searchParams.get('group_by') || null, url.searchParams.get('period') ?? '14d', url.searchParams.get('interval')))
    }),

    // ---- dashboards ----------------------------------------------------------
    http.get(`${BASE}/orgs/:org/dashboards`, async ({ params }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      return json({ data: r.t.dashboards })
    }),
    http.get(`${BASE}/orgs/:org/dashboards/:id`, async ({ params }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const d = r.t.dashboards.find((x) => x.id === p(params, 'id'))
      return d ? json(d) : err(404, 'not_found', 'Dashboard not found.')
    }),
    http.post(`${BASE}/orgs/:org/dashboards`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      if (!ctx.canWrite(r.org.id)) return err(403, 'forbidden', 'Viewers cannot change dashboards.')
      const body = (await request.json()) as Partial<Dashboard>
      if (!body.title?.trim()) return HttpResponse.json({ error: { code: 'validation_failed', message: 'title: must not be empty', details: { field: 'title' } } }, { status: 422 })
      const now = new Date().toISOString()
      const d: Dashboard = { id: String(r.t.nextDashboard++), title: body.title.trim(), description: body.description ?? null, widgets: body.widgets ?? [], created_at: now, updated_at: now }
      r.t.dashboards.push(d)
      return json(d, 201)
    }),
    http.patch(`${BASE}/orgs/:org/dashboards/:id`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const d = r.t.dashboards.find((x) => x.id === p(params, 'id'))
      if (!d) return err(404, 'not_found', 'Dashboard not found.')
      const body = (await request.json()) as Partial<Dashboard>
      Object.assign(d, { ...body, id: d.id, updated_at: new Date().toISOString() })
      return json(d)
    }),
    http.delete(`${BASE}/orgs/:org/dashboards/:id`, async ({ params }) => {
      const r = withTel(params)
      if ('error' in r) return r.error
      r.t.dashboards = r.t.dashboards.filter((x) => x.id !== p(params, 'id'))
      return new HttpResponse(null, { status: 204 })
    }),

    // ---- profiles ------------------------------------------------------------
    http.get(`${BASE}/orgs/:org/profiles`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const url = new URL(request.url)
      const projects = projectSet(r.world, r.org, url)
      const tx = url.searchParams.get('transaction')
      const q = (url.searchParams.get('query') ?? '').toLowerCase()
      const [from] = timeRange(r.t, url)
      const rows = r.t.profiles.filter((x) => projects.has(x.project_id) && (!tx || x.transaction_name === tx) && x.start >= from && (!q || x.transaction_name.toLowerCase().includes(q)))
      const pg = pageOf(rows, url)
      return json({ data: pg.data.map(({ project_slug: _s, ...x }) => ({ ...x, start: new Date(x.start).toISOString() })), next_cursor: pg.next_cursor })
    }),
    http.get(`${BASE}/orgs/:org/profiles/aggregate-flamegraph`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const url = new URL(request.url)
      const tx = url.searchParams.get('transaction')
      const projects = projectSet(r.world, r.org, url)
      const latest = r.t.profiles.filter((x) => x.transaction_name === tx && projects.has(x.project_id)).slice(0, 50)
      if (!latest.length) return err(404, 'not_found', 'No profiles for this transaction in the period.')
      return json(flamegraphOf(latest.map(sampledProfile)))
    }),
    http.get(`${BASE}/orgs/:org/profiles/:project/:profileId/flamegraph`, async ({ params }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const prof = r.t.profiles.find((x) => x.profile_id === p(params, 'profileId'))
      if (!prof) return err(404, 'not_found', 'Profile not found. It may have been deleted by the retention policy.')
      return json(flamegraphOf([sampledProfile(prof)]))
    }),
    http.get(`${BASE}/orgs/:org/profiles/:project/:profileId/raw`, async ({ params }) => {
      const r = withTel(params)
      if ('error' in r) return r.error
      const prof = r.t.profiles.find((x) => x.profile_id === p(params, 'profileId'))
      if (!prof) return err(404, 'not_found', 'Profile not found.')
      return json(sampledProfile(prof))
    }),

    // ---- replays -------------------------------------------------------------
    http.get(`${BASE}/orgs/:org/replays`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const url = new URL(request.url)
      const projects = projectSet(r.world, r.org, url)
      const envs = list(url, 'environment')
      const [from] = timeRange(r.t, url)
      let rows = r.t.replays.filter((x) => projects.has(x.project_id) && (!envs.length || envs.includes(x.environment)) && x.started >= from)
      for (const tok of tokenize(url.searchParams.get('query') ?? '')) {
        if (tok.type === 'text') {
          const w = tok.value.toLowerCase()
          rows = rows.filter((x) => x.user.email.includes(w) || x.user.name.toLowerCase().includes(w) || x.urls.some((u) => u.includes(w)))
        } else if (tok.key === 'url') rows = rows.filter((x) => x.urls.some((u) => tok.values.some((v) => u.includes(v))))
        else if (tok.key === 'user.email') rows = rows.filter((x) => tok.values.some((v) => x.user.email.includes(v.toLowerCase())))
        else if (tok.key === 'browser') rows = rows.filter((x) => tok.values.some((v) => x.browser.toLowerCase().startsWith(v.toLowerCase())))
        else if (tok.key === 'count_errors') rows = rows.filter((x) => (tok.op === '>' ? x.issues.length > Number(tok.values[0]) : x.issues.length === Number(tok.values[0])))
      }
      const sort = url.searchParams.get('sort') ?? '-started'
      const key = sort.replace(/^-/, '')
      const val = (x: MockReplay) => (key === 'duration' ? x.duration_ms : key === 'count_errors' ? x.issues.length : key === 'count_clicks' ? x.count_clicks : x.started)
      rows = [...rows].sort((a, b) => (val(a) - val(b)) * (sort.startsWith('-') ? -1 : 1))
      const pg = pageOf(rows, url)
      return json({ data: pg.data.map((x) => replayOut(r.world, x)), next_cursor: pg.next_cursor })
    }),
    http.get(`${BASE}/orgs/:org/replays/:project/:replayId`, async ({ params }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const rep = r.t.replays.find((x) => x.replay_id === p(params, 'replayId'))
      if (!rep) return err(404, 'not_found', 'Replay not found. It may have been deleted by the retention policy.')
      // The fixture recording is what plays; report its length.
      const out = replayOut(r.world, { ...rep, duration_ms: RECORDING_MS })
      // As the server: the issues of the recorded errors, without times.
      const issues = replayErrors(r.world, rep).map((e) => {
        const i = r.world.issues.find((x) => x.id === e.issue_id)!
        return { id: i.id, short_id: i.short_id, title: i.title, level: i.level, status: i.status, event_ids: [e.event_id] }
      })
      return json({ ...out, issues, recording_url: `${BASE}/orgs/${p(params, 'org')}/replays/${p(params, 'project')}/${rep.replay_id}/recording-segments` })
    }),
    http.get(`${BASE}/orgs/:org/replays/:project/:replayId/recording-segments`, async ({ params, request }) => {
      await ctx.wait()
      const r = withTel(params)
      if ('error' in r) return r.error
      const rep = r.t.replays.find((x) => x.replay_id === p(params, 'replayId'))
      if (!rep) return err(404, 'not_found', 'Replay not found.')
      const events = recordingFor(rep)
      // Two segments per page, as the server pages them.
      const segments: (typeof events)[] = []
      const size = Math.ceil(events.length / 4)
      for (let i = 0; i < events.length; i += size) segments.push(events.slice(i, i + size))
      const url = new URL(request.url)
      const offset = Number(url.searchParams.get('cursor') ?? 0) || 0
      return json({ data: segments.slice(offset, offset + 2), next_cursor: offset + 2 < segments.length ? String(offset + 2) : null })
    }),
    http.get(`${BASE}/issues/:id/replays`, async ({ params }) => {
      await ctx.wait()
      const world = ctx.world()
      const t = telemetry(world)
      const id = p(params, 'id')
      const rows = t.replays.filter((x) => x.issues.includes(id))
      return json({ data: rows.map((x) => replayOut(world, x)) })
    }),
  ]
}

/** For tests: the replay id at an index of the fixture list. */
export const mockReplayId = replayIdAt
