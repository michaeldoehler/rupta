import { useQuery, useQueryClient } from '@tanstack/react-query'
import { AreaChart, BarChart3, LayoutDashboard, LineChart, Plus, Search, Sigma } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { telemetryApi, tk, useMetricNames } from '@/api/telemetry'
import type { ChartType, MetricName, MetricOp, Widget } from '@/api/telemetry-types'
import { Field } from '@/components/common/field'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { cn } from '@/lib/utils'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { TimeSeriesChart } from '@/features/telemetry/charts'
import { MAX_SERIES, SERIES_COLORS, withUnit } from '@/features/telemetry/format'
import { ExploreHeader, Panel, SectionTabs } from '@/features/telemetry/ui'
import { OP_LABEL, OPS_BY_TYPE, STACKABLE, useMetricSeries } from './widget-view'

export const METRICS_TABS = (org: string) => [
  { to: '/$org/metrics' as const, params: { org }, label: 'Explorer', exact: true },
  { to: '/$org/dashboards' as const, params: { org }, label: 'Dashboards' },
]

const TYPE_STYLE: Record<string, string> = { counter: 'brand', gauge: 'secondary', distribution: 'outline', set: 'outline' }

export interface MetricsState {
  name?: string
  op?: MetricOp
  group_by?: string
  chart?: ChartType
}

/** Attribute to group by: suggestions (attribute keys, environment, release) or any key typed. */
function GroupByInput({ value, options, onCommit, ...rest }: { id: string; value: string; options: string[]; onCommit: (v: string) => void }) {
  const [draft, setDraft] = useState(value)
  useEffect(() => setDraft(value), [value])
  const listId = `${rest.id}-options`
  return (
    <>
      <Input
        {...rest}
        list={listId}
        value={draft}
        placeholder="No grouping"
        className="w-44 font-mono text-[13px]"
        onChange={(e) => {
          setDraft(e.target.value)
          if (options.includes(e.target.value)) onCommit(e.target.value)
        }}
        onBlur={() => draft.trim() !== value && onCommit(draft.trim())}
        onKeyDown={(e) => e.key === 'Enter' && onCommit(draft.trim())}
      />
      <datalist id={listId}>
        {[...new Set(options)].map((o) => (
          <option key={o} value={o} />
        ))}
      </datalist>
    </>
  )
}

function MetricList({ names, value, onPick }: { names: MetricName[]; value?: string; onPick: (n: MetricName) => void }) {
  const [filter, setFilter] = useState('')
  const shown = names.filter((n) => n.name.toLowerCase().includes(filter.toLowerCase()))
  return (
    <div className="flex h-full flex-col">
      <div className="border-b p-2">
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Find a metric"
            className="h-7 pl-7 text-[13px]"
            aria-label="Find a metric"
          />
        </div>
      </div>
      <ul className="min-h-0 flex-1 overflow-y-auto p-1" role="listbox" aria-label="Metrics">
        {shown.map((n) => (
          <li key={n.name} role="option" aria-selected={n.name === value}>
            <button
              type="button"
              onClick={() => onPick(n)}
              className={cn(
                'flex w-full flex-col gap-0.5 rounded-md px-2 py-1.5 text-left hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none',
                n.name === value && 'bg-accent',
              )}
            >
              <span className="truncate font-mono text-[12.5px] font-medium">{n.name}</span>
              <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                <Badge variant={TYPE_STYLE[n.type] as 'brand'}>{n.type}</Badge>
                {n.unit && <span>{n.unit}</span>}
                {n.last_seen && (
                  <span className="ml-auto">
                    <RelativeTime date={n.last_seen} plain />
                  </span>
                )}
              </span>
            </button>
          </li>
        ))}
        {shown.length === 0 && <li className="px-2 py-4 text-center text-xs text-muted-foreground">No metrics match</li>}
      </ul>
    </div>
  )
}

function AddToDashboard({ org, widget, open, onOpenChange }: { org: string; widget: Widget; open: boolean; onOpenChange: (o: boolean) => void }) {
  const qc = useQueryClient()
  const dashboards = useQuery({ queryKey: tk.dashboards(org), queryFn: () => telemetryApi.dashboards.list(org), enabled: open })
  const [target, setTarget] = useState<string>('new')
  const [title, setTitle] = useState(widget.title)
  const [newTitle, setNewTitle] = useState('')
  const [saving, setSaving] = useState(false)
  async function save() {
    setSaving(true)
    try {
      const w = { ...widget, title: title.trim() || widget.title }
      if (target === 'new') await telemetryApi.dashboards.create(org, { title: newTitle.trim() || 'New dashboard', widgets: [w] })
      else {
        const d = dashboards.data?.find((x) => x.id === target)
        if (d) await telemetryApi.dashboards.update(org, d.id, { widgets: [...d.widgets, w] })
      }
      void qc.invalidateQueries({ queryKey: tk.dashboards(org) })
      toast.success('Added to the dashboard')
      onOpenChange(false)
    } catch (e) {
      toast.error('Could not add the widget', { description: errorMessage(e) })
    } finally {
      setSaving(false)
    }
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add to a dashboard</DialogTitle>
          <DialogDescription>
            The widget keeps this metric, aggregation, grouping and chart type; the dashboard sets the time range and projects.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <Field label="Widget title">{(p) => <Input {...p} value={title} onChange={(e) => setTitle(e.target.value)} />}</Field>
          <Field label="Dashboard">
            {(p) => (
              <Select value={target} onValueChange={setTarget}>
                <SelectTrigger id={p.id} className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="new">New dashboard…</SelectItem>
                  {dashboards.data?.map((d) => (
                    <SelectItem key={d.id} value={d.id}>
                      {d.title}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </Field>
          {target === 'new' && (
            <Field label="New dashboard title">
              {(p) => <Input {...p} value={newTitle} onChange={(e) => setNewTitle(e.target.value)} placeholder="e.g. Checkout health" />}
            </Field>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={() => void save()} disabled={saving}>
            Add widget
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export function MetricsPage({ state, onChange }: { state: MetricsState; onChange: (s: MetricsState) => void }) {
  const { org, projects, environments, period } = useGlobalFilters()
  const names = useMetricNames(org, { project: projects, period })
  const meta = names.data?.find((n) => n.name === state.name) ?? (state.name ? undefined : names.data?.[0])
  const name = meta?.name ?? state.name
  const ops = meta ? OPS_BY_TYPE[meta.type] : (['sum'] as MetricOp[])
  const op: MetricOp = state.op && ops.includes(state.op) ? state.op : ops[0]
  const chart: ChartType = state.chart ?? (meta?.type === 'counter' ? 'bar' : 'line')
  const groupBy = state.group_by ?? ''
  const q = useMetricSeries({ org, project: projects, environment: environments, period }, { name, op, group_by: groupBy }, Boolean(meta))
  const unit = op === 'count' ? null : (meta?.unit ?? null)
  const [adding, setAdding] = useState(false)

  const summary = useMemo(
    () =>
      (q.data?.series ?? []).map((s) => {
        const vals = s.data.map(([, v]) => v).filter((v): v is number => v !== null)
        const sum = vals.reduce((a, b) => a + b, 0)
        return {
          group: s.group,
          min: vals.length ? Math.min(...vals) : null,
          max: vals.length ? Math.max(...vals) : null,
          avg: vals.length ? sum / vals.length : null,
          total: sum,
          last: vals.at(-1) ?? null,
        }
      }),
    [q.data],
  )

  const widget: Widget = {
    id: `w${Date.now().toString(36)}`,
    title: `${OP_LABEL[op]} of ${name ?? ''}${groupBy ? ` by ${groupBy}` : ''}`,
    kind: 'metric',
    chart,
    query: { name, op, group_by: groupBy || undefined },
    width: 2,
  }

  return (
    <div className="flex h-full min-h-[560px] flex-col">
      <ExploreHeader title="Metrics" tabs={<SectionTabs label="Metrics sections" items={METRICS_TABS(org)} />} />
      {names.isPending ? (
        <ListSkeleton rows={8} />
      ) : names.isError ? (
        <ErrorState error={names.error} retry={() => names.refetch()} />
      ) : names.data.length === 0 ? (
        <EmptyState
          icon={<Sigma />}
          title="No metrics yet"
          description="Send counters, gauges, distributions or sets with your SDK's metrics API; they are listed here once received."
        />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col lg:flex-row">
          <aside className="hidden w-72 shrink-0 border-r lg:block" aria-label="Metric picker">
            <MetricList
              names={names.data}
              value={name}
              onPick={(n) => onChange({ ...state, name: n.name, op: undefined, group_by: undefined, chart: undefined })}
            />
          </aside>
          <div className="min-w-0 flex-1 space-y-3 overflow-y-auto p-3 sm:p-5">
            <div className="flex flex-wrap items-end gap-2" role="toolbar" aria-label="Metric query">
              <div className="w-full lg:hidden">
                <Field label="Metric">
                  {(p) => (
                    <Select value={name} onValueChange={(v) => onChange({ ...state, name: v, op: undefined, group_by: undefined })}>
                      <SelectTrigger id={p.id} className="w-full font-mono">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {names.data.map((n) => (
                          <SelectItem key={n.name} value={n.name} className="font-mono">
                            {n.name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  )}
                </Field>
              </div>
              <Field label="Aggregation">
                {(p) => (
                  <Select value={op} onValueChange={(v) => onChange({ ...state, name, op: v as MetricOp })}>
                    <SelectTrigger id={p.id} className="w-36">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {ops.map((o) => (
                        <SelectItem key={o} value={o}>
                          {OP_LABEL[o]}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              </Field>
              <Field label="Group by">
                {(p) => (
                  <GroupByInput
                    {...p}
                    value={groupBy}
                    options={['environment', 'release', ...(meta?.attributes ?? [])]}
                    onCommit={(v) => onChange({ ...state, name, group_by: v || undefined })}
                  />
                )}
              </Field>
              <div className="grid gap-1.5">
                <span className="text-[13px] leading-none font-medium">Chart</span>
                <ToggleGroup
                  type="single"
                  value={chart}
                  onValueChange={(v) => v && onChange({ ...state, name, chart: v as ChartType })}
                  aria-label="Chart type"
                  className="h-8"
                >
                  <ToggleGroupItem value="line" aria-label="Line chart">
                    <LineChart />
                  </ToggleGroupItem>
                  <ToggleGroupItem value="area" aria-label="Area chart">
                    <AreaChart />
                  </ToggleGroupItem>
                  <ToggleGroupItem value="bar" aria-label="Bar chart">
                    <BarChart3 />
                  </ToggleGroupItem>
                </ToggleGroup>
              </div>
              <Button variant="outline" className="ml-auto" onClick={() => setAdding(true)} disabled={!name}>
                <LayoutDashboard /> Add to dashboard
              </Button>
            </div>
            <Panel
              title={
                <span className="font-mono">
                  {OP_LABEL[op]}({name}){groupBy && <span className="text-muted-foreground"> by {groupBy}</span>}
                </span>
              }
              description={meta ? `${meta.type}${meta.unit ? ` · ${meta.unit}` : ''}` : undefined}
            >
              {q.isError ? (
                <ErrorState error={q.error} retry={() => q.refetch()} />
              ) : (
                <TimeSeriesChart
                  series={(q.data?.series ?? []).map((s) => ({ name: s.group || name || '', data: s.data }))}
                  type={chart}
                  loading={q.isPending}
                  height={300}
                  format={(v) => withUnit(v, unit)}
                  stacked={chart !== 'line' && STACKABLE.includes(op)}
                  label={`${OP_LABEL[op]} of ${name}`}
                />
              )}
            </Panel>
            {summary.length > 0 && (
              <Panel
                title="Series"
                description={summary.length > MAX_SERIES ? `The chart shows the first ${MAX_SERIES} of ${summary.length} groups.` : undefined}
                bodyClassName="p-0"
              >
                <div className="overflow-x-auto">
                  <table className="w-full text-[13px]" aria-label="Series summary">
                    <thead className="text-xs text-muted-foreground">
                      <tr className="border-b">
                        <th className="px-3 py-1.5 text-left font-medium">{groupBy || 'Series'}</th>
                        <th className="px-3 py-1.5 text-right font-medium">Min</th>
                        <th className="px-3 py-1.5 text-right font-medium">Avg</th>
                        <th className="px-3 py-1.5 text-right font-medium">Max</th>
                        {STACKABLE.includes(op) && <th className="px-3 py-1.5 text-right font-medium">Total</th>}
                        <th className="px-3 py-1.5 text-right font-medium">Last</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y">
                      {summary.map((s, i) => (
                        <tr key={s.group || i}>
                          <td className="px-3 py-1.5">
                            <span className="flex items-center gap-2 font-mono text-[12.5px]">
                              {i < MAX_SERIES ? (
                                <span className="size-2 rounded-[2px]" style={{ background: SERIES_COLORS[i] }} />
                              ) : (
                                <span className="size-2" />
                              )}
                              {s.group || name}
                              {i >= MAX_SERIES && <span className="font-sans text-[11px] text-muted-foreground">not charted</span>}
                            </span>
                          </td>
                          <td className="px-3 py-1.5 text-right tabular">{withUnit(s.min, unit)}</td>
                          <td className="px-3 py-1.5 text-right tabular">{withUnit(s.avg, unit)}</td>
                          <td className="px-3 py-1.5 text-right tabular">{withUnit(s.max, unit)}</td>
                          {STACKABLE.includes(op) && <td className="px-3 py-1.5 text-right tabular">{withUnit(s.total, unit)}</td>}
                          <td className="px-3 py-1.5 text-right font-medium tabular">{withUnit(s.last, unit)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </Panel>
            )}
            <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <Plus className="size-3" /> Tip: group by an attribute to compare routes, hosts or plans; sums and counts stack in area and bar charts.
            </p>
          </div>
        </div>
      )}
      {adding && <AddToDashboard org={org} widget={widget} open={adding} onOpenChange={setAdding} />}
    </div>
  )
}
