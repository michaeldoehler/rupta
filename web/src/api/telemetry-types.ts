// View types for performance, traces, logs, metrics, profiles and session
// replays. The normalisers in telemetry.ts map server responses to these.

import type { Id, Iso, Level } from './types'

export type TelemetryPeriod = '1h' | '24h' | '7d' | '14d' | '30d' | '90d'

// --- performance --------------------------------------------------------------

export type TransactionSort = 'tpm' | 'p50' | 'p95' | 'p99' | 'failure_rate' | 'count'

export interface TransactionRow {
  name: string
  op: string | null
  project_id: Id | null
  count: number
  /** Transactions per minute over the period. */
  tpm: number
  p50: number
  p75: number
  p95: number
  p99: number
  /** 0..1 */
  failure_rate: number
  /** 0..1, threshold 300 ms. */
  apdex: number
}

export interface SeriesPoint {
  /** Epoch milliseconds. */
  ts: number
  count: number
  p50: number
  p95: number
}

export interface HistogramBucket {
  /** Bucket start and end in milliseconds. */
  start: number
  end: number
  count: number
}

export interface TransactionSample {
  trace_id: string
  span_id: string | null
  event_id: string | null
  project_id: Id | null
  duration_ms: number
  timestamp: Iso
  status: string | null
  release: string | null
  user: string | null
  profile_id: string | null
  replay_id: string | null
}

export interface RelatedIssue {
  id: Id
  short_id: string
  title: string
  level: Level
  count: number
}

export type VitalName = 'lcp' | 'fcp' | 'cls' | 'inp' | 'ttfb'

export interface VitalValue {
  p75: number | null
  count: number
}

export interface TransactionSummary {
  name: string
  op: string | null
  totals: Pick<TransactionRow, 'count' | 'tpm' | 'p50' | 'p95' | 'failure_rate' | 'apdex'> | null
  series: SeriesPoint[]
  histogram: HistogramBucket[]
  slowest: TransactionSample[]
  related_issues: RelatedIssue[]
  vitals: Partial<Record<VitalName, VitalValue>>
}

export interface TraceSpan {
  span_id: string
  parent_span_id: string | null
  trace_id: string
  project_id: Id | null
  /** Span id of the transaction (segment) the span belongs to. */
  segment_id: string | null
  is_segment: boolean
  /** Transaction name, for segment spans. */
  transaction: string | null
  op: string | null
  description: string | null
  status: string | null
  /** Start, epoch milliseconds. */
  start: number
  duration_ms: number
  data: Record<string, unknown>
  tags: Record<string, string>
  profile_id: string | null
}

export interface TraceError {
  event_id: string
  issue_id: Id | null
  short_id: string | null
  title: string
  level: Level
  span_id: string | null
  project_id: Id | null
  timestamp: number
}

export interface TraceLogMarker {
  span_id: string | null
  timestamp: number
  level: string
  body: string
}

export interface Trace {
  trace_id: string
  spans: TraceSpan[]
  errors: TraceError[]
  logs_count: number
}

export type SpanGroupBy = 'op' | 'description'

export interface SpanGroupRow {
  op: string | null
  description: string | null
  count: number
  p50: number
  p95: number
  sum: number
}

// --- logs ---------------------------------------------------------------------

export type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error' | 'fatal'

export interface LogRow {
  /** Stable key in the list (server id, or derived). */
  id: string
  timestamp: number
  project_id: Id | null
  trace_id: string | null
  span_id: string | null
  level: LogLevel
  severity_number: number | null
  body: string
  attributes: Record<string, unknown>
  environment: string | null
  release: string | null
  sdk: string | null
}

export interface LogHistogram {
  interval_ms: number
  buckets: { ts: number; counts: Partial<Record<LogLevel, number>> }[]
}

// --- metrics ------------------------------------------------------------------

export type MetricType = 'counter' | 'gauge' | 'distribution' | 'set'
export type MetricOp = 'sum' | 'avg' | 'min' | 'max' | 'count' | 'p50' | 'p75' | 'p95' | 'p99'

export interface MetricName {
  name: string
  type: MetricType
  unit: string | null
  last_seen: Iso | null
}

export interface MetricSeries {
  /** Group label ("" when not grouped). */
  group: string
  data: [number, number | null][]
}

export interface MetricQueryResult {
  interval_ms: number
  series: MetricSeries[]
}

export type ChartType = 'line' | 'area' | 'bar'

export type WidgetKind = 'metric' | 'transactions' | 'logs' | 'issues'

export interface WidgetQuery {
  /** metric */
  name?: string
  op?: MetricOp
  group_by?: string
  /** free-form filter, e.g. log or issue query */
  query?: string
  /** transactions: which value to plot */
  field?: 'count' | 'p50' | 'p95'
  /** transactions: transaction name */
  transaction?: string
}

export interface Widget {
  id: string
  title: string
  kind: WidgetKind
  chart: ChartType
  query: WidgetQuery
  /** Columns of a 4-column grid the widget spans (1–4). */
  width: number
}

export interface Dashboard {
  id: Id
  title: string
  description: string | null
  widgets: Widget[]
  created_at: Iso
  updated_at: Iso
}

// --- profiles -----------------------------------------------------------------

export interface ProfileRow {
  profile_id: string
  project_id: Id
  kind: 'transaction' | 'continuous'
  transaction_name: string | null
  trace_id: string | null
  platform: string | null
  release: string | null
  environment: string | null
  start: Iso
  duration_ms: number
}

export interface ProfileFrame {
  name: string
  file: string | null
  line: number | null
  package: string | null
  is_application: boolean
}

export interface CallTreeNode {
  frame_index: number | null
  self_weight: number
  total_weight: number
  children: CallTreeNode[]
}

export interface Flamegraph {
  frames: ProfileFrame[]
  root: CallTreeNode
  samples: number
  profiles: number
}

// --- replays ------------------------------------------------------------------

export interface ReplayRow {
  replay_id: string
  project_id: Id
  started: Iso
  finished: Iso
  duration_ms: number
  segment_count: number
  urls: string[]
  error_ids: string[]
  trace_ids: string[]
  user: { id: string | null; email: string | null; name: string | null; ip: string | null }
  browser: string | null
  os: string | null
  device: string | null
  environment: string | null
  release: string | null
  count_errors: number
  count_clicks: number
}

export interface ReplayError {
  event_id: string
  issue_id: Id | null
  short_id: string | null
  title: string
  timestamp: number
}

export interface ReplayDetail extends ReplayRow {
  errors: ReplayError[]
}

/** An rrweb event (type, data, timestamp in ms). */
export interface RecordingEvent {
  type: number
  data: unknown
  timestamp: number
}

// --- project settings ---------------------------------------------------------

export interface TelemetrySettings {
  retention: { logs: number; spans: number; metrics: number; profiles: number; replays: number }
  sampling: { transactions: number; spans: number; logs: number }
  data_types: { traces: boolean; logs: boolean; metrics: boolean; profiles: boolean; replays: boolean }
}
