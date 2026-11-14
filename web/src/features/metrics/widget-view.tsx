// Renders one dashboard widget (metric, transaction, log or issue query)
// for the shared time range and project selection.

import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { api } from '@/api/endpoints'
import { telemetryApi, tk, useMetricNames } from '@/api/telemetry'
import type { MetricOp, MetricType, Widget } from '@/api/telemetry-types'
import { LevelDot } from '@/components/common/level'
import { ErrorState } from '@/components/common/states'
import { Skeleton } from '@/components/ui/skeleton'
import { compact } from '@/lib/format'
import { LOG_LEVELS } from '@/lib/logs/query'
import { TimeSeriesChart, type ChartSeries } from '@/features/telemetry/charts'
import { LOG_LEVEL_VAR, ms, withUnit } from '@/features/telemetry/format'

export const OPS_BY_TYPE: Record<MetricType, MetricOp[]> = {
  counter: ['sum', 'count', 'avg'],
  gauge: ['avg', 'min', 'max', 'p50', 'p95'],
  distribution: ['avg', 'p50', 'p75', 'p95', 'p99', 'min', 'max', 'sum', 'count'],
  set: ['count_unique', 'count'],
}

export const OP_LABEL: Record<MetricOp, string> = { sum: 'Sum', avg: 'Average', min: 'Minimum', max: 'Maximum', count: 'Count', count_unique: 'Distinct values', p50: 'p50', p75: 'p75', p95: 'p95', p99: 'p99' }

/** Ops that add up across groups, so bars and areas can stack. */
export const STACKABLE: MetricOp[] = ['sum', 'count', 'count_unique']

interface Scope {
  org: string
  project: string[]
  environment: string[]
  period: string
}

export function useMetricSeries(scope: Scope, q: { name?: string; op?: MetricOp; group_by?: string; query?: string }, enabled = true) {
  const params = { name: q.name ?? '', op: q.op ?? 'sum', group_by: q.group_by || undefined, query: q.query || undefined, period: scope.period, project: scope.project, environment: scope.environment }
  return useQuery({
    queryKey: tk.metricQuery(scope.org, params),
    queryFn: ({ signal }) => telemetryApi.metrics.query(scope.org, params, signal),
    enabled: enabled && Boolean(q.name),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  })
}

function MetricWidget({ w, scope, height }: { w: Widget; scope: Scope; height: number }) {
  const names = useMetricNames(scope.org, { project: scope.project, period: scope.period })
  const meta = names.data?.find((m) => m.name === w.query.name)
  const q = useMetricSeries(scope, w.query)
  if (q.isError) return <ErrorState error={q.error} retry={() => q.refetch()} className="py-6" />
  const unit = w.query.op === 'count' ? null : (meta?.unit ?? null)
  const series: ChartSeries[] = (q.data?.series ?? []).map((s) => ({ name: s.group || w.query.name || '', data: s.data }))
  return <TimeSeriesChart series={series} type={w.chart} loading={q.isPending} height={height} format={(v) => withUnit(v, unit)} stacked={w.chart !== 'line' && STACKABLE.includes(w.query.op ?? 'sum')} label={w.title || `${w.query.op} of ${w.query.name}`} />
}

function TransactionWidget({ w, scope, height }: { w: Widget; scope: Scope; height: number }) {
  const name = w.query.transaction ?? ''
  const params = { project: scope.project, environment: scope.environment, period: scope.period }
  const q = useQuery({ queryKey: tk.summary(scope.org, name, params), queryFn: ({ signal }) => telemetryApi.performance.summary(scope.org, name, params, signal), enabled: Boolean(name), staleTime: 30_000, placeholderData: keepPreviousData })
  if (q.isError) return <ErrorState error={q.error} retry={() => q.refetch()} className="py-6" />
  const field = w.query.field ?? 'p95'
  const data = (q.data?.series ?? []).map((p) => [p.ts, p[field]] as [number, number])
  return <TimeSeriesChart series={[{ name: field, data }]} type={w.chart} loading={q.isPending} height={height} format={field === 'count' ? (v) => compact(Math.round(v)) : ms} label={w.title || `${field} of ${name}`} />
}

function LogsWidget({ w, scope, height }: { w: Widget; scope: Scope; height: number }) {
  const params = { project: scope.project, environment: scope.environment, period: scope.period, query: w.query.query ?? '' }
  const q = useQuery({ queryKey: tk.logHistogram(scope.org, params), queryFn: ({ signal }) => telemetryApi.logs.histogram(scope.org, params, signal), staleTime: 30_000, placeholderData: keepPreviousData })
  if (q.isError) return <ErrorState error={q.error} retry={() => q.refetch()} className="py-6" />
  const levels = [...LOG_LEVELS].reverse().filter((l) => (q.data?.buckets ?? []).some((b) => b.counts[l]))
  const series: ChartSeries[] = levels.map((l) => ({ name: l, color: LOG_LEVEL_VAR[l], data: (q.data?.buckets ?? []).map((b) => [b.ts, b.counts[l] ?? 0] as [number, number]) }))
  return <TimeSeriesChart series={series} type={w.chart} loading={q.isPending} height={height} stacked format={(v) => compact(Math.round(v))} label={w.title || 'Log lines'} />
}

function IssuesWidget({ w, scope, height }: { w: Widget; scope: Scope; height: number }) {
  const q = useQuery({
    queryKey: ['issues', 'widget', scope, w.query.query],
    queryFn: ({ signal }) => api.issues.list(scope.org, { project: scope.project, environment: scope.environment, query: w.query.query || 'is:unresolved', sort: 'freq', limit: 6 }, signal),
    staleTime: 30_000,
  })
  if (q.isPending) return <Skeleton className="w-full" style={{ height }} />
  if (q.isError) return <ErrorState error={q.error} retry={() => q.refetch()} className="py-6" />
  if (!q.data.data.length) return <div className="flex items-center justify-center text-xs text-muted-foreground" style={{ height }}>No matching issues</div>
  return (
    <ul className="divide-y overflow-y-auto" style={{ maxHeight: height }}>
      {q.data.data.map((i) => (
        <li key={i.id}>
          <Link to="/$org/issues/$issueId" params={{ org: scope.org, issueId: i.id }} className="flex items-center gap-2 py-1.5 text-[13px] hover:bg-row-hover">
            <LevelDot level={i.level} />
            <span className="min-w-0 flex-1 truncate">{i.title}</span>
            <span className="text-xs text-muted-foreground tabular">{compact(i.count)}</span>
          </Link>
        </li>
      ))}
    </ul>
  )
}

export function WidgetView({ widget, scope, height = 180 }: { widget: Widget; scope: Scope; height?: number }) {
  if (widget.kind === 'transactions') return <TransactionWidget w={widget} scope={scope} height={height} />
  if (widget.kind === 'logs') return <LogsWidget w={widget} scope={scope} height={height} />
  if (widget.kind === 'issues') return <IssuesWidget w={widget} scope={scope} height={height} />
  return <MetricWidget w={widget} scope={scope} height={height} />
}
