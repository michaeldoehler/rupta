import { cn } from '@/lib/utils'

/** Crash-free percentage with a colour cue: green ≥ 99.5, amber ≥ 98, red below. */
export function CrashFree({ value, className }: { value: number | null; className?: string }) {
  if (value === null) return <span className="text-muted-foreground">—</span>
  const tone = value >= 99.5 ? 'text-success' : value >= 98 ? 'text-warning' : 'text-destructive'
  const ring = value >= 99.5 ? 'var(--success)' : value >= 98 ? 'var(--warning)' : 'var(--destructive)'
  const pct = Math.max(0, Math.min(100, value))
  return (
    <span className={cn('inline-flex items-center gap-1.5 tabular', className)}>
      <svg viewBox="0 0 16 16" className="size-3.5 -rotate-90" aria-hidden>
        <circle cx="8" cy="8" r="6" fill="none" stroke="var(--border)" strokeWidth="2.5" />
        <circle cx="8" cy="8" r="6" fill="none" stroke={ring} strokeWidth="2.5" strokeDasharray={`${(pct / 100) * 37.7} 37.7`} />
      </svg>
      <span className={cn('font-medium', tone)}>{value.toFixed(2)}%</span>
    </span>
  )
}

export function Adoption({ value }: { value: number | null }) {
  if (value === null) return <span className="text-muted-foreground">—</span>
  return (
    <span className="inline-flex w-full max-w-28 items-center gap-2">
      <span className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
        <span className="block h-full rounded-full bg-chart-2" style={{ width: `${Math.min(100, value)}%` }} />
      </span>
      <span className="w-8 text-right text-xs text-muted-foreground tabular">{Math.round(value)}%</span>
    </span>
  )
}
