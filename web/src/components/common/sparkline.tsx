import { memo } from 'react'
import { cn } from '@/lib/utils'

/**
 * Tiny bar chart for issue rows. Pure SVG, no chart library, so the issue
 * stream stays light and renders thousands of rows quickly.
 */
export const Sparkline = memo(function Sparkline({
  data,
  width = 96,
  height = 24,
  className,
  label,
}: {
  data: [number, number][]
  width?: number
  height?: number
  className?: string
  label?: string
}) {
  const n = data.length
  if (n === 0) return <svg width={width} height={height} className={className} aria-hidden />
  let max = 0
  for (const [, v] of data) if (v > max) max = v
  const gap = n > 20 ? 1 : 2
  const bw = Math.max(1, (width - gap * (n - 1)) / n)
  const total = data.reduce((s, [, v]) => s + v, 0)
  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      className={cn('text-muted-foreground/45', className)}
      role="img"
      aria-label={label ?? `${total} events`}
    >
      <line x1="0" x2={width} y1={height - 0.5} y2={height - 0.5} className="stroke-border" strokeWidth="1" />
      {data.map(([ts, v], i) => {
        const h = max === 0 ? 0 : Math.max(v > 0 ? 1.5 : 0, (v / max) * (height - 2))
        return <rect key={ts} x={i * (bw + gap)} y={height - h - 1} width={bw} height={h} rx={Math.min(1, bw / 2)} fill="currentColor" />
      })}
    </svg>
  )
})
