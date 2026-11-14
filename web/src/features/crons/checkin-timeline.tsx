import type { Checkin, CheckinStatus } from '@/api/types'
import { Tip } from '@/components/ui/tooltip'
import { absolute, duration } from '@/lib/format'
import { cn } from '@/lib/utils'

export const CHECKIN_COLOR: Record<CheckinStatus, string> = {
  ok: 'bg-success',
  error: 'bg-level-error',
  missed: 'bg-level-warning',
  timeout: 'bg-level-fatal',
  in_progress: 'bg-level-info',
}

export const CHECKIN_LABEL: Record<CheckinStatus, string> = { ok: 'OK', error: 'Failed', missed: 'Missed', timeout: 'Timed out', in_progress: 'In progress' }

/** Check-ins as ticks, oldest left. `checkins` come newest first. */
export function CheckinTimeline({ checkins, max = 40, className, tall = false }: { checkins: Checkin[]; max?: number; className?: string; tall?: boolean }) {
  const list = checkins.slice(0, max).reverse()
  const pad = Math.max(0, max - list.length)
  return (
    <div
      className={cn('flex items-end gap-[2px]', className)}
      role="img"
      aria-label={`${list.filter((c) => c.status === 'ok').length} of ${list.length} recent check-ins succeeded`}
    >
      {Array.from({ length: pad }, (_, i) => (
        <span key={`p${i}`} className={cn('w-1.5 rounded-[1px] bg-muted', tall ? 'h-6' : 'h-4')} />
      ))}
      {list.map((c) => (
        <Tip
          key={c.id}
          content={
            <span>
              {CHECKIN_LABEL[c.status]} · {absolute(c.created_at)}
              {c.duration_ms != null && ` · ${duration(c.duration_ms)}`}
            </span>
          }
        >
          <span
            className={cn(
              'w-1.5 rounded-[1px]',
              tall ? 'h-6' : 'h-4',
              CHECKIN_COLOR[c.status],
              c.status !== 'ok' && 'opacity-100',
              c.status === 'ok' && 'opacity-80',
            )}
          />
        </Tip>
      ))}
    </div>
  )
}
