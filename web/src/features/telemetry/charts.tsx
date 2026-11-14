// Time series and histogram charts for the telemetry pages (recharts, in
// the lazily loaded route chunks only). Recessive axes, one y-axis, a
// legend for two or more series, crosshair tooltip.

import { memo, useMemo, type ReactNode } from 'react'
import { Area, AreaChart, Bar, BarChart, CartesianGrid, Line, LineChart, ReferenceLine, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import type { ChartType } from '@/api/telemetry-types'
import { Skeleton } from '@/components/ui/skeleton'
import { absolute } from '@/lib/format'
import { cn } from '@/lib/utils'
import { MAX_SERIES, SERIES_COLORS } from './format'

export interface ChartSeries {
  name: string
  data: [number, number | null][]
  color?: string
}

const AXIS = { fontSize: 11, fill: 'var(--muted-foreground)' }

function tickTime(span: number) {
  return (ts: number) => {
    const d = new Date(ts)
    if (span <= 2 * 86_400_000) return d.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false })
    return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
  }
}

function ChartTooltip({ active, payload, label, format, names }: { active?: boolean; payload?: { dataKey?: unknown; value?: unknown; color?: string }[]; label?: unknown; format: (v: number) => string; names: string[] }) {
  if (!active || !payload?.length) return null
  const rows = payload.filter((p) => p.value !== null && p.value !== undefined).sort((a, b) => Number(b.value) - Number(a.value))
  return (
    <div className="min-w-36 rounded-md border bg-popover px-2.5 py-1.5 text-xs shadow-md">
      <div className="mb-1 text-muted-foreground">{absolute(Number(label))}</div>
      {rows.map((p) => {
        const i = Number(String(p.dataKey).slice(1))
        return (
          <div key={String(p.dataKey)} className="flex items-center gap-2">
            <span className="size-2 shrink-0 rounded-[2px]" style={{ background: p.color }} />
            <span className="min-w-0 flex-1 truncate">{names[i] || 'value'}</span>
            <span className="font-medium tabular">{format(Number(p.value))}</span>
          </div>
        )
      })}
    </div>
  )
}

export function Legend({ items, className }: { items: { name: string; color: string }[]; className?: string }) {
  if (items.length < 2) return null
  return (
    <ul className={cn('flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground', className)} aria-label="Legend">
      {items.map((it) => (
        <li key={it.name} className="flex min-w-0 items-center gap-1.5">
          <span className="size-2 shrink-0 rounded-[2px]" style={{ background: it.color }} />
          <span className="truncate">{it.name || '(none)'}</span>
        </li>
      ))}
    </ul>
  )
}

export const TimeSeriesChart = memo(function TimeSeriesChart({
  series,
  type = 'line',
  format = (v) => String(v),
  height = 200,
  stacked = false,
  loading = false,
  label,
  markers = [],
  legend = true,
}: {
  series: ChartSeries[]
  type?: ChartType
  format?: (v: number) => string
  height?: number
  stacked?: boolean
  loading?: boolean
  /** Accessible description, e.g. "Throughput per minute". */
  label: string
  /** Vertical reference lines (deploys, incidents). */
  markers?: { ts: number; label: string }[]
  legend?: boolean
}) {
  const shown = series.slice(0, MAX_SERIES)
  const names = shown.map((s) => s.name)
  const colors = shown.map((s, i) => s.color ?? SERIES_COLORS[i])
  const rows = useMemo(() => {
    const byTs = new Map<number, Record<string, number | null>>()
    shown.forEach((s, i) => {
      for (const [ts, v] of s.data) {
        let r = byTs.get(ts)
        if (!r) byTs.set(ts, (r = { ts }))
        r[`s${i}`] = v
      }
    })
    return [...byTs.values()].sort((a, b) => (a.ts as number) - (b.ts as number))
  }, [shown])
  const span = rows.length > 1 ? (rows[rows.length - 1].ts as number) - (rows[0].ts as number) : 0
  if (loading) return <Skeleton className="w-full" style={{ height }} />
  if (!rows.length)
    return (
      <div className="flex items-center justify-center rounded-md border border-dashed text-xs text-muted-foreground" style={{ height }}>
        No data in this period
      </div>
    )
  const common = {
    data: rows,
    margin: { top: 6, right: 4, bottom: 0, left: 0 },
  }
  const axes = (
    <>
      <CartesianGrid vertical={false} stroke="var(--border)" strokeDasharray="3 3" />
      <XAxis dataKey="ts" type="number" scale="time" domain={['dataMin', 'dataMax']} tickFormatter={tickTime(span)} tick={AXIS} tickLine={false} axisLine={false} minTickGap={40} />
      <YAxis tickFormatter={(v: number) => format(v)} tick={AXIS} tickLine={false} axisLine={false} width={56} allowDecimals />
      <Tooltip content={<ChartTooltip format={format} names={names} />} cursor={type === 'bar' ? { fill: 'var(--accent)' } : { stroke: 'var(--muted-foreground)', strokeDasharray: '3 3' }} isAnimationActive={false} />
      {markers.map((m) => (
        <ReferenceLine key={m.ts} x={m.ts} stroke="var(--muted-foreground)" strokeDasharray="2 3" label={{ value: m.label, position: 'insideTopLeft', fontSize: 10, fill: 'var(--muted-foreground)' }} />
      ))}
    </>
  )
  let chart: ReactNode
  if (type === 'bar') {
    chart = (
      <BarChart {...common} barCategoryGap={1}>
        {axes}
        {shown.map((_, i) => (
          <Bar key={i} dataKey={`s${i}`} stackId={stacked ? 'a' : undefined} fill={colors[i]} radius={stacked && i < shown.length - 1 ? 0 : [2, 2, 0, 0]} isAnimationActive={false} />
        ))}
      </BarChart>
    )
  } else if (type === 'area') {
    chart = (
      <AreaChart {...common}>
        {axes}
        {shown.map((_, i) => (
          <Area key={i} dataKey={`s${i}`} type="monotone" stackId={stacked ? 'a' : undefined} stroke={colors[i]} strokeWidth={1.5} fill={colors[i]} fillOpacity={stacked ? 0.35 : 0.14} connectNulls isAnimationActive={false} dot={false} />
        ))}
      </AreaChart>
    )
  } else {
    chart = (
      <LineChart {...common}>
        {axes}
        {shown.map((_, i) => (
          <Line key={i} dataKey={`s${i}`} type="monotone" stroke={colors[i]} strokeWidth={2} dot={false} activeDot={{ r: 4, strokeWidth: 2, stroke: 'var(--card)' }} connectNulls isAnimationActive={false} />
        ))}
      </LineChart>
    )
  }
  return (
    <div className="min-w-0">
      <div style={{ height }} role="img" aria-label={label}>
        <ResponsiveContainer width="100%" height="100%">
          {chart}
        </ResponsiveContainer>
      </div>
      {legend && <Legend className="mt-2" items={shown.map((s, i) => ({ name: s.name, color: colors[i] }))} />}
    </div>
  )
})

export function HistogramChart({
  buckets,
  height = 180,
  format,
  lines = [],
  label,
}: {
  buckets: { start: number; end: number; count: number }[]
  height?: number
  format: (v: number) => string
  lines?: { value: number; label: string }[]
  label: string
}) {
  const data = buckets.map((b) => ({ x: (b.start + b.end) / 2, count: b.count, start: b.start, end: b.end }))
  if (!data.length) return <div className="flex items-center justify-center text-xs text-muted-foreground" style={{ height }}>No data</div>
  return (
    <div style={{ height }} role="img" aria-label={label}>
      <ResponsiveContainer width="100%" height="100%">
        <BarChart data={data} margin={{ top: 14, right: 8, bottom: 0, left: 0 }} barCategoryGap={1}>
          <CartesianGrid vertical={false} stroke="var(--border)" strokeDasharray="3 3" />
          <XAxis dataKey="x" type="number" domain={['dataMin', 'dataMax']} tickFormatter={format} tick={AXIS} tickLine={false} axisLine={false} minTickGap={30} />
          <YAxis tick={AXIS} tickLine={false} axisLine={false} width={44} allowDecimals={false} />
          <Tooltip
            isAnimationActive={false}
            cursor={{ fill: 'var(--accent)' }}
            content={({ active, payload }) =>
              active && payload?.length ? (
                <div className="rounded-md border bg-popover px-2 py-1 text-xs shadow-md">
                  <div className="font-medium tabular">{Number(payload[0].payload.count).toLocaleString('en-US')} transactions</div>
                  <div className="text-muted-foreground">
                    {format(payload[0].payload.start as number)} – {format(payload[0].payload.end as number)}
                  </div>
                </div>
              ) : null
            }
          />
          <Bar dataKey="count" fill="var(--series-2)" radius={[2, 2, 0, 0]} isAnimationActive={false} />
          {lines.map((l) => (
            <ReferenceLine key={l.label} x={l.value} stroke="var(--brand)" strokeDasharray="3 3" label={{ value: l.label, position: 'top', fontSize: 10, fill: 'var(--muted-foreground)' }} />
          ))}
        </BarChart>
      </ResponsiveContainer>
    </div>
  )
}
