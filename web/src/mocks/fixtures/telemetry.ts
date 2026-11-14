// Mock telemetry: transactions and traces with nested spans, thousands of
// log lines, metric series, sampled profiles and session replays. Built
// lazily from the issue world and deterministic for a given `now`.

import type { CallTreeNode, Dashboard, Flamegraph, LogLevel, ProfileFrame } from '@/api/telemetry-types'
import { hashString, Rng } from '../rng'
import { errorSpanId, issueFromTraceId, REPLAY_COUNT, replayIdAt, replaysForIssue } from './ids'
import recordingFixture from './replay-recording.json'
import { DAY, HOUR, iso, type World } from './world'

const MIN = 60_000

// ---- statistics helpers ------------------------------------------------------

/** Standard normal CDF (Abramowitz–Stegun). */
function phi(x: number) {
  const t = 1 / (1 + 0.2316419 * Math.abs(x))
  const d = 0.3989423 * Math.exp((-x * x) / 2)
  const p = d * t * (0.3193815 + t * (-0.3565638 + t * (1.781478 + t * (-1.821256 + t * 1.330274))))
  return x > 0 ? 1 - p : p
}

const Z = { p50: 0, p75: 0.674, p95: 1.645, p99: 2.326 }

function lognormal(rng: Rng, median: number, sigma: number) {
  // Box–Muller
  const u = Math.max(1e-9, rng.float())
  const v = rng.float()
  return median * Math.exp(sigma * Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v))
}

// ---- transactions ------------------------------------------------------------

export interface TxDef {
  project: string
  name: string
  op: string
  p50: number
  sigma: number
  tpm: number
  failure: number
  browser?: boolean
}

export const TX: TxDef[] = [
  { project: 'web-app', name: '/', op: 'pageload', p50: 820, sigma: 0.45, tpm: 42, failure: 0.004, browser: true },
  { project: 'web-app', name: '/products/:slug', op: 'pageload', p50: 1150, sigma: 0.5, tpm: 65, failure: 0.006, browser: true },
  { project: 'web-app', name: '/checkout', op: 'navigation', p50: 640, sigma: 0.55, tpm: 18, failure: 0.021, browser: true },
  { project: 'web-app', name: '/cart', op: 'navigation', p50: 380, sigma: 0.4, tpm: 24, failure: 0.003, browser: true },
  { project: 'web-app', name: '/account/orders', op: 'pageload', p50: 1400, sigma: 0.6, tpm: 6, failure: 0.012, browser: true },
  { project: 'storefront', name: '/', op: 'pageload', p50: 960, sigma: 0.45, tpm: 30, failure: 0.003, browser: true },
  { project: 'storefront', name: '/search', op: 'navigation', p50: 520, sigma: 0.5, tpm: 22, failure: 0.01, browser: true },
  { project: 'api', name: 'GET /api/products', op: 'http.server', p50: 48, sigma: 0.55, tpm: 410, failure: 0.002 },
  { project: 'api', name: 'GET /api/products/{id}', op: 'http.server', p50: 36, sigma: 0.5, tpm: 520, failure: 0.001 },
  { project: 'api', name: 'POST /api/orders', op: 'http.server', p50: 310, sigma: 0.7, tpm: 19, failure: 0.034 },
  { project: 'api', name: 'GET /api/users/me', op: 'http.server', p50: 22, sigma: 0.4, tpm: 380, failure: 0.0005 },
  { project: 'api', name: 'POST /api/auth/login', op: 'http.server', p50: 180, sigma: 0.45, tpm: 12, failure: 0.018 },
  { project: 'api', name: 'GET /api/search', op: 'http.server', p50: 140, sigma: 0.75, tpm: 95, failure: 0.004 },
  { project: 'billing', name: 'POST /invoices', op: 'http.server', p50: 260, sigma: 0.5, tpm: 8, failure: 0.011 },
  { project: 'billing', name: 'GET /invoices/{id}/pdf', op: 'http.server', p50: 920, sigma: 0.6, tpm: 3, failure: 0.02 },
  { project: 'billing', name: 'POST /webhooks/stripe', op: 'http.server', p50: 75, sigma: 0.5, tpm: 14, failure: 0.006 },
  { project: 'worker', name: 'process-order', op: 'queue.process', p50: 540, sigma: 0.6, tpm: 19, failure: 0.009 },
  { project: 'worker', name: 'send-email', op: 'queue.process', p50: 210, sigma: 0.5, tpm: 44, failure: 0.015 },
  { project: 'worker', name: 'reindex-search', op: 'task', p50: 38_000, sigma: 0.25, tpm: 0.02, failure: 0 },
  { project: 'gateway', name: 'POST /api/{project_id}/envelope/', op: 'http.server', p50: 3.2, sigma: 0.6, tpm: 2600, failure: 0.0008 },
]

function percentile(def: TxDef, q: keyof typeof Z, drift = 1) {
  return def.p50 * drift * Math.exp(def.sigma * Z[q])
}

function apdex(def: TxDef, threshold = 300) {
  const cdf = (x: number) => phi(Math.log(x / def.p50) / def.sigma)
  const satisfied = cdf(threshold)
  const tolerating = cdf(4 * threshold) - satisfied
  return Math.max(0, Math.min(1, (satisfied + tolerating / 2) * (1 - def.failure)))
}

export function periodMs(period: string | null | undefined): number {
  const m = /^(\d+)(m|h|d|w)$/.exec(period ?? '')
  if (!m) return 14 * DAY
  return Number(m[1]) * { m: MIN, h: HOUR, d: DAY, w: 7 * DAY }[m[2] as 'm']
}

export function transactionRow(def: TxDef, projectId: string, period: string, envFactor = 1) {
  const minutes = periodMs(period) / MIN
  const drift = 1 + ((hashString(def.name) % 9) - 4) / 100
  const count = Math.max(def.tpm * minutes * envFactor >= 1 ? 1 : 0, Math.round(def.tpm * minutes * envFactor))
  return {
    name: def.name,
    op: def.op,
    project_id: projectId,
    count,
    tpm: Math.round(def.tpm * envFactor * 100) / 100,
    p50: round(percentile(def, 'p50', drift)),
    p75: round(percentile(def, 'p75', drift)),
    p95: round(percentile(def, 'p95', drift)),
    p99: round(percentile(def, 'p99', drift)),
    failure_rate: def.failure,
    apdex: Math.round(apdex(def) * 1000) / 1000,
  }
}

const round = (n: number) => (n < 10 ? Math.round(n * 100) / 100 : Math.round(n * 10) / 10)

const VITALS_BASE: Record<string, [number, number, number, number, number]> = {
  // lcp, fcp, cls, inp, ttfb
  '/': [1850, 980, 0.03, 140, 260],
  '/products/:slug': [2950, 1320, 0.17, 210, 420],
  '/checkout': [2300, 1100, 0.06, 340, 380],
  '/cart': [1600, 860, 0.02, 120, 240],
  '/account/orders': [4300, 1900, 0.09, 260, 690],
  '/search': [2100, 1050, 0.28, 180, 330],
}

export interface Telemetry {
  now: number
  projectIds: Map<string, string>
  /** trace id → [tx def index, duration] for sampled traces. */
  sampled: Map<string, { def: number; duration: number; start: number; profile?: string }>
  logs: MockLog[]
  profiles: MockProfile[]
  replays: MockReplay[]
  dashboards: Dashboard[]
  nextDashboard: number
}

export function summary(t: Telemetry, world: World, def: TxDef, period: string) {
  const projectId = t.projectIds.get(def.project) ?? ''
  const rng = new Rng(`summary:${def.name}:${period}`)
  const ms = periodMs(period)
  const buckets = 48
  const bucket = ms / buckets
  const series = []
  const regression = def.name === '/products/:slug' || def.name === 'POST /api/orders'
  for (let b = 0; b < buckets; b++) {
    const ts = t.now - ms + b * bucket
    const daily = 0.7 + 0.45 * Math.sin((((ts / HOUR) % 24) / 24) * 2 * Math.PI - 1.5)
    const slow = regression && b > buckets * 0.72 && b < buckets * 0.86 ? 1.6 : 1
    const noise = 0.85 + rng.float() * 0.3
    series.push({
      ts: Math.floor(ts / 1000),
      count: Math.round(def.tpm * (bucket / MIN) * daily * noise),
      p50: round(def.p50 * slow * (0.92 + rng.float() * 0.16)),
      p95: round(percentile(def, 'p95') * slow * (0.88 + rng.float() * 0.24)),
    })
  }
  const total = series.reduce((s, p) => s + p.count, 0)
  // Duration histogram from the log-normal density.
  const max = percentile(def, 'p99') * 1.4
  const width = max / 30
  const histogram = Array.from({ length: 30 }, (_, i) => {
    const a = i * width
    const b = a + width
    const cdf = (x: number) => (x <= 0 ? 0 : phi(Math.log(x / def.p50) / def.sigma))
    return { start: round(a), end: round(b), count: Math.round((cdf(b) - cdf(a)) * total) }
  })
  const slowest = Array.from({ length: 10 }, (_, i) => {
    const trace_id = new Rng(`slow:${def.name}:${i}`).hex(32)
    // Independent of the period, so the trace view matches the row; all
    // within the last hour, so every period lists them.
    const r = new Rng(`slow-meta:${trace_id}`)
    const existing = t.sampled.get(trace_id)
    const duration = existing?.duration ?? round(percentile(def, 'p99') * (2.4 - i * 0.12) * (0.95 + r.float() * 0.1))
    const start = existing?.start ?? t.now - r.int(1, 55) * MIN
    const profile = existing ? existing.profile : def.op === 'http.server' || def.browser || def.op === 'queue.process' ? (i % 3 === 0 ? profileIdFor(def.name, i) : undefined) : undefined
    if (!existing) t.sampled.set(trace_id, { def: TX.indexOf(def), duration, start, profile })
    return {
      trace_id,
      span_id: new Rng(`root:${trace_id}`).hex(16),
      project_id: projectId,
      duration_ms: duration,
      timestamp: new Date(start).toISOString(),
      status: i === 2 && def.failure > 0.01 ? 'internal_error' : 'ok',
      release: world.releases.find((x) => x.project_ids.includes(projectId))?.version ?? null,
      user: def.browser || def.project === 'api' ? `${r.pick(['maria', 'jonas', 'li', 'sam', 'ines', 'tomas'])}${r.int(1, 99)}@example.com` : null,
      profile_id: profile ?? null,
      replay_id: def.browser && i % 2 === 0 ? replayIdAt((hashString(def.name) + i) % REPLAY_COUNT) : null,
    }
  })
  const related = world.issues
    .filter((i) => i.project_id === projectId && i.status === 'unresolved')
    .sort((a, b) => b.count - a.count)
    .slice(0, 4)
    .map((i) => ({ id: i.id, short_id: i.short_id, title: i.title, level: i.level, count: Math.round(i.count / 6) }))
  const v = def.browser ? VITALS_BASE[def.name] : undefined
  const vitals = v
    ? { lcp: { p75: v[0], count: Math.round(total * 0.8) }, fcp: { p75: v[1], count: Math.round(total * 0.9) }, cls: { p75: v[2], count: Math.round(total * 0.8) }, inp: { p75: v[3], count: Math.round(total * 0.5) }, ttfb: { p75: v[4], count: total } }
    : {}
  const row = transactionRow(def, projectId, period)
  // The server's shape: `stats`, `interval` seconds, `web_vitals`.
  return {
    name: def.name,
    interval: Math.round(bucket / 1000),
    stats: row,
    series,
    histogram,
    slowest,
    related_issues: related,
    web_vitals: vitals,
  }
}

// ---- traces ------------------------------------------------------------------

interface MockSpan {
  span_id: string
  parent_span_id: string | null
  trace_id: string
  project_id: string
  segment_id: string
  is_segment: boolean
  transaction: string | null
  op: string
  description: string
  status: string
  start_timestamp: number
  timestamp: number
  duration_ms: number
  data: Record<string, unknown>
  tags: Record<string, string>
  profile_id: string | null
}

class TraceBuilder {
  spans: MockSpan[] = []
  readonly rng: Rng
  readonly traceId: string
  readonly projects: Map<string, string>
  constructor(rng: Rng, traceId: string, projects: Map<string, string>) {
    this.rng = rng
    this.traceId = traceId
    this.projects = projects
  }
  id() {
    return this.rng.hex(16)
  }
  add(p: Omit<MockSpan, 'trace_id' | 'span_id' | 'timestamp' | 'data' | 'tags' | 'status' | 'profile_id' | 'is_segment' | 'transaction'> & Partial<MockSpan>) {
    const s: MockSpan = {
      span_id: p.span_id ?? this.id(),
      trace_id: this.traceId,
      status: 'ok',
      data: {},
      tags: {},
      profile_id: null,
      is_segment: false,
      transaction: null,
      ...p,
      timestamp: (p.start_timestamp + p.duration_ms) / 1000,
    }
    this.spans.push(s)
    return s
  }
}

const SQL = [
  'SELECT "products"."id", "products"."name", "products"."price" FROM "products" WHERE "products"."slug" = %s LIMIT 1',
  'SELECT "inventory"."sku", "inventory"."available" FROM "inventory" WHERE "inventory"."product_id" IN (%s, %s, %s)',
  'SELECT COUNT(*) FROM "reviews" WHERE "reviews"."product_id" = %s',
  'SELECT "users"."id", "users"."email" FROM "users" WHERE "users"."id" = %s',
  'INSERT INTO "orders" ("customer_id", "total", "currency", "created_at") VALUES (%s, %s, %s, %s) RETURNING "orders"."id"',
  'UPDATE "inventory" SET "available" = "available" - %s WHERE "sku" = %s',
  'SELECT "prices"."amount" FROM "prices" WHERE "prices"."product_id" = %s AND "prices"."currency" = %s',
]

/** A server transaction with middleware, database, cache and outbound spans. */
function serverTx(b: TraceBuilder, parent: MockSpan | null, project: string, name: string, start: number, duration: number, opts: { fail?: boolean; nPlusOne?: boolean; errorSpan?: string; profile?: string; long?: boolean } = {}) {
  const r = b.rng
  const projectId = b.projects.get(project) ?? ''
  const segId = b.id()
  const seg = b.add({
    span_id: segId,
    parent_span_id: parent?.span_id ?? null,
    project_id: projectId,
    segment_id: segId,
    is_segment: true,
    transaction: name,
    op: project === 'worker' ? (name === 'reindex-search' ? 'task' : 'queue.process') : 'http.server',
    description: name,
    start_timestamp: start,
    duration_ms: duration,
    status: opts.fail ? 'internal_error' : 'ok',
    data: project === 'worker' ? { 'messaging.system': 'redis', 'messaging.destination.name': 'orders', 'job.attempt': 1 } : { 'http.request.method': name.split(' ')[0], 'http.route': name.split(' ')[1] ?? name, 'http.response.status_code': opts.fail ? 500 : 200, 'server.address': `${project}-7d9f${r.hex(2)}` },
    tags: { environment: 'production', 'service.name': project },
    profile_id: opts.profile ?? null,
  })
  if (opts.long) {
    // A batch job: thousands of small spans, for zooming.
    let t = start + 40
    const batches = 120
    for (let i = 0; i < batches && t < start + duration - 200; i++) {
      const batchDur = (duration - 300) / batches
      const batch = b.add({ parent_span_id: segId, project_id: projectId, segment_id: segId, op: 'function', description: `index_batch(${i * 200}..${i * 200 + 199})`, start_timestamp: t, duration_ms: batchDur * 0.95 })
      let u = t + 2
      for (let k = 0; k < 10; k++) {
        const d = batchDur * 0.09 * (0.6 + r.float() * 0.5)
        b.add({ parent_span_id: batch.span_id, project_id: projectId, segment_id: segId, op: k % 3 === 2 ? 'http.client' : 'db.query', description: k % 3 === 2 ? 'POST http://search:9200/_bulk' : SQL[k % 3], start_timestamp: u, duration_ms: d, data: k % 3 === 2 ? { 'http.response.status_code': 200 } : { 'db.system': 'postgresql', 'db.rows': 200 } })
        u += d + 0.4
      }
      t += batchDur
    }
    return seg
  }
  let t = start + Math.min(3, duration * 0.02)
  const mw = b.add({ parent_span_id: segId, project_id: projectId, segment_id: segId, op: 'middleware', description: project === 'billing' ? 'Illuminate\\Auth\\Middleware\\Authenticate' : project === 'worker' ? 'bullmq.worker.run' : 'django.contrib.sessions.middleware.SessionMiddleware', start_timestamp: t, duration_ms: duration * 0.96 })
  t += duration * 0.03
  const end = start + duration * 0.97
  const step = (frac: number) => {
    const d = Math.max(0.3, duration * frac * (0.7 + r.float() * 0.6))
    const s0 = t
    t = Math.min(end, t + d + duration * 0.01)
    return [s0, Math.min(d, end - s0)] as const
  }
  {
    const [s0, d] = step(0.04)
    b.add({ parent_span_id: mw.span_id, project_id: projectId, segment_id: segId, op: 'cache.get', description: `session:${r.hex(12)}`, start_timestamp: s0, duration_ms: d, data: { 'cache.hit': true, 'cache.item_size': 412, 'network.peer.address': 'redis:6379' } })
  }
  const queries = opts.nPlusOne ? 14 : 2 + r.int(0, 3)
  for (let i = 0; i < queries; i++) {
    const [s0, d] = step(opts.nPlusOne ? 0.035 : 0.12)
    const sql = opts.nPlusOne ? SQL[6] : SQL[(i + (name.includes('orders') ? 4 : 0)) % SQL.length]
    const isErr = opts.errorSpan && i === queries - 1
    b.add({
      span_id: isErr ? opts.errorSpan : undefined,
      parent_span_id: mw.span_id,
      project_id: projectId,
      segment_id: segId,
      op: 'db.query',
      description: sql,
      start_timestamp: s0,
      duration_ms: d,
      status: isErr ? 'internal_error' : 'ok',
      data: { 'db.system': 'postgresql', 'db.name': 'shop', 'db.operation': sql.split(' ')[0], 'db.rows': r.int(0, 40) },
    })
  }
  if (name.includes('orders') || name.includes('invoices') || project === 'worker') {
    const [s0, d] = step(0.3)
    const isErr = opts.fail
    b.add({
      parent_span_id: mw.span_id,
      project_id: projectId,
      segment_id: segId,
      op: 'http.client',
      description: project === 'worker' ? 'POST https://api.postmarkapp.com/email' : 'POST https://api.stripe.com/v1/payment_intents',
      start_timestamp: s0,
      duration_ms: d,
      status: isErr ? 'deadline_exceeded' : 'ok',
      data: { 'http.request.method': 'POST', 'http.response.status_code': isErr ? 504 : 200, 'server.address': project === 'worker' ? 'api.postmarkapp.com' : 'api.stripe.com' },
    })
  }
  {
    const [s0, d] = step(0.08)
    b.add({ parent_span_id: mw.span_id, project_id: projectId, segment_id: segId, op: project === 'billing' ? 'view.render' : 'serialize', description: project === 'billing' ? 'invoices/show.blade.php' : 'ProductSerializer.data', start_timestamp: s0, duration_ms: d })
  }
  return seg
}

function browserTx(b: TraceBuilder, def: TxDef, start: number, duration: number, opts: { fail?: boolean; errorSpan?: string; profile?: string; errorProject?: string }) {
  const r = b.rng
  const projectId = b.projects.get(def.project) ?? ''
  const segId = b.id()
  const seg = b.add({
    span_id: segId,
    parent_span_id: null,
    project_id: projectId,
    segment_id: segId,
    is_segment: true,
    transaction: def.name,
    op: def.op,
    description: def.name,
    start_timestamp: start,
    duration_ms: duration,
    data: { 'browser.name': 'Chrome', 'browser.version': '131', 'url.full': `https://app.acme.dev${def.name.replace(':slug', 'trail-jacket')}` },
    tags: { environment: 'production', 'browser.name': 'Chrome' },
    profile_id: opts.profile ?? null,
  })
  const ttfb = Math.min(duration * 0.25, 120 + r.float() * 300)
  if (def.op === 'pageload') {
    b.add({ parent_span_id: segId, project_id: projectId, segment_id: segId, op: 'browser.request', description: 'Request document', start_timestamp: start + 2, duration_ms: ttfb })
    let t = start + ttfb + 8
    const assets = ['/assets/index-3f9a2c1d.js', '/assets/vendor-react-8b21.js', '/assets/router-4c90.js', '/assets/index-d81a.css', '/assets/fonts/geist-latin.woff2', '/img/hero@2x.avif', '/img/products/trail-jacket.avif']
    for (const a of assets) {
      const d = 20 + r.float() * duration * 0.18
      b.add({ parent_span_id: segId, project_id: projectId, segment_id: segId, op: a.endsWith('.js') ? 'resource.script' : a.endsWith('.css') ? 'resource.css' : a.includes('font') ? 'resource.font' : 'resource.img', description: a, start_timestamp: t, duration_ms: d, data: { 'http.response_content_length': r.int(4_000, 240_000), 'http.response_transfer_size': r.int(2_000, 90_000), 'resource.render_blocking_status': a.endsWith('.css') ? 'blocking' : 'non-blocking' } })
      t += 4 + r.float() * 16
    }
    b.add({ parent_span_id: segId, project_id: projectId, segment_id: segId, op: 'browser.domContentLoadedEvent', description: 'DOMContentLoaded', start_timestamp: start + duration * 0.42, duration_ms: 6 })
    b.add({ parent_span_id: segId, project_id: projectId, segment_id: segId, op: 'browser.loadEvent', description: 'load', start_timestamp: start + duration * 0.8, duration_ms: 9 })
  }
  b.add({ parent_span_id: segId, project_id: projectId, segment_id: segId, op: 'ui.long-task', description: 'Main UI thread blocked', start_timestamp: start + duration * 0.36, duration_ms: 60 + r.float() * 140 })
  b.add({ parent_span_id: segId, project_id: projectId, segment_id: segId, op: 'ui.react.render', description: def.name === '/checkout' ? '<CheckoutPage>' : '<ProductPage>', start_timestamp: start + duration * 0.45, duration_ms: duration * 0.12 })
  // API calls with their server transactions.
  const calls =
    def.name === '/checkout'
      ? ['POST /api/orders', 'GET /api/users/me']
      : def.name === '/search'
        ? ['GET /api/search']
        : def.name === '/account/orders'
          ? ['GET /api/users/me', 'GET /api/products']
          : ['GET /api/products/{id}', 'GET /api/users/me', 'GET /api/products']
  let t = start + duration * 0.48
  for (const [i, call] of calls.entries()) {
    const d = Math.min(duration * 0.38, 40 + r.float() * duration * 0.3)
    const failHere = opts.fail && i === 0
    const http = b.add({
      parent_span_id: segId,
      project_id: projectId,
      segment_id: segId,
      op: 'http.client',
      description: `${call.split(' ')[0]} https://api.acme.dev${call.split(' ')[1].replace('{id}', '42')}`,
      start_timestamp: t,
      duration_ms: d,
      status: failHere ? 'internal_error' : 'ok',
      data: { 'http.request.method': call.split(' ')[0], 'http.response.status_code': failHere ? 502 : 200, 'http.response_content_length': r.int(800, 24000) },
    })
    const errHere = failHere && opts.errorProject === 'api' ? opts.errorSpan : undefined
    const apiTx = serverTx(b, http, 'api', call, t + d * 0.08, d * 0.84, { fail: failHere, nPlusOne: call === 'GET /api/products' && r.chance(0.4), errorSpan: errHere })
    if (call === 'POST /api/orders') {
      // the order is processed later by the worker
      const pub = b.add({ parent_span_id: apiTx.span_id, project_id: apiTx.project_id, segment_id: apiTx.span_id, op: 'queue.publish', description: 'orders:process-order', start_timestamp: t + d * 0.7, duration_ms: 2.4 })
      serverTx(b, pub, 'worker', 'process-order', t + d + 180, 420 + r.float() * 400, {})
    }
    t += d * (0.4 + r.float() * 0.5)
  }
  if (opts.errorSpan && opts.errorProject !== 'api') {
    b.add({ span_id: opts.errorSpan, parent_span_id: segId, project_id: projectId, segment_id: segId, op: 'ui.action.click', description: 'button.place-order', start_timestamp: start + duration * 0.9, duration_ms: 12, status: 'internal_error' })
  }
  return seg
}

export function buildTrace(t: Telemetry, world: World, traceId: string) {
  const rng = new Rng(`trace:${traceId}`)
  const b = new TraceBuilder(rng, traceId, t.projectIds)
  const issueId = issueFromTraceId(traceId)
  const issue = issueId ? world.issues.find((i) => i.id === issueId) : undefined
  const sampled = t.sampled.get(traceId)
  let def: TxDef
  if (sampled) def = TX[sampled.def]
  else if (issue) {
    const slug = world.projects.find((p) => p.id === issue.project_id)?.slug ?? 'web-app'
    def = slug === 'web-app' || slug === 'storefront' ? TX.find((d) => d.project === slug && d.name === '/checkout')! ?? TX[2] : TX.find((d) => d.project === 'web-app' && d.name === '/checkout')!
  } else {
    const browserDefs = TX.filter((d) => d.browser)
    def = browserDefs[hashString(traceId) % browserDefs.length]
  }
  const duration = sampled?.duration ?? lognormal(rng, def.p50, def.sigma)
  const start = sampled?.start ?? (issue ? issue.last_seen - 1500 : t.now - rng.int(2, 600) * MIN)
  const errSpan = issue ? errorSpanId(traceId) : undefined
  const issueSlug = issue ? world.projects.find((p) => p.id === issue.project_id)?.slug : undefined
  const fail = Boolean(issue) || (sampled ? sampled.duration > percentile(def, 'p99') * 2 : rng.chance(0.25))
  if (def.browser) browserTx(b, def, start, Math.max(duration, 300), { fail, errorSpan: errSpan, profile: sampled?.profile, errorProject: issueSlug === 'web-app' || issueSlug === 'storefront' ? 'browser' : 'api' })
  else serverTx(b, null, def.project, def.name, start, duration, { fail, profile: sampled?.profile, long: def.name === 'reindex-search', errorSpan: errSpan, nPlusOne: def.name === 'GET /api/products' })

  const errors: Record<string, unknown>[] = []
  if (issue) {
    errors.push({ event_id: new Rng(`ev:${traceId}`).hex(32), issue_id: issue.id, short_id: issue.short_id, title: issue.title, level: issue.level, span_id: errSpan, project_id: issue.project_id, timestamp: iso(b.spans.find((s) => s.span_id === errSpan)?.start_timestamp ?? start) })
  } else {
    // spans that failed get an error event from a matching issue
    for (const s of b.spans.filter((x) => x.status !== 'ok').slice(0, 2)) {
      const candidates = world.issues.filter((i) => i.project_id === s.project_id && i.status === 'unresolved')
      const i = candidates[hashString(s.span_id) % Math.max(1, candidates.length)]
      if (i) errors.push({ event_id: new Rng(`ev:${s.span_id}`).hex(32), issue_id: i.id, short_id: i.short_id, title: i.title, level: i.level, span_id: s.span_id, project_id: i.project_id, timestamp: iso(s.start_timestamp + s.duration_ms * 0.8) })
    }
  }
  const logs = traceLogs(t, b.spans, errors.length > 0)
  const segs = b.spans.filter((s) => s.is_segment)
  return {
    trace_id: traceId,
    transactions: segs.map((s) => ({ span_id: s.span_id, parent_span_id: s.parent_span_id, name: s.transaction, op: s.op, project_id: s.project_id, start_timestamp: s.start_timestamp, duration_ms: s.duration_ms, status: s.status, profile_id: s.profile_id })),
    spans: nest(b.spans),
    errors,
    logs_count: logs.length,
  }
}

/** The span tree as the server sends it: nested `children`, ISO timestamps. */
function nest(spans: MockSpan[]) {
  type Out = Omit<MockSpan, 'timestamp' | 'start_timestamp'> & { start_timestamp: string; end_timestamp: string; children: Out[] }
  const byId = new Map<string, Out>()
  for (const s of spans) {
    const { timestamp: _t, ...rest } = s
    byId.set(s.span_id, { ...rest, start_timestamp: new Date(s.start_timestamp).toISOString(), end_timestamp: new Date(s.start_timestamp + s.duration_ms).toISOString(), children: [] })
  }
  const roots: Out[] = []
  for (const s of spans) {
    const node = byId.get(s.span_id)!
    const parent = s.parent_span_id ? byId.get(s.parent_span_id) : undefined
    if (parent) parent.children.push(node)
    else roots.push(node)
  }
  return roots
}

/** Log lines emitted while the trace ran, attached to its spans. */
export function traceLogs(t: Telemetry, spans: MockSpan[], failed: boolean): MockLog[] {
  if (!spans.length) return []
  const rng = new Rng(`trace-logs:${spans[0].trace_id}`)
  const out: MockLog[] = []
  const pick = spans.filter((s) => s.is_segment || s.op === 'db.query' || s.op === 'http.client' || s.op === 'queue.publish')
  for (const s of pick.slice(0, 40)) {
    const service = [...t.projectIds.entries()].find(([, id]) => id === s.project_id)?.[0] ?? 'api'
    const at = s.start_timestamp + s.duration_ms * rng.float()
    let level: LogLevel = 'info'
    let body: string
    const attributes: Record<string, unknown> = { 'service.name': service }
    if (s.is_segment) body = `Handling ${s.description}`
    else if (s.op === 'db.query') {
      level = s.duration_ms > 80 ? 'warn' : 'debug'
      body = s.duration_ms > 80 ? `Slow query (${Math.round(s.duration_ms)} ms): ${s.description.slice(0, 60)}…` : `Query ${s.description.split(' ')[0]} took ${Math.round(s.duration_ms)} ms`
      attributes['db.system'] = 'postgresql'
      attributes.duration_ms = Math.round(s.duration_ms)
    } else if (s.op === 'queue.publish') body = `Enqueued job ${s.description}`
    else {
      level = s.status === 'ok' ? 'info' : 'error'
      body = s.status === 'ok' ? `${s.description} → ${String(s.data['http.response.status_code'] ?? 200)}` : `Request failed: ${s.description} (${String(s.data['http.response.status_code'] ?? 'timeout')})`
      attributes['http.response.status_code'] = s.data['http.response.status_code'] ?? 504
    }
    out.push({ id: `tl-${s.span_id}`, timestamp: at, project_id: s.project_id, trace_id: s.trace_id, span_id: s.span_id, level, body, attributes, environment: 'production', release: null, sdk: service === 'web-app' ? 'sentry.javascript.browser' : 'sentry.python.django' })
  }
  if (failed) {
    const errSpan = spans.find((s) => s.status !== 'ok') ?? spans[0]
    out.push({ id: `tl-err-${errSpan.span_id}`, timestamp: errSpan.start_timestamp + errSpan.duration_ms, project_id: errSpan.project_id, trace_id: errSpan.trace_id, span_id: errSpan.span_id, level: 'error', body: 'Unhandled exception while processing the request; see the linked issue', attributes: { 'service.name': 'api', 'exception.type': 'PaymentProviderTimeout' }, environment: 'production', release: null, sdk: 'sentry.python.django' })
  }
  return out.sort((a, b) => a.timestamp - b.timestamp)
}

// ---- logs --------------------------------------------------------------------

export interface MockLog {
  id: string
  timestamp: number
  project_id: string
  trace_id: string | null
  span_id: string | null
  level: LogLevel
  body: string
  attributes: Record<string, unknown>
  environment: string
  release: string | null
  sdk: string
}

type LogTemplate = { level: LogLevel; weight: number; make: (r: Rng) => [string, Record<string, unknown>] }

const ROUTES = ['/api/products', '/api/products/{id}', '/api/orders', '/api/users/me', '/api/search', '/api/auth/login', '/api/cart']
const QUEUES = ['orders', 'emails', 'search-index', 'webhooks']
const CUSTOMERS = ['cus_9f2KdL', 'cus_A81mQe', 'cus_Hx02pR', 'cus_T7wqZa', 'cus_44kLmN']

const TEMPLATES_BY_SERVICE: Record<string, { sdk: string; templates: LogTemplate[] }> = {
  api: {
    sdk: 'sentry.python.django',
    templates: [
      {
        level: 'info',
        weight: 40,
        make: (r) => {
          const route = r.pick(ROUTES)
          const method = route.includes('orders') || route.includes('login') || route.includes('cart') ? r.pick(['POST', 'GET']) : 'GET'
          const status = r.weighted([[200, 90], [201, 4], [304, 3], [404, 2], [400, 1]] as const)
          const ms = Math.round(lognormalSimple(r, 40, 0.7))
          return [`${method} ${route.replace('{id}', String(r.int(1, 900)))} ${status} in ${ms}ms`, { 'http.request.method': method, 'http.route': route, 'http.response.status_code': status, duration_ms: ms, 'user.id': String(r.int(1000, 9999)), 'server.address': `api-7d9f${r.hex(2)}` }]
        },
      },
      { level: 'debug', weight: 22, make: (r) => [`Cache ${r.chance(0.8) ? 'hit' : 'miss'} for products:page:${r.int(1, 30)}`, { 'cache.key': `products:page:${r.int(1, 30)}`, 'cache.hit': r.chance(0.8), 'server.address': `api-7d9f${r.hex(2)}` }] },
      {
        level: 'warn',
        weight: 6,
        make: (r) => {
          const ms = r.int(500, 2400)
          return [`Slow query (${ms} ms): SELECT … FROM "orders" WHERE "customer_id" = %s`, { 'db.system': 'postgresql', 'db.operation': 'SELECT', duration_ms: ms, 'server.address': `api-7d9f${r.hex(2)}` }]
        },
      },
      { level: 'error', weight: 3, make: (r) => [`Payment provider request failed: timeout after 10000 ms (order ${r.int(10000, 19999)})`, { 'payment.provider': 'stripe', 'order.id': String(r.int(10000, 19999)), attempt: r.int(1, 3), 'http.response.status_code': 504 }] },
      { level: 'info', weight: 5, make: (r) => [`User ${r.int(1000, 9999)} signed in`, { 'user.id': String(r.int(1000, 9999)), 'auth.method': r.pick(['password', 'sso', 'sso', 'magic_link']) }] },
      { level: 'warn', weight: 2, make: (r) => [`Rate limit close for API token rpt_${r.hex(6)}: ${r.int(85, 99)}% used`, { 'token.prefix': `rpt_${r.hex(6)}` }] },
      { level: 'trace', weight: 1, make: (r) => [`Entering ProductDetailView.get(id=${r.int(1, 900)})`, { 'code.function': 'ProductDetailView.get', 'code.filepath': 'shop/views/products.py' }] },
    ],
  },
  worker: {
    sdk: 'sentry.javascript.node',
    templates: [
      {
        level: 'info',
        weight: 30,
        make: (r) => {
          const job = r.pick(['process-order', 'send-email', 'send-email', 'sync-inventory'])
          const ms = r.int(80, 1400)
          return [`Job ${job} completed in ${ms}ms`, { 'job.name': job, 'job.id': `${r.int(100000, 999999)}`, 'messaging.destination.name': r.pick(QUEUES), duration_ms: ms, attempt: 1 }]
        },
      },
      { level: 'debug', weight: 18, make: (r) => [`Polling queue ${r.pick(QUEUES)}: ${r.int(0, 12)} jobs waiting`, { 'messaging.destination.name': r.pick(QUEUES), 'queue.depth': r.int(0, 12) }] },
      { level: 'warn', weight: 5, make: (r) => [`Retrying job send-email (attempt ${r.int(2, 4)}/5): SMTP 421 Too many connections`, { 'job.name': 'send-email', attempt: r.int(2, 4), 'smtp.code': 421 }] },
      { level: 'error', weight: 2, make: (r) => [`Job sync-inventory failed: ECONNRESET while reading from warehouse API`, { 'job.name': 'sync-inventory', 'job.id': `${r.int(100000, 999999)}`, 'error.type': 'ECONNRESET' }] },
      { level: 'fatal', weight: 0.15, make: () => ['Worker out of memory: heap limit reached (2048 MB), exiting', { 'process.runtime.name': 'node', 'process.memory.heap_limit_mb': 2048 }] },
    ],
  },
  billing: {
    sdk: 'sentry.php.laravel',
    templates: [
      { level: 'info', weight: 20, make: (r) => [`Invoice INV-2026-${String(r.int(1000, 9999)).padStart(5, '0')} created for ${r.pick(CUSTOMERS)}`, { 'invoice.amount': r.int(900, 99000) / 100, currency: 'EUR', 'customer.id': r.pick(CUSTOMERS) }] },
      { level: 'info', weight: 10, make: (r) => [`Stripe webhook ${r.pick(['invoice.paid', 'payment_intent.succeeded', 'customer.updated'])} processed`, { 'webhook.provider': 'stripe', 'webhook.event': r.pick(['invoice.paid', 'payment_intent.succeeded']) }] },
      { level: 'error', weight: 1.5, make: () => ['Stripe webhook signature verification failed', { 'webhook.provider': 'stripe', 'http.response.status_code': 400 }] },
      { level: 'warn', weight: 2, make: (r) => [`PDF rendering took ${r.int(3000, 9000)} ms for invoice`, { duration_ms: r.int(3000, 9000), 'pdf.pages': r.int(1, 6) }] },
    ],
  },
  gateway: {
    sdk: 'sentry.go',
    templates: [
      { level: 'info', weight: 30, make: (r) => [`envelope accepted (${r.int(1, 6)} items, ${r.int(1, 220)} KiB)`, { 'project.id': String(r.int(1, 7)), 'envelope.items': r.int(1, 6), 'http.request.body.size': r.int(1000, 220000) }] },
      { level: 'warn', weight: 1.2, make: (r) => [`rate limited project ${r.int(1, 7)}: 429 for category error`, { 'project.id': String(r.int(1, 7)), 'http.response.status_code': 429, 'rate_limit.category': 'error' }] },
      { level: 'debug', weight: 8, make: (r) => [`key cache refreshed: ${r.int(10, 40)} keys`, { 'cache.size': r.int(10, 40) }] },
    ],
  },
  'web-app': {
    sdk: 'sentry.javascript.browser',
    templates: [
      { level: 'info', weight: 10, make: (r) => [`Route change to ${r.pick(['/checkout', '/cart', '/products/trail-jacket', '/account/orders', '/'])}`, { 'browser.name': r.pick(['Chrome', 'Safari', 'Firefox', 'Edge']), 'url.path': r.pick(['/checkout', '/cart']) }] },
      { level: 'warn', weight: 3, make: (r) => [`Recommendation widget slow to load (${r.int(900, 3000)} ms)`, { 'browser.name': r.pick(['Chrome', 'Safari']), duration_ms: r.int(900, 3000) }] },
      { level: 'error', weight: 1.2, make: () => ['Failed to fetch /api/orders: 502 Bad Gateway', { 'browser.name': 'Chrome', 'http.response.status_code': 502, 'url.path': '/checkout' }] },
      { level: 'debug', weight: 4, make: (r) => [`Feature flag new_checkout = ${r.chance(0.4) ? 'on' : 'off'}`, { 'feature.flag': 'new_checkout' }] },
    ],
  },
}

function lognormalSimple(r: Rng, median: number, sigma: number) {
  return lognormal(r, median, sigma)
}

const SERVICE_WEIGHTS: [string, number][] = [
  ['api', 46],
  ['worker', 18],
  ['gateway', 20],
  ['billing', 8],
  ['web-app', 8],
]

function makeLog(rng: Rng, i: number, timestamp: number, projectIds: Map<string, string>, releases: Map<string, string>, traces: string[]): MockLog {
  const service = rng.weighted(SERVICE_WEIGHTS)
  const def = TEMPLATES_BY_SERVICE[service]
  const tpl = rng.weighted(def.templates.map((x) => [x, x.weight] as const))
  const [body, attributes] = tpl.make(rng)
  const withTrace = service !== 'gateway' && rng.chance(0.45)
  return {
    id: `log-${i}`,
    timestamp,
    project_id: projectIds.get(service) ?? '',
    trace_id: withTrace ? rng.pick(traces) : null,
    span_id: withTrace ? rng.hex(16) : null,
    level: tpl.level,
    body,
    attributes: { 'service.name': service, ...attributes },
    environment: rng.chance(0.9) ? 'production' : 'staging',
    release: releases.get(service) ?? null,
    sdk: def.sdk,
  }
}

/** One live log line at `timestamp` (live tail). */
export function liveLog(t: Telemetry, seq: number, timestamp: number): MockLog {
  const rng = new Rng(`live:${seq}:${timestamp}`)
  return makeLog(rng, 900_000 + seq, timestamp, t.projectIds, new Map(), t.logs.slice(0, 50).map((l) => l.trace_id).filter((x): x is string => Boolean(x)).concat(['0'.repeat(32)]))
}

function buildLogs(now: number, projectIds: Map<string, string>, world: World): MockLog[] {
  const rng = new Rng(`logs:${Math.floor(now / DAY)}`)
  const releases = new Map<string, string>()
  for (const [slug, id] of projectIds) {
    const r = world.releases.find((x) => x.project_ids.includes(id))
    if (r) releases.set(slug, r.version)
  }
  const traces = Array.from({ length: 220 }, (_, i) => new Rng(`log-trace:${i}`).hex(32))
  const out: MockLog[] = []
  // Dense last day (with a daily curve and an incident 3 hours ago), sparse week.
  const DENSE = 6400
  for (let i = 0; i < DENSE; i++) {
    let ts: number
    for (;;) {
      ts = now - rng.float() * DAY
      const hour = ((ts / HOUR) % 24) / 24
      if (rng.float() < 0.55 + 0.45 * Math.sin(hour * 2 * Math.PI - 1.4)) break
    }
    out.push(makeLog(rng, i, ts, projectIds, releases, traces))
  }
  const incidentStart = now - 3 * HOUR
  for (let i = 0; i < 380; i++) {
    const ts = incidentStart + rng.float() * 25 * MIN
    const l = makeLog(rng, DENSE + i, ts, projectIds, releases, traces)
    if (rng.chance(0.75)) {
      l.level = 'error'
      l.body = `Payment provider request failed: timeout after 10000 ms (order ${rng.int(10000, 19999)})`
      l.attributes = { 'service.name': 'api', 'payment.provider': 'stripe', 'http.response.status_code': 504, attempt: rng.int(1, 3) }
      l.project_id = projectIds.get('api') ?? l.project_id
      l.sdk = 'sentry.python.django'
    }
    out.push(l)
  }
  for (let i = 0; i < 2600; i++) {
    out.push(makeLog(rng, DENSE + 400 + i, now - DAY - rng.float() * 13 * DAY, projectIds, releases, traces))
  }
  // A handful of lines in the last minute, so "last hour" always has data.
  for (let i = 0; i < 30; i++) out.push(makeLog(rng, DENSE + 3100 + i, now - rng.float() * 2 * MIN, projectIds, releases, traces))
  return out.sort((a, b) => b.timestamp - a.timestamp)
}

// ---- metrics -----------------------------------------------------------------

export interface MetricDef {
  name: string
  type: 'counter' | 'gauge' | 'distribution' | 'set'
  unit: string | null
  project: string
  base: number
  attributes: Record<string, string[]>
}

export const METRICS: MetricDef[] = [
  { name: 'checkout.completed', type: 'counter', unit: null, project: 'web-app', base: 4.2, attributes: { payment_method: ['card', 'paypal', 'sepa', 'apple_pay'], country: ['DE', 'FR', 'NL', 'US'] } },
  { name: 'checkout.failed', type: 'counter', unit: null, project: 'web-app', base: 0.18, attributes: { reason: ['payment_timeout', 'card_declined', 'validation'], payment_method: ['card', 'paypal'] } },
  { name: 'cart.value', type: 'distribution', unit: 'euro', project: 'web-app', base: 84, attributes: { country: ['DE', 'FR', 'NL', 'US'] } },
  { name: 'http.server.duration', type: 'distribution', unit: 'millisecond', project: 'api', base: 46, attributes: { route: ['/api/products', '/api/orders', '/api/users/me', '/api/search'], status_class: ['2xx', '4xx', '5xx'] } },
  { name: 'db.pool.in_use', type: 'gauge', unit: null, project: 'api', base: 14, attributes: { host: ['api-7d9f1a', 'api-7d9f2b', 'api-7d9f3c'] } },
  { name: 'cache.requests', type: 'counter', unit: null, project: 'api', base: 380, attributes: { result: ['hit', 'miss'], cache: ['products', 'sessions'] } },
  { name: 'users.active', type: 'set', unit: null, project: 'web-app', base: 220, attributes: { plan: ['free', 'pro', 'business'] } },
  { name: 'queue.depth', type: 'gauge', unit: null, project: 'worker', base: 9, attributes: { queue: QUEUES } },
  { name: 'jobs.processed', type: 'counter', unit: null, project: 'worker', base: 62, attributes: { job: ['process-order', 'send-email', 'sync-inventory'], status: ['ok', 'failed'] } },
  { name: 'email.send.duration', type: 'distribution', unit: 'millisecond', project: 'worker', base: 310, attributes: { provider: ['postmark', 'ses'] } },
  { name: 'invoice.amount', type: 'distribution', unit: 'euro', project: 'billing', base: 240, attributes: { plan: ['pro', 'business', 'enterprise'] } },
  { name: 'envelope.size', type: 'distribution', unit: 'byte', project: 'gateway', base: 18_400, attributes: { item_type: ['event', 'transaction', 'replay_recording', 'log'] } },
  { name: 'process.memory.rss', type: 'gauge', unit: 'byte', project: 'worker', base: 512_000_000, attributes: { host: ['worker-1', 'worker-2'] } },
]

const OP_FACTOR: Record<string, number> = { avg: 1, p50: 0.85, p75: 1.25, p95: 2.3, p99: 3.6, min: 0.12, max: 5.2 }

export function metricSeries(t: Telemetry, def: MetricDef, op: string, groupBy: string | null, period: string, interval?: string | null) {
  const ms = periodMs(period)
  const points = 60
  const step = interval ? periodMs(interval) || ms / points : ms / points
  const n = Math.min(400, Math.max(2, Math.round(ms / step)))
  const groups = groupBy ? (def.attributes[groupBy] ?? ['(none)']) : ['']
  const start = Math.floor((t.now - ms) / step) * step
  return {
    name: def.name,
    op,
    type: def.type,
    unit: def.unit,
    truncated: false,
    interval: Math.round(step / 1000),
    series: groups.map((g, gi) => {
      const rng = new Rng(`metric:${def.name}:${op}:${groupBy}:${g}:${period}`)
      const share = groups.length > 1 ? (1 / (gi + 1.4)) * (groups.length / 2) : 1
      const data: [number, number | null][] = []
      let level = def.base
      for (let i = 0; i < n; i++) {
        const ts = start + i * step
        const daily = 0.7 + 0.4 * Math.sin((((ts / HOUR) % 24) / 24) * 2 * Math.PI - 1.4)
        const spike = def.name === 'checkout.failed' && Math.abs(ts - (t.now - 3 * HOUR)) < 25 * MIN ? 9 : def.name === 'queue.depth' && Math.abs(ts - (t.now - 3 * HOUR)) < 40 * MIN ? 6 : 1
        let v: number
        if (def.type === 'counter' || (def.type === 'distribution' && op === 'count') || (def.type === 'set' && op === 'count')) {
          v = def.base * (step / MIN) * daily * share * spike * (0.8 + rng.float() * 0.4)
          if (op === 'avg') v = def.base * daily * share
        } else if (def.type === 'gauge') {
          level = Math.max(0, level + (rng.float() - 0.5) * def.base * 0.15 + (def.base * share - level) * 0.1)
          v = level * spike * (op === 'max' ? 1.4 : op === 'min' ? 0.6 : 1)
        } else if (def.type === 'set') {
          v = def.base * daily * share * (0.85 + rng.float() * 0.3) * Math.sqrt(step / HOUR)
        } else {
          v = def.base * (OP_FACTOR[op] ?? 1) * (gi === 0 ? 1 : 0.7 + gi * 0.35) * (0.85 + rng.float() * 0.3) * (op === 'sum' ? daily * (step / MIN) * 4 : 1)
        }
        data.push([Math.floor(ts / 1000), Math.round(v * 100) / 100])
      }
      return { group: groupBy ? { [groupBy]: g } : null, total: Math.round(data.reduce((s, [, v]) => s + (v ?? 0), 0) * 100) / 100, points: data }
    }),
  }
}

// ---- profiles ----------------------------------------------------------------

interface ModelNode {
  f: [string, string, string | null, boolean, number?]
  /** Share of samples that stop here. */
  self?: number
  /** Time window (fractions of the profile) this call is active in. */
  at?: [number, number]
  weight?: number
  children?: ModelNode[]
}

const app = (name: string, file: string, line?: number): ModelNode['f'] => [name, file, null, true, line]
const lib = (name: string, file: string, pkg: string | null, line?: number): ModelNode['f'] => [name, file, pkg, false, line]

const PY_MODEL: ModelNode = {
  f: lib('run', 'gunicorn/workers/sync.py', 'gunicorn', 126),
  children: [
    {
      f: lib('handle_request', 'gunicorn/workers/sync.py', 'gunicorn', 178),
      children: [
        {
          f: lib('__call__', 'django/core/handlers/wsgi.py', 'django', 124),
          children: [
            {
              f: lib('get_response', 'django/core/handlers/base.py', 'django', 140),
              children: [
                { f: lib('process_request', 'django/contrib/sessions/middleware.py', 'django', 19), at: [0, 0.12], self: 0.2, children: [{ f: lib('get', 'redis/commands/core.py', 'redis', 1829), self: 0.3, children: [{ f: lib('read_response', 'redis/connection.py', 'redis', 512), self: 1 }] }] },
                {
                  f: lib('_get_response', 'django/core/handlers/base.py', 'django', 197),
                  at: [0.1, 1],
                  children: [
                    {
                      f: app('ProductDetailView.get', 'shop/views/products.py', 48),
                      self: 0.04,
                      children: [
                        {
                          f: app('load_product', 'shop/services/catalog.py', 112),
                          at: [0.1, 0.55],
                          children: [
                            {
                              f: lib('get', 'django/db/models/query.py', 'django', 633),
                              weight: 3,
                              children: [
                                {
                                  f: lib('__iter__', 'django/db/models/query.py', 'django', 398),
                                  children: [
                                    {
                                      f: lib('execute_sql', 'django/db/models/sql/compiler.py', 'django', 1562),
                                      self: 0.1,
                                      children: [{ f: lib('execute', 'psycopg/cursor.py', 'psycopg', 732), self: 0.15, children: [{ f: lib('wait', 'psycopg/waiting.py', 'psycopg', 320), self: 1 }] }],
                                    },
                                  ],
                                },
                              ],
                            },
                            { f: app('apply_regional_prices', 'shop/services/pricing.py', 77), weight: 1.2, self: 0.6, children: [{ f: lib('quantize', 'decimal.py', 'python', 2409), self: 1 }] },
                          ],
                        },
                        {
                          f: app('compute_recommendations', 'shop/services/recommendations.py', 31),
                          at: [0.5, 0.78],
                          self: 0.15,
                          children: [
                            { f: lib('dot', 'numpy/core/multiarray.py', 'numpy', 0), self: 1, weight: 2 },
                            { f: lib('argsort', 'numpy/core/fromnumeric.py', 'numpy', 1133), self: 1 },
                          ],
                        },
                        {
                          f: lib('data', 'rest_framework/serializers.py', 'djangorestframework', 553),
                          at: [0.76, 0.98],
                          children: [
                            {
                              f: lib('to_representation', 'rest_framework/serializers.py', 'djangorestframework', 522),
                              self: 0.3,
                              children: [
                                { f: app('ProductSerializer.get_image_urls', 'shop/serializers.py', 64), self: 0.5, children: [{ f: lib('url', 'storages/backends/s3.py', 'django-storages', 590), self: 1 }] },
                                { f: lib('to_representation', 'rest_framework/fields.py', 'djangorestframework', 1173), self: 1 },
                              ],
                            },
                          ],
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          ],
        },
      ],
    },
  ],
}

const JS_MODEL: ModelNode = {
  f: lib('performWorkUntilDeadline', 'react-dom/cjs/scheduler.production.js', 'scheduler', 151),
  children: [
    {
      f: lib('workLoopConcurrent', 'react-dom/cjs/react-dom-client.production.js', 'react-dom', 10872),
      at: [0, 0.7],
      children: [
        {
          f: lib('beginWork', 'react-dom/cjs/react-dom-client.production.js', 'react-dom', 7110),
          self: 0.2,
          children: [
            { f: app('ProductPage', 'src/pages/product-page.tsx', 41), self: 0.2, children: [{ f: app('useProductQuery', 'src/api/products.ts', 18), self: 0.4, children: [{ f: lib('fetchQuery', '@tanstack/query-core/build/modern/queryClient.js', '@tanstack/query-core', 140), self: 1 }] }] },
            { f: app('Gallery', 'src/components/gallery.tsx', 22), self: 0.3, children: [{ f: app('computeSrcSet', 'src/lib/images.ts', 9), self: 1 }] },
            { f: app('PriceTag', 'src/components/price-tag.tsx', 12), self: 0.2, children: [{ f: lib('format', 'Intl.NumberFormat', null, 0), self: 1 }] },
          ],
        },
        { f: lib('completeWork', 'react-dom/cjs/react-dom-client.production.js', 'react-dom', 8022), self: 0.6, children: [{ f: lib('setInitialProperties', 'react-dom/cjs/react-dom-client.production.js', 'react-dom', 13001), self: 1 }] },
      ],
    },
    {
      f: lib('commitRoot', 'react-dom/cjs/react-dom-client.production.js', 'react-dom', 11904),
      at: [0.6, 0.85],
      self: 0.3,
      children: [{ f: lib('commitLayoutEffects', 'react-dom/cjs/react-dom-client.production.js', 'react-dom', 9420), self: 0.4, children: [{ f: app('useStickyHeader', 'src/hooks/use-sticky-header.ts', 7), self: 1 }] }],
    },
    {
      f: lib('flush', 'src/lib/analytics.ts', null, 0),
      at: [0.82, 1],
      children: [{ f: app('trackPageView', 'src/lib/analytics.ts', 33), self: 0.4, children: [{ f: lib('stringify', 'JSON.stringify', null, 0), self: 1 }] }],
    },
  ],
}

const NODE_MODEL: ModelNode = {
  f: lib('processTicksAndRejections', 'node:internal/process/task_queues', 'node', 95),
  children: [
    {
      f: lib('Worker.processJob', 'bullmq/dist/cjs/classes/worker.js', 'bullmq', 512),
      children: [
        {
          f: app('processOrder', 'src/jobs/process-order.ts', 14),
          self: 0.05,
          children: [
            { f: app('loadOrder', 'src/db/orders.ts', 40), at: [0, 0.3], children: [{ f: lib('Client.query', 'pg/lib/client.js', 'pg', 526), self: 0.3, children: [{ f: lib('Parser.parse', 'pg-protocol/dist/parser.js', 'pg-protocol', 99), self: 1 }] }] },
            { f: app('reserveInventory', 'src/services/inventory.ts', 61), at: [0.28, 0.55], self: 0.2, children: [{ f: lib('Client.query', 'pg/lib/client.js', 'pg', 526), self: 1 }] },
            { f: app('renderReceipt', 'src/emails/receipt.ts', 22), at: [0.52, 0.8], self: 0.1, children: [{ f: lib('render', 'ejs/lib/ejs.js', 'ejs', 412), self: 0.5, children: [{ f: lib('escapeXML', 'ejs/lib/utils.js', 'ejs', 109), self: 1 }] }] },
            { f: app('sendEmail', 'src/services/mailer.ts', 18), at: [0.78, 1], children: [{ f: lib('request', 'undici/lib/api/api-request.js', 'undici', 171), self: 0.4, children: [{ f: lib('TLSSocket.write', 'node:tls', 'node', 0), self: 1 }] }] },
          ],
        },
      ],
    },
  ],
}

export interface MockProfile {
  profile_id: string
  project_id: string
  project_slug: string
  kind: 'transaction' | 'continuous'
  transaction_name: string
  trace_id: string
  platform: string
  release: string | null
  environment: string
  start: number
  duration_ms: number
}

function profileIdFor(name: string, i: number) {
  return new Rng(`profile:${name}:${i}`).hex(32)
}

function modelFor(p: { project_slug: string }) {
  return p.project_slug === 'api' ? PY_MODEL : p.project_slug === 'worker' ? NODE_MODEL : JS_MODEL
}

/** A sampled profile (version 1 format) simulated from a call model. */
export function sampledProfile(p: MockProfile) {
  const rng = new Rng(`samples:${p.profile_id}`)
  const model = modelFor(p)
  const frames: { function: string; filename: string; module?: string; package?: string; lineno?: number; in_app: boolean }[] = []
  const frameIndex = new Map<string, number>()
  const idx = (f: ModelNode['f']) => {
    const key = f.join('|')
    let i = frameIndex.get(key)
    if (i === undefined) {
      i = frames.length
      frameIndex.set(key, i)
      frames.push({ function: f[0], filename: f[1], package: f[2] ?? undefined, lineno: f[4] || undefined, in_app: f[3] })
    }
    return i
  }
  const stacks: number[][] = []
  const stackIndex = new Map<string, number>()
  const samples: { stack_id: number; thread_id: string; elapsed_since_start_ns: string }[] = []
  const interval = 10_000_000 // 10 ms in ns
  const count = Math.max(30, Math.min(2400, Math.round(p.duration_ms / 10)))
  const speed = 0.7 + rng.float() * 0.6
  for (let s = 0; s < count; s++) {
    const f = Math.min(0.999, (s / count) * speed + (1 - speed) * (rng.float() * 0.2))
    const path: number[] = []
    let node: ModelNode | undefined = model
    while (node) {
      path.push(idx(node.f))
      if (!node.children?.length || rng.float() < (node.self ?? 0)) break
      const active: ModelNode[] = node.children.filter((c) => !c.at || (f >= c.at[0] - 0.03 && f <= c.at[1] + 0.03))
      const pool: ModelNode[] = active.length ? active : node.children
      node = rng.weighted(pool.map((c) => [c, c.weight ?? 1] as const))
    }
    // the format lists stacks leaf first
    const stack = [...path].reverse()
    const key = stack.join(',')
    let si = stackIndex.get(key)
    if (si === undefined) {
      si = stacks.length
      stackIndex.set(key, si)
      stacks.push(stack)
    }
    samples.push({ stack_id: si, thread_id: '1', elapsed_since_start_ns: String(s * interval) })
  }
  return {
    event_id: p.profile_id,
    version: '1',
    platform: p.platform,
    release: p.release,
    environment: p.environment,
    timestamp: iso(p.start),
    transaction: { name: p.transaction_name, id: new Rng(`tx:${p.profile_id}`).hex(32), trace_id: p.trace_id, active_thread_id: '1' },
    profile: { frames, stacks, samples, thread_metadata: { '1': { name: 'main' } } },
  }
}

/** Merges sampled profiles into a call tree (callees in first-seen order), like the server. */
export function flamegraphOf(profiles: ReturnType<typeof sampledProfile>[]): Flamegraph {
  const frames: ProfileFrame[] = []
  const frameKey = new Map<string, number>()
  const root: CallTreeNode & { by?: Map<number, CallTreeNode & { by?: Map<number, unknown> }> } = { frame_index: null, self_weight: 0, total_weight: 0, children: [] }
  type N = CallTreeNode & { by?: Map<number, N> }
  let samples = 0
  for (const prof of profiles) {
    const local = prof.profile.frames.map((f) => {
      const key = `${f.function}|${f.filename}|${f.package ?? ''}|${f.lineno ?? ''}`
      let i = frameKey.get(key)
      if (i === undefined) {
        i = frames.length
        frameKey.set(key, i)
        frames.push({ name: f.function, file: f.filename, line: f.lineno ?? null, package: f.package ?? null, is_application: f.in_app })
      }
      return i
    })
    for (const s of prof.profile.samples) {
      const stack = prof.profile.stacks[s.stack_id]
      let node = root as N
      node.total_weight += 1
      for (let k = stack.length - 1; k >= 0; k--) {
        const fi = local[stack[k]]
        node.by ??= new Map()
        let child = node.by.get(fi)
        if (!child) {
          child = { frame_index: fi, self_weight: 0, total_weight: 0, children: [] }
          node.by.set(fi, child)
          node.children.push(child)
        }
        child.total_weight += 1
        node = child
      }
      node.self_weight += 1
      samples += 1
    }
  }
  const strip = (n: N): CallTreeNode => ({ frame_index: n.frame_index, self_weight: n.self_weight, total_weight: n.total_weight, children: n.children.map((c) => strip(c as N)) })
  return { frames, root: strip(root as N), samples, profiles: profiles.length }
}

function buildProfiles(t: Telemetry, world: World): MockProfile[] {
  const rng = new Rng('profiles')
  const out: MockProfile[] = []
  const defs = TX.filter((d) => (d.project === 'api' && d.op === 'http.server') || (d.project === 'web-app' && d.op === 'pageload') || (d.project === 'worker' && d.name === 'process-order'))
  for (const def of defs) {
    const projectId = t.projectIds.get(def.project) ?? ''
    const release = world.releases.find((r) => r.project_ids.includes(projectId))?.version ?? null
    const n = def.project === 'api' ? 14 : 9
    for (let i = 0; i < n; i++) {
      const profile_id = profileIdFor(def.name, i)
      const trace_id = new Rng(`slow:${def.name}:${i}`).hex(32)
      const sampled = t.sampled.get(trace_id)
      const duration = sampled?.duration ?? Math.max(def.browser ? 400 : 30, lognormal(rng, def.p50 * (def.browser ? 1 : 2.5), def.sigma))
      out.push({
        profile_id,
        project_id: projectId,
        project_slug: def.project,
        kind: def.project === 'worker' && i % 3 === 1 ? 'continuous' : 'transaction',
        transaction_name: def.name,
        trace_id,
        platform: def.project === 'api' ? 'python' : def.project === 'worker' ? 'node' : 'javascript',
        release,
        environment: rng.chance(0.85) ? 'production' : 'staging',
        start: sampled?.start ?? t.now - rng.int(3, 7 * 24 * 60) * MIN,
        duration_ms: Math.round(Math.min(duration, 20_000)),
      })
      // traces of these profiles point back to them
      if (!sampled) t.sampled.set(trace_id, { def: TX.indexOf(def), duration: Math.round(Math.min(duration, 20_000)), start: out[out.length - 1].start, profile: profile_id })
      else sampled.profile = profile_id
    }
  }
  return out.sort((a, b) => b.start - a.start)
}

// ---- replays -----------------------------------------------------------------

export interface MockReplay {
  replay_id: string
  project_id: string
  started: number
  duration_ms: number
  urls: string[]
  user: { id: string; email: string; name: string; ip: string }
  browser: string
  os: string
  device: string
  environment: string
  release: string | null
  count_clicks: number
  /** Issue ids whose errors the replay captured. */
  issues: string[]
}

interface RecordingFixture {
  url: string
  errors: { offset_ms: number; title: string }[]
  events: { type: number; data: unknown; timestamp: number }[]
}

export const RECORDING = recordingFixture as unknown as RecordingFixture
export const RECORDING_MS = RECORDING.events[RECORDING.events.length - 1].timestamp - RECORDING.events[0].timestamp

// The pages of the recorded demo shop, so the list matches the recording.
const PAGES = ['/', '/products', '/cart', '/order/confirmed', '/journal', '/products?sort=price']
const NAMES = ['Maria Keller', 'Jonas Weber', 'Li Wei', 'Sam Carter', 'Inês Duarte', 'Tomás Rivera', 'Aiko Tanaka', 'Noah Fischer', 'Emma Laurent', 'Ola Nordmann']

function buildReplays(t: Telemetry, world: World): MockReplay[] {
  const jsProjects = world.projects.filter((p) => p.slug === 'web-app' || p.slug === 'storefront')
  // The most frequent browser issues have replays; most sessions have no errors.
  const jsIssues = world.issues
    .filter((i) => jsProjects.some((p) => p.id === i.project_id))
    .sort((a, b) => b.count - a.count)
    .slice(0, 30)
  const byReplay = new Map<number, string[]>()
  for (const i of jsIssues) for (const r of replaysForIssue(i.id)) byReplay.set(r, [...(byReplay.get(r) ?? []), i.id])
  const out: MockReplay[] = []
  for (let i = 0; i < REPLAY_COUNT; i++) {
    const rng = new Rng(`replay-meta:${i}`)
    const project = jsProjects[i % 5 === 4 ? 1 : 0] ?? jsProjects[0]
    const name = rng.pick(NAMES)
    const issues = (byReplay.get(i) ?? []).slice(0, 4)
    const urlCount = rng.int(2, 6)
    const browser = rng.weighted([
      ['Chrome 131', 55],
      ['Safari 18.1', 22],
      ['Firefox 133', 12],
      ['Edge 131', 8],
      ['Mobile Safari 18.1', 14],
    ] as const)
    const mobile = browser.startsWith('Mobile')
    out.push({
      replay_id: replayIdAt(i),
      project_id: project.id,
      started: t.now - (i === 0 ? 4 * MIN : rng.int(10, 14 * 24 * 60) * MIN),
      duration_ms: Math.round(i < 8 ? RECORDING_MS : rng.powerLaw(8, 1800) * 1000),
      urls: rng.shuffle(PAGES).slice(0, urlCount).map((p) => `https://shop.acme.dev${p}`),
      user: { id: String(rng.int(1000, 99999)), email: `${name.split(' ')[0].toLowerCase().normalize('NFD').replace(/[^a-z]/g, '')}.${name.split(' ')[1].toLowerCase()}@example.com`, name, ip: `203.0.113.${rng.int(2, 254)}` },
      browser,
      os: mobile ? 'iOS 18.1' : rng.pick(['macOS 15.1', 'Windows 11', 'Windows 10', 'Ubuntu 24.04']),
      device: mobile ? 'iPhone' : 'Desktop',
      environment: rng.chance(0.9) ? 'production' : 'staging',
      release: world.releases.find((r) => r.project_ids.includes(project.id))?.version ?? null,
      count_clicks: rng.int(2, 60),
      issues,
    })
  }
  return out.sort((a, b) => b.started - a.started)
}

// ---- dashboards --------------------------------------------------------------

function seedDashboards(now: number): Dashboard[] {
  const at = (d: number) => new Date(now - d * DAY).toISOString()
  return [
    {
      id: '1',
      title: 'Checkout health',
      description: 'Conversion, failures and latency of the checkout flow.',
      created_at: at(40),
      updated_at: at(2),
      widgets: [
        { id: 'w1', title: 'Completed checkouts', kind: 'metric', chart: 'bar', query: { name: 'checkout.completed', op: 'sum' }, width: 2 },
        { id: 'w2', title: 'Failed checkouts by reason', kind: 'metric', chart: 'area', query: { name: 'checkout.failed', op: 'sum', group_by: 'reason' }, width: 2 },
        { id: 'w3', title: 'POST /api/orders p95', kind: 'transactions', chart: 'line', query: { transaction: 'POST /api/orders', field: 'p95' }, width: 2 },
        { id: 'w4', title: 'Payment errors in logs', kind: 'logs', chart: 'bar', query: { query: 'level:error payment.provider:stripe' }, width: 2 },
        { id: 'w5', title: 'Cart value p75 by country', kind: 'metric', chart: 'line', query: { name: 'cart.value', op: 'p75', group_by: 'country' }, width: 3 },
        { id: 'w6', title: 'Top checkout issues', kind: 'issues', chart: 'bar', query: { query: 'is:unresolved transaction:/checkout' }, width: 1 },
      ],
    },
    {
      id: '2',
      title: 'Workers & queues',
      description: null,
      created_at: at(20),
      updated_at: at(6),
      widgets: [
        { id: 'w1', title: 'Queue depth', kind: 'metric', chart: 'line', query: { name: 'queue.depth', op: 'avg', group_by: 'queue' }, width: 2 },
        { id: 'w2', title: 'Jobs processed', kind: 'metric', chart: 'bar', query: { name: 'jobs.processed', op: 'sum', group_by: 'status' }, width: 2 },
        { id: 'w3', title: 'Email send p95', kind: 'metric', chart: 'line', query: { name: 'email.send.duration', op: 'p95' }, width: 2 },
        { id: 'w4', title: 'Worker memory (RSS)', kind: 'metric', chart: 'area', query: { name: 'process.memory.rss', op: 'max', group_by: 'host' }, width: 2 },
      ],
    },
  ]
}

// ---- world -------------------------------------------------------------------

export function createTelemetry(world: World): Telemetry {
  const acme = world.orgs.find((o) => o.slug === 'acme')
  const projectIds = new Map(world.projects.filter((p) => p.org_id === acme?.id).map((p) => [p.slug, p.id]))
  const t: Telemetry = { now: world.now, projectIds, sampled: new Map(), logs: [], profiles: [], replays: [], dashboards: seedDashboards(world.now), nextDashboard: 3 }
  // Register the slowest samples of every transaction so their traces match.
  for (const def of TX) summary(t, world, def, '14d')
  t.logs = buildLogs(world.now, projectIds, world)
  t.profiles = buildProfiles(t, world)
  t.replays = buildReplays(t, world)
  return t
}

/** Recording events of a replay, shifted to its start time. */
export function recordingFor(r: MockReplay) {
  const shift = r.started - RECORDING.events[0].timestamp
  return RECORDING.events.map((e) => ({ ...e, timestamp: e.timestamp + shift, data: shiftData(e, shift) }))
}

/** Custom events carry their own timestamps (seconds); shift those too. */
function shiftData(e: { type: number; data: unknown }, shift: number): unknown {
  if (e.type !== 5) return e.data
  const d = e.data as { tag: string; payload: Record<string, unknown> }
  const p = { ...d.payload }
  for (const k of ['timestamp', 'startTimestamp', 'endTimestamp']) if (typeof p[k] === 'number') p[k] = (p[k] as number) + shift / 1000
  return { ...d, payload: p }
}

export function replayErrors(world: World, r: MockReplay) {
  return r.issues.map((issueId, k) => {
    const issue = world.issues.find((i) => i.id === issueId)!
    const offset = k === 0 ? (RECORDING.errors[0]?.offset_ms ?? 15_000) : Math.min(RECORDING_MS - 500, 5_000 + k * 6_000)
    return { event_id: new Rng(`replay-err:${r.replay_id}:${issueId}`).hex(32), issue_id: issue.id, short_id: issue.short_id, title: issue.title, timestamp: iso(r.started + offset) }
  })
}
