import { memo, useMemo, useState } from 'react'
import type { LogHistogram as Histogram, LogLevel } from '@/api/telemetry-types'
import { Skeleton } from '@/components/ui/skeleton'
import { absolute, compact } from '@/lib/format'
import { LOG_LEVELS } from '@/lib/logs/query'
import { LOG_LEVEL_VAR } from '@/features/telemetry/format'

const ORDER: LogLevel[] = [...LOG_LEVELS] // trace at the bottom … fatal on top

/** Stacked log counts per interval and level. Plain SVG (no chart library). */
export const LogHistogram = memo(function LogHistogram({ data, loading, height = 72, onPick }: { data: Histogram | undefined; loading?: boolean; height?: number; onPick?: (start: number, end: number) => void }) {
  const [hover, setHover] = useState<number | null>(null)
  const { buckets, max, totals } = useMemo(() => {
    const b = data?.buckets ?? []
    let m = 0
    const t: Partial<Record<LogLevel, number>> = {}
    for (const x of b) {
      let sum = 0
      for (const l of ORDER) {
        sum += x.counts[l] ?? 0
        t[l] = (t[l] ?? 0) + (x.counts[l] ?? 0)
      }
      if (sum > m) m = sum
    }
    return { buckets: b, max: m, totals: t }
  }, [data])
  if (loading && !data) return <Skeleton className="w-full" style={{ height: height + 20 }} />
  const n = buckets.length
  const W = 1000
  const gap = n > 120 ? 0.6 : 2
  const bw = n ? Math.max(0.6, (W - gap * (n - 1)) / n) : 0
  const total = ORDER.reduce((s, l) => s + (totals[l] ?? 0), 0)
  const h = hover !== null ? buckets[hover] : null
  return (
    <div className="relative">
      <div className="mb-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
        <span className="font-medium text-foreground tabular">{total.toLocaleString('en-US')} lines</span>
        {[...ORDER].reverse().map((l) =>
          totals[l] ? (
            <span key={l} className="flex items-center gap-1">
              <span className="size-2 rounded-[2px]" style={{ background: LOG_LEVEL_VAR[l] }} />
              {l} <span className="tabular">{compact(totals[l]!)}</span>
            </span>
          ) : null,
        )}
      </div>
      <svg
        viewBox={`0 0 ${W} ${height}`}
        preserveAspectRatio="none"
        className="block w-full"
        style={{ height }}
        role="img"
        aria-label={`Log volume over time: ${total} lines`}
        onMouseLeave={() => setHover(null)}
      >
        <line x1={0} x2={W} y1={height - 0.5} y2={height - 0.5} className="stroke-border" vectorEffect="non-scaling-stroke" />
        {buckets.map((b, i) => {
          let y = height - 1
          const x = i * (bw + gap)
          return (
            <g
              key={b.ts}
              onMouseEnter={() => setHover(i)}
              onClick={() => onPick?.(b.ts, b.ts + (data?.interval_ms ?? 60_000))}
              className={onPick ? 'cursor-pointer' : undefined}
              opacity={hover === null || hover === i ? 1 : 0.55}
            >
              <rect x={x} y={0} width={bw + gap} height={height} fill="transparent" />
              {ORDER.map((l) => {
                const c = b.counts[l] ?? 0
                if (!c || !max) return null
                const hh = Math.max(1, (c / max) * (height - 4))
                y -= hh
                return <rect key={l} x={x} y={y} width={bw} height={hh} fill={LOG_LEVEL_VAR[l]} />
              })}
            </g>
          )
        })}
      </svg>
      {h && hover !== null && (
        <div
          className="pointer-events-none absolute top-5 z-10 min-w-36 rounded-md border bg-popover px-2.5 py-1.5 text-xs shadow-md"
          style={{ left: `min(calc(${((hover + 0.5) / n) * 100}% + 8px), calc(100% - 10rem))` }}
        >
          <div className="mb-1 text-muted-foreground">{absolute(h.ts)}</div>
          {[...ORDER].reverse().map((l) =>
            h.counts[l] ? (
              <div key={l} className="flex items-center gap-2">
                <span className="size-2 rounded-[2px]" style={{ background: LOG_LEVEL_VAR[l] }} />
                <span className="flex-1">{l}</span>
                <span className="font-medium tabular">{h.counts[l]!.toLocaleString('en-US')}</span>
              </div>
            ) : null,
          )}
          {onPick && <div className="mt-1 text-[11px] text-muted-foreground">Click to show this interval</div>}
        </div>
      )}
    </div>
  )
})
