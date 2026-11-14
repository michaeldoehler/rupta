// Endpoints, normalisers and query hooks for performance, traces, logs,
// metrics, dashboards, profiles and session replays. Kept apart from
// endpoints.ts so the issue pages do not load any of it.
//
// The normalisers are tolerant in the same way as adapt.ts: timestamps may
// be ISO strings, epoch seconds or epoch milliseconds; series may be
// `[[ts, v]]` pairs or objects; collections may be bare arrays or pages.

import { keepPreviousData, queryOptions, useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { request } from './client'
import type {
  CallTreeNode,
  Dashboard,
  Flamegraph,
  HistogramBucket,
  LogHistogram,
  LogLevel,
  LogRow,
  MetricName,
  MetricOp,
  MetricQueryResult,
  MetricType,
  ProfileFrame,
  ProfileRow,
  RecordingEvent,
  RelatedIssue,
  ReplayDetail,
  ReplayError,
  ReplayRow,
  SeriesPoint,
  SpanGroupBy,
  SpanGroupRow,
  TelemetryPeriod,
  Trace,
  TraceError,
  TraceSpan,
  TransactionRow,
  TransactionSample,
  TransactionSort,
  TransactionSummary,
  VitalName,
  Widget,
} from './telemetry-types'
import type { Id, Level, Page } from './types'

type Raw = Record<string, unknown>
const obj = (v: unknown): Raw => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Raw) : {})
const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : [])
const str = (v: unknown): string | null => (v === null || v === undefined || v === '' ? null : String(v))
const num = (v: unknown, d = 0): number => {
  const n = typeof v === 'number' ? v : typeof v === 'string' && v.trim() !== '' ? Number(v) : NaN
  return Number.isFinite(n) ? n : d
}
const numOrNull = (v: unknown): number | null => {
  const n = num(v, NaN)
  return Number.isFinite(n) ? n : null
}

/** Epoch milliseconds from an ISO string, epoch seconds or epoch milliseconds. */
export function toMs(v: unknown): number {
  if (typeof v === 'number') return v < 1e11 ? v * 1000 : v
  if (typeof v === 'string') {
    if (/^\d+(\.\d+)?$/.test(v)) return toMs(Number(v))
    const t = Date.parse(v)
    return Number.isFinite(t) ? t : 0
  }
  return 0
}

const isoOf = (v: unknown): string => {
  const ms = toMs(v)
  return ms ? new Date(ms).toISOString() : ''
}

/** Collection body: bare array, `{data: [...]}` or `{data, next_cursor}`. */
function listOf(body: unknown): unknown[] {
  if (Array.isArray(body)) return body
  const o = obj(body)
  return arr(o.data ?? o.rows ?? o.items)
}

function pageOf<T>(body: unknown, map: (v: unknown) => T): Page<T> {
  return { data: listOf(body).map(map), next_cursor: str(obj(body).next_cursor) }
}

const e = encodeURIComponent

// ---- performance -------------------------------------------------------------

export function normTransactionRow(raw: unknown): TransactionRow {
  const o = obj(raw)
  return {
    name: String(o.name ?? o.transaction ?? ''),
    op: str(o.op),
    project_id: str(o.project_id),
    count: num(o.count),
    tpm: num(o.tpm),
    p50: num(o.p50),
    p75: num(o.p75),
    p95: num(o.p95),
    p99: num(o.p99),
    failure_rate: num(o.failure_rate),
    apdex: num(o.apdex, 1),
  }
}

/** Series as `[{ts, count, p50, p95}]`, `{count: [[ts,v]], p50: ..., p95: ...}` or `[[ts, count, p50, p95]]`. */
export function normSummarySeries(raw: unknown): SeriesPoint[] {
  if (Array.isArray(raw)) {
    return raw.map((p) => {
      if (Array.isArray(p)) return { ts: toMs(p[0]), count: num(p[1]), p50: num(p[2]), p95: num(p[3]) }
      const o = obj(p)
      return { ts: toMs(o.ts ?? o.timestamp ?? o.time), count: num(o.count), p50: num(o.p50), p95: num(o.p95) }
    })
  }
  const o = obj(raw)
  const byTs = new Map<number, SeriesPoint>()
  for (const field of ['count', 'p50', 'p95'] as const) {
    for (const p of arr(o[field])) {
      const [t, v] = p as [unknown, unknown]
      const ts = toMs(t)
      const cur = byTs.get(ts) ?? { ts, count: 0, p50: 0, p95: 0 }
      cur[field] = num(v)
      byTs.set(ts, cur)
    }
  }
  return [...byTs.values()].sort((a, b) => a.ts - b.ts)
}

function normHistogram(raw: unknown): HistogramBucket[] {
  return arr(raw).map((b) => {
    if (Array.isArray(b)) return { start: num(b[0]), end: num(b[1]), count: num(b[2]) }
    const o = obj(b)
    const start = num(o.start ?? o.bucket ?? o.min)
    return { start, end: num(o.end ?? o.max, start), count: num(o.count) }
  })
}

function normSample(raw: unknown): TransactionSample {
  const o = obj(raw)
  return {
    trace_id: String(o.trace_id ?? ''),
    span_id: str(o.span_id),
    event_id: str(o.event_id),
    project_id: str(o.project_id),
    duration_ms: num(o.duration_ms ?? o.duration),
    timestamp: isoOf(o.timestamp ?? o.start),
    status: str(o.status),
    release: str(o.release),
    user: str(o.user ?? o.user_email ?? o.user_id),
    profile_id: str(o.profile_id),
    replay_id: str(o.replay_id),
  }
}

function normRelatedIssue(raw: unknown): RelatedIssue {
  const o = obj(raw)
  return { id: String(o.id ?? o.issue_id), short_id: String(o.short_id ?? ''), title: String(o.title ?? ''), level: (str(o.level) ?? 'error') as Level, count: num(o.count) }
}

const VITALS: VitalName[] = ['lcp', 'fcp', 'cls', 'inp', 'ttfb']

export function normSummary(raw: unknown, name: string): TransactionSummary {
  const o = obj(raw)
  const vitalsRaw = obj(o.vitals ?? o.web_vitals)
  const vitals: TransactionSummary['vitals'] = {}
  for (const k of VITALS) {
    const v = vitalsRaw[k] ?? vitalsRaw[k.toUpperCase()]
    if (v === undefined || v === null) continue
    vitals[k] = typeof v === 'number' ? { p75: v, count: 0 } : { p75: numOrNull(obj(v).p75), count: num(obj(v).count) }
  }
  const t = o.stats ?? o.totals ?? o.summary
  return {
    name: String(o.name ?? name),
    op: str(o.op ?? obj(o.stats).op),
    totals: t ? (({ count, tpm, p50, p95, failure_rate, apdex }) => ({ count, tpm, p50, p95, failure_rate, apdex }))(normTransactionRow(t)) : null,
    series: normSummarySeries(o.series ?? o.timeseries),
    histogram: normHistogram(o.histogram ?? o.duration_histogram),
    slowest: arr(o.slowest ?? o.slowest_samples ?? o.samples).map(normSample),
    related_issues: arr(o.related_issues ?? o.issues).map(normRelatedIssue),
    vitals,
  }
}

function strRecord(v: unknown): Record<string, string> {
  if (Array.isArray(v)) return Object.fromEntries(v.map((p) => [String((p as unknown[])[0]), String((p as unknown[])[1])]))
  return Object.fromEntries(Object.entries(obj(v)).map(([k, x]) => [k, typeof x === 'string' ? x : JSON.stringify(x)]))
}

export function normSpan(raw: unknown, traceId = ''): TraceSpan {
  const o = obj(raw)
  const start = toMs(o.start_timestamp ?? o.start ?? o.timestamp)
  let duration = numOrNull(o.duration_ms ?? o.duration)
  if (duration === null && o.end_timestamp !== undefined) duration = toMs(o.end_timestamp) - start
  return {
    span_id: String(o.span_id ?? ''),
    parent_span_id: str(o.parent_span_id),
    trace_id: String(o.trace_id ?? traceId),
    project_id: str(o.project_id),
    segment_id: str(o.segment_id ?? o.transaction_span_id),
    is_segment: Boolean(o.is_segment ?? o.is_transaction),
    transaction: str(o.transaction ?? (o.is_segment || o.is_transaction ? o.name : null)),
    op: str(o.op),
    description: str(o.description ?? (o.is_transaction ? o.name : null)),
    status: str(o.status),
    start,
    duration_ms: Math.max(0, duration ?? 0),
    data: obj(o.data),
    tags: strRecord(o.tags),
    profile_id: str(o.profile_id),
  }
}

/** Spans may be flat or nested (`children`); returns them flat. */
function flattenSpans(list: unknown[], out: unknown[] = []): unknown[] {
  for (const s of list) {
    out.push(s)
    const children = obj(s).children
    if (Array.isArray(children)) flattenSpans(children, out)
  }
  return out
}

function normTraceError(raw: unknown): TraceError {
  const o = obj(raw)
  return {
    event_id: String(o.event_id ?? ''),
    issue_id: str(o.issue_id),
    short_id: str(o.short_id),
    title: String(o.title ?? o.message ?? 'Error'),
    level: (str(o.level) ?? 'error') as Level,
    span_id: str(o.span_id),
    project_id: str(o.project_id),
    timestamp: toMs(o.timestamp),
  }
}

export function normTrace(raw: unknown, traceId: string): Trace {
  const o = obj(raw)
  const spans = flattenSpans(arr(o.spans)).map((s) => normSpan(s, traceId))
  const ids = new Set(spans.map((s) => s.span_id))
  // Transactions are segment spans; add those the span list lacks.
  for (const t of arr(o.transactions)) {
    const s = normSpan({ ...obj(t), is_segment: true }, traceId)
    if (!ids.has(s.span_id)) {
      spans.push(s)
      ids.add(s.span_id)
    } else {
      const existing = spans.find((x) => x.span_id === s.span_id)!
      existing.is_segment = true
      existing.transaction ??= s.transaction
      existing.profile_id ??= s.profile_id
    }
  }
  return {
    trace_id: String(o.trace_id ?? traceId),
    spans,
    errors: arr(o.errors).map(normTraceError),
    logs_count: num(o.logs_count),
  }
}

function normSpanGroup(raw: unknown, groupBy: SpanGroupBy): SpanGroupRow {
  const o = obj(raw)
  // The server names the grouped value `group`; earlier drafts used op/description.
  const group = str(o.group)
  const op = str(o.op) ?? (groupBy === 'op' ? group : null)
  const description = str(o.description) ?? (groupBy === 'description' ? group : null)
  return { group: group ?? (groupBy === 'op' ? op : groupBy === 'description' ? description : str(o.transaction)), op, description, count: num(o.count), p50: num(o.p50), p95: num(o.p95), sum: num(o.sum_duration_ms ?? o.sum ?? o.sum_duration ?? o.total) }
}

export interface SpanSample {
  span_id: string
  trace_id: string
  project_id: string
  op: string | null
  description: string | null
  transaction: string | null
  status: string | null
  duration_ms: number
  timestamp: number
}

function normSpanSample(raw: unknown): SpanSample {
  const o = obj(raw)
  return {
    span_id: String(o.span_id ?? ''),
    trace_id: String(o.trace_id ?? ''),
    project_id: String(o.project_id ?? ''),
    op: str(o.op),
    description: str(o.description),
    transaction: str(o.transaction),
    status: str(o.status),
    duration_ms: num(o.duration_ms),
    timestamp: toMs(o.timestamp ?? o.start_timestamp),
  }
}

// ---- logs --------------------------------------------------------------------

const SEVERITY_LEVELS: [number, LogLevel][] = [
  [21, 'fatal'],
  [17, 'error'],
  [13, 'warn'],
  [9, 'info'],
  [5, 'debug'],
  [1, 'trace'],
]

export function normLevel(v: unknown, severity?: number | null): LogLevel {
  const s = String(v ?? '').toLowerCase()
  if (s === 'warning') return 'warn'
  if (s === 'critical') return 'fatal'
  if (['trace', 'debug', 'info', 'warn', 'error', 'fatal'].includes(s)) return s as LogLevel
  if (severity) return SEVERITY_LEVELS.find(([n]) => severity >= n)?.[1] ?? 'info'
  return 'info'
}

/** Typed wire attributes (`{key: {type, value}}`) to plain values. */
function plainAttributes(v: unknown): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [k, x] of Object.entries(obj(v))) {
    const o = obj(x)
    out[k] = 'value' in o && 'type' in o ? o.value : x
  }
  return out
}

export function normLog(raw: unknown, index = 0): LogRow {
  const o = obj(raw)
  const severity = numOrNull(o.severity_number)
  const timestamp = toMs(o.timestamp)
  return {
    id: String(o.id ?? o.log_id ?? `${timestamp}-${index}-${String(o.body ?? '').length}`),
    timestamp,
    project_id: str(o.project_id),
    trace_id: str(o.trace_id),
    span_id: str(o.span_id),
    level: normLevel(o.level, severity),
    severity_number: severity,
    body: String(o.body ?? o.message ?? ''),
    attributes: plainAttributes(o.attributes),
    environment: str(o.environment),
    release: str(o.release),
    sdk: str(o.sdk),
  }
}

/** Histogram as `{interval, data: [{ts, counts|level→n}]}` or `[[ts, {level: n}]]` or `[{ts, level, count}]`. */
export function normLogHistogram(raw: unknown): LogHistogram {
  const o = obj(raw)
  const list = Array.isArray(raw) ? raw : arr(o.data ?? o.buckets ?? o.series)
  const byTs = new Map<number, Partial<Record<LogLevel, number>>>()
  for (const b of list) {
    let ts: number
    let counts: Record<string, unknown>
    if (Array.isArray(b)) {
      ts = toMs(b[0])
      counts = obj(b[1])
    } else {
      const x = obj(b)
      ts = toMs(x.ts ?? x.timestamp ?? x.time)
      counts = x.level !== undefined ? { [String(x.level)]: x.count } : obj(x.counts ?? x.levels ?? x)
    }
    const cur = byTs.get(ts) ?? {}
    for (const [k, v] of Object.entries(counts)) {
      if (['ts', 'timestamp', 'time'].includes(k)) continue
      const level = normLevel(k)
      cur[level] = (cur[level] ?? 0) + num(v)
    }
    byTs.set(ts, cur)
  }
  const buckets = [...byTs.entries()].sort((a, b) => a[0] - b[0]).map(([ts, counts]) => ({ ts, counts }))
  const interval = toIntervalMs(o.interval ?? o.interval_seconds ?? o.interval_ms, o.interval_ms !== undefined)
  return { interval_ms: interval || (buckets.length > 1 ? buckets[1].ts - buckets[0].ts : 60_000), buckets }
}

function toIntervalMs(v: unknown, isMs = false): number {
  if (typeof v === 'string') {
    const m = /^(\d+)(s|m|h|d)$/.exec(v)
    if (m) return Number(m[1]) * { s: 1e3, m: 6e4, h: 3.6e6, d: 8.64e7 }[m[2] as 's']
  }
  const n = num(v)
  return isMs ? n : n * 1000
}

// ---- metrics -----------------------------------------------------------------

function normMetricName(raw: unknown): MetricName {
  const o = obj(raw)
  const t = String(o.type ?? 'counter')
  const type: MetricType = t === 'c' ? 'counter' : t === 'g' ? 'gauge' : t === 'd' ? 'distribution' : t === 's' ? 'set' : (t as MetricType)
  return { name: String(o.name), type, unit: str(o.unit) === 'none' ? null : str(o.unit), last_seen: o.last_seen ? isoOf(o.last_seen) : null, attributes: arr(o.attributes ?? o.attribute_keys).map(String) }
}

function groupLabel(v: unknown): string {
  if (v === null || v === undefined) return ''
  if (typeof v === 'object') return Object.values(v as Raw).map((x) => (x === null ? '(none)' : String(x))).join(', ')
  return String(v)
}

export function normMetricQuery(raw: unknown): MetricQueryResult {
  const o = obj(raw)
  const list = Array.isArray(raw) ? raw : arr(o.series ?? o.data ?? o.groups)
  // A single bare series: [[ts, v], ...]
  if (list.length && Array.isArray(list[0])) {
    return { interval_ms: toIntervalMs(o.interval), series: [{ group: '', data: list.map((p) => [toMs((p as unknown[])[0]), numOrNull((p as unknown[])[1])]) }] }
  }
  const series = list.map((s) => {
    const x = obj(s)
    const data = arr(x.data ?? x.points ?? x.values).map((p) =>
      Array.isArray(p) ? ([toMs(p[0]), numOrNull(p[1])] as [number, number | null]) : ([toMs(obj(p).ts), numOrNull(obj(p).value)] as [number, number | null]),
    )
    return { group: groupLabel(x.group ?? x.by ?? x.key), data }
  })
  const interval = toIntervalMs(o.interval ?? o.interval_seconds ?? o.interval_ms, o.interval_ms !== undefined)
  const first = series[0]?.data
  return { interval_ms: interval || (first && first.length > 1 ? first[1][0] - first[0][0] : 60_000), series }
}

function normWidget(raw: unknown, i: number): Widget {
  const o = obj(raw)
  const kind = (['metric', 'transactions', 'logs', 'issues'].includes(String(o.kind ?? o.type)) ? String(o.kind ?? o.type) : 'metric') as Widget['kind']
  const chart = (['line', 'area', 'bar'].includes(String(o.chart ?? o.display)) ? String(o.chart ?? o.display) : 'line') as Widget['chart']
  return {
    id: String(o.id ?? `w${i}`),
    title: String(o.title ?? ''),
    kind,
    chart,
    query: obj(o.query) as Widget['query'],
    // The server keeps a 12-column `layout`; the UI uses quarters of the width.
    width: Math.min(4, Math.max(1, o.width !== undefined ? num(o.width, 2) : obj(o.layout).w !== undefined ? Math.round(num(obj(o.layout).w, 6) / 3) : 2)),
  }
}

/** A widget as the server stores it (`type`, `chart`, `query`, `layout` on 12 columns). */
export function widgetBody(w: Widget, index: number) {
  return { id: w.id, title: w.title, type: w.kind, chart: w.chart, query: w.query, layout: { x: 0, y: index, w: w.width * 3, h: 4 } }
}

export function normDashboard(raw: unknown): Dashboard {
  const o = obj(raw)
  return {
    id: String(o.id),
    title: String(o.title ?? 'Untitled'),
    description: str(o.description),
    widgets: arr(o.widgets).map(normWidget),
    created_at: isoOf(o.created_at),
    updated_at: isoOf(o.updated_at ?? o.created_at),
  }
}

// ---- profiles ----------------------------------------------------------------

function normProfileRow(raw: unknown): ProfileRow {
  const o = obj(raw)
  return {
    profile_id: String(o.profile_id ?? o.id),
    project_id: String(o.project_id ?? ''),
    kind: o.kind === 'continuous' || o.kind === 'chunk' ? 'continuous' : 'transaction',
    transaction_name: str(o.transaction_name ?? o.transaction),
    trace_id: str(o.trace_id),
    platform: str(o.platform),
    release: str(o.release),
    environment: str(o.environment),
    start: isoOf(o.start ?? o.timestamp ?? o.started),
    duration_ms: num(o.duration_ms ?? o.duration),
  }
}

function normNode(raw: unknown): CallTreeNode {
  const o = obj(raw)
  return {
    frame_index: o.frame_index === null || o.frame_index === undefined ? null : num(o.frame_index),
    self_weight: num(o.self_weight),
    total_weight: num(o.total_weight),
    children: arr(o.children).map(normNode),
  }
}

export function normFlamegraph(raw: unknown): Flamegraph {
  const o = obj(raw)
  const frames: ProfileFrame[] = arr(o.frames).map((f) => {
    const x = obj(f)
    return { name: String(x.name ?? x.function ?? '<anonymous>'), file: str(x.file ?? x.filename), line: numOrNull(x.line ?? x.lineno), package: str(x.package ?? x.module), is_application: Boolean(x.is_application ?? x.in_app) }
  })
  const root = normNode(o.root)
  return { frames, root, samples: num(o.samples, root.total_weight), profiles: num(o.profiles, 1) }
}

// ---- replays -----------------------------------------------------------------

export function normReplay(raw: unknown): ReplayRow {
  const o = obj(raw)
  const u = obj(o.user)
  const started = isoOf(o.started ?? o.started_at)
  const finished = isoOf(o.finished ?? o.finished_at ?? o.started)
  // `duration` is in seconds (rows) unless named duration_ms.
  const duration = o.duration_ms !== undefined ? num(o.duration_ms) : o.duration !== undefined ? num(o.duration) * 1000 : toMs(finished) - toMs(started)
  const strs = (v: unknown) => arr(v).map(String)
  return {
    replay_id: String(o.replay_id ?? o.id),
    project_id: String(o.project_id ?? ''),
    started,
    finished,
    duration_ms: Math.max(0, duration),
    segment_count: num(o.segment_count ?? o.segments),
    urls: strs(o.urls),
    error_ids: strs(o.error_ids),
    trace_ids: strs(o.trace_ids),
    user: {
      id: str(u.id ?? o.user_id),
      email: str(u.email ?? o.user_email),
      name: str(u.name ?? u.username ?? o.user_name),
      ip: str(u.ip ?? u.ip_address ?? o.user_ip),
    },
    browser: str(o.browser),
    os: str(o.os),
    device: str(o.device),
    environment: str(o.environment),
    release: str(o.release),
    count_errors: num(o.count_errors ?? arr(o.error_ids).length),
    count_clicks: num(o.count_clicks),
  }
}

function normReplayError(raw: unknown): ReplayError {
  const o = obj(raw)
  return { event_id: String(o.event_id ?? o.id), issue_id: str(o.issue_id), short_id: str(o.short_id), title: String(o.title ?? 'Error'), timestamp: toMs(o.timestamp) }
}

export function normReplayDetail(raw: unknown): ReplayDetail {
  const o = obj(raw)
  // The server lists `issues: [{id, short_id, title, level, status, event_ids}]`
  // without times; their errors are placed on the timeline from the recording.
  const fromIssues = arr(o.issues).flatMap((i) => {
    const x = obj(i)
    const ids = arr(x.event_ids).map(String)
    return (ids.length ? ids : ['']).slice(0, 1).map((event_id) => ({ event_id, issue_id: str(x.id), short_id: str(x.short_id), title: String(x.title ?? 'Error'), timestamp: toMs(x.timestamp ?? 0) }))
  })
  return { ...normReplay(raw), errors: o.errors ? arr(o.errors).map(normReplayError) : fromIssues }
}

/** Segments as `[[...events], [...events]]`, a flat event array, or a page of either. */
export function normRecording(raw: unknown): RecordingEvent[] {
  const list = listOf(raw)
  const out: RecordingEvent[] = []
  const push = (ev: unknown) => {
    const x = obj(ev)
    if (typeof x.type === 'number') out.push({ type: x.type, data: x.data, timestamp: toMs(x.timestamp) })
  }
  for (const item of list) {
    if (Array.isArray(item)) item.forEach(push)
    else if (Array.isArray(obj(item).events)) arr(obj(item).events).forEach(push)
    else push(item)
  }
  return out.sort((a, b) => a.timestamp - b.timestamp)
}

// ---- endpoints ---------------------------------------------------------------

export interface CommonParams {
  project?: string[]
  environment?: string[]
  period?: TelemetryPeriod | string
  query?: string
}

export interface LogParams extends CommonParams {
  cursor?: string
  limit?: number
  sort?: string
  start?: string
  end?: string
}

export const telemetryApi = {
  performance: {
    transactions: (org: string, params: CommonParams & { sort?: TransactionSort }, signal?: AbortSignal) =>
      request<unknown>(`/orgs/${e(org)}/performance/transactions`, { query: { ...params, limit: 100 }, signal }).then((r) => listOf(r).map(normTransactionRow)),
    summary: (org: string, name: string, params: CommonParams, signal?: AbortSignal) =>
      request<unknown>(`/orgs/${e(org)}/performance/transactions/summary`, { query: { ...params, name }, signal }).then((r) => normSummary(r, name)),
  },
  traces: {
    get: (org: string, traceId: string) => request<unknown>(`/orgs/${e(org)}/traces/${e(traceId)}`).then((r) => normTrace(r, traceId)),
  },
  spans: {
    groups: (org: string, params: CommonParams & { group_by: SpanGroupBy; sort?: string }, signal?: AbortSignal) =>
      request<unknown>(`/orgs/${e(org)}/spans`, { query: { ...params, limit: 100 }, signal }).then((r) =>
        (Array.isArray(obj(r).groups) ? arr(obj(r).groups) : listOf(r)).map((g) => normSpanGroup(g, params.group_by)),
      ),
    /** Matching spans, newest first (`group_by=none`). */
    samples: (org: string, params: CommonParams, signal?: AbortSignal) =>
      request<unknown>(`/orgs/${e(org)}/spans`, { query: { ...params, group_by: 'none', limit: 100 }, signal }).then((r) =>
        (Array.isArray(obj(r).spans) ? arr(obj(r).spans) : listOf(r)).map(normSpanSample),
      ),
  },
  logs: {
    list: (org: string, params: LogParams, signal?: AbortSignal) =>
      request<unknown>(`/orgs/${e(org)}/logs`, { query: { sort: '-timestamp', ...params }, signal }).then((r) => {
        const p = pageOf(r, (v) => v)
        return { data: p.data.map((v, i) => normLog(v, i)), next_cursor: p.next_cursor }
      }),
    histogram: (org: string, params: LogParams, signal?: AbortSignal) =>
      request<unknown>(`/orgs/${e(org)}/logs/histogram`, { query: { ...params }, signal }).then(normLogHistogram),
    streamUrl: (org: string, params: LogParams) => {
      const sp = new URLSearchParams()
      for (const [k, v] of Object.entries(params)) {
        if (v === undefined || v === null || v === '' || (Array.isArray(v) && !v.length)) continue
        sp.set(k, Array.isArray(v) ? v.join(',') : String(v))
      }
      const s = sp.toString()
      return `/api/v1/orgs/${e(org)}/logs/stream${s ? `?${s}` : ''}`
    },
  },
  metrics: {
    names: (org: string, params: { project?: string[]; period?: string }) =>
      request<unknown>(`/orgs/${e(org)}/metrics/names`, { query: params }).then((r) => listOf(r).map(normMetricName)),
    query: (
      org: string,
      params: { name: string; op: MetricOp; group_by?: string; interval?: string; period?: string; project?: string[]; environment?: string[]; query?: string },
      signal?: AbortSignal,
    ) => request<unknown>(`/orgs/${e(org)}/metrics/query`, { query: { ...params }, signal }).then(normMetricQuery),
  },
  dashboards: {
    list: (org: string) => request<unknown>(`/orgs/${e(org)}/dashboards`).then((r) => listOf(r).map(normDashboard)),
    get: (org: string, id: Id) => request<unknown>(`/orgs/${e(org)}/dashboards/${e(id)}`).then(normDashboard),
    create: (org: string, body: { title: string; description?: string | null; widgets: Widget[] }) =>
      request<unknown>(`/orgs/${e(org)}/dashboards`, { method: 'POST', body: { ...body, widgets: body.widgets.map(widgetBody) } }).then(normDashboard),
    update: (org: string, id: Id, body: Partial<{ title: string; description: string | null; widgets: Widget[] }>) =>
      request<unknown>(`/orgs/${e(org)}/dashboards/${e(id)}`, { method: 'PATCH', body: body.widgets ? { ...body, widgets: body.widgets.map(widgetBody) } : body }).then(normDashboard),
    remove: (org: string, id: Id) => request<void>(`/orgs/${e(org)}/dashboards/${e(id)}`, { method: 'DELETE' }),
  },
  profiles: {
    list: (org: string, params: CommonParams & { transaction?: string; cursor?: string; limit?: number }, signal?: AbortSignal) =>
      request<unknown>(`/orgs/${e(org)}/profiles`, { query: { ...params }, signal }).then((r) => pageOf(r, normProfileRow)),
    flamegraph: (org: string, project: string, profileId: string) =>
      request<unknown>(`/orgs/${e(org)}/profiles/${e(project)}/${e(profileId)}/flamegraph`).then(normFlamegraph),
    rawUrl: (org: string, project: string, profileId: string) => `/api/v1/orgs/${e(org)}/profiles/${e(project)}/${e(profileId)}/raw`,
    aggregate: (org: string, params: { transaction: string; project?: string[]; period?: string; environment?: string[] }) =>
      request<unknown>(`/orgs/${e(org)}/profiles/aggregate-flamegraph`, { query: { ...params } }).then(normFlamegraph),
  },
  replays: {
    list: (org: string, params: CommonParams & { sort?: string; cursor?: string; limit?: number }, signal?: AbortSignal) =>
      request<unknown>(`/orgs/${e(org)}/replays`, { query: { ...params }, signal }).then((r) => pageOf(r, normReplay)),
    get: (org: string, project: string, replayId: string) => request<unknown>(`/orgs/${e(org)}/replays/${e(project)}/${e(replayId)}`).then(normReplayDetail),
    /** All segments, following cursors. */
    recording: async (org: string, project: string, replayId: string, signal?: AbortSignal) => {
      const events: RecordingEvent[] = []
      let cursor: string | null = null
      for (let i = 0; i < 200; i++) {
        const body: unknown = await request<unknown>(`/orgs/${e(org)}/replays/${e(project)}/${e(replayId)}/recording-segments`, { query: { cursor: cursor ?? undefined }, signal })
        events.push(...normRecording(body))
        cursor = Array.isArray(body) ? null : str(obj(body).next_cursor)
        if (!cursor) break
      }
      return events.sort((a, b) => a.timestamp - b.timestamp)
    },
    forIssue: (issueId: Id) => request<unknown>(`/issues/${e(issueId)}/replays`).then((r) => listOf(r).map(normReplay)),
  },
}

// ---- query keys & hooks ------------------------------------------------------

export const tk = {
  transactions: (org: string, params: object) => ['perf', org, 'transactions', params] as const,
  summary: (org: string, name: string, params: object) => ['perf', org, 'summary', name, params] as const,
  trace: (org: string, traceId: string) => ['traces', org, traceId] as const,
  traceLogs: (org: string, traceId: string) => ['traces', org, traceId, 'logs'] as const,
  spans: (org: string, params: object) => ['spans', org, params] as const,
  logs: (org: string, params: object) => ['logs', org, 'list', params] as const,
  logHistogram: (org: string, params: object) => ['logs', org, 'histogram', params] as const,
  metricNames: (org: string, params: object) => ['metrics', org, 'names', params] as const,
  metricQuery: (org: string, params: object) => ['metrics', org, 'query', params] as const,
  dashboards: (org: string) => ['dashboards', org] as const,
  dashboard: (org: string, id: Id) => ['dashboards', org, id] as const,
  profiles: (org: string, params: object) => ['profiles', org, 'list', params] as const,
  flamegraph: (org: string, project: string, id: string) => ['profiles', org, 'flamegraph', project, id] as const,
  aggregate: (org: string, params: object) => ['profiles', org, 'aggregate', params] as const,
  replays: (org: string, params: object) => ['replays', org, 'list', params] as const,
  replay: (org: string, project: string, id: string) => ['replays', org, project, id] as const,
  recording: (org: string, project: string, id: string) => ['replays', org, project, id, 'recording'] as const,
  issueReplays: (issueId: Id) => ['issues', 'detail', issueId, 'replays'] as const,
}

export const traceQuery = (org: string, traceId: string) =>
  queryOptions({ queryKey: tk.trace(org, traceId), queryFn: () => telemetryApi.traces.get(org, traceId), staleTime: 60_000 })

export function useTransactions(org: string, params: CommonParams & { sort?: TransactionSort }) {
  return useQuery({
    queryKey: tk.transactions(org, params),
    queryFn: ({ signal }) => telemetryApi.performance.transactions(org, params, signal),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  })
}

export function useMetricNames(org: string, params: { project?: string[]; period?: string }) {
  return useQuery({ queryKey: tk.metricNames(org, params), queryFn: () => telemetryApi.metrics.names(org, params), staleTime: 60_000 })
}

export const LIST_PAGE = 50

export function useInfiniteList<T>(key: readonly unknown[], fn: (cursor: string | undefined, signal: AbortSignal) => Promise<Page<T>>, enabled = true) {
  return useInfiniteQuery({
    queryKey: key,
    queryFn: ({ pageParam, signal }) => fn(pageParam ?? undefined, signal),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.next_cursor,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
    enabled,
  })
}
