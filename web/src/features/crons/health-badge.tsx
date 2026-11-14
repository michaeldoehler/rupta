import type { Monitor } from '@/api/types'
import { cn } from '@/lib/utils'

const STYLE: Record<string, [string, string]> = {
  ok: ['OK', 'bg-success-soft text-success'],
  error: ['Failing', 'bg-destructive/12 text-destructive'],
  missed: ['Missed', 'bg-warning-soft text-warning'],
  timeout: ['Timed out', 'bg-destructive/12 text-destructive'],
  unknown: ['Waiting', 'bg-muted text-muted-foreground'],
  disabled: ['Disabled', 'bg-muted text-muted-foreground'],
}

export function HealthBadge({ monitor, className }: { monitor: Pick<Monitor, 'health' | 'status'>; className?: string }) {
  const [label, cls] = STYLE[monitor.status === 'disabled' ? 'disabled' : monitor.health] ?? STYLE.unknown
  return <span className={cn('inline-flex items-center gap-1 rounded-sm px-1.5 py-px text-[11px] font-semibold', cls, className)}>{label}</span>
}
