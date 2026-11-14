import type { Level } from '@/api/types'
import { cn } from '@/lib/utils'

export const LEVEL_COLOR: Record<string, string> = {
  fatal: 'bg-level-fatal',
  error: 'bg-level-error',
  warning: 'bg-level-warning',
  info: 'bg-level-info',
  debug: 'bg-level-debug',
  log: 'bg-level-debug',
  sample: 'bg-level-sample',
}

export const LEVEL_TEXT: Record<string, string> = {
  fatal: 'text-level-fatal',
  error: 'text-level-error',
  warning: 'text-warning',
  info: 'text-level-info',
  debug: 'text-level-debug',
  log: 'text-muted-foreground',
}

/** Vertical bar used at the start of issue rows. */
export function LevelBar({ level, className }: { level: Level | string; className?: string }) {
  return <span aria-hidden className={cn('block w-[3px] shrink-0 rounded-full', LEVEL_COLOR[level] ?? 'bg-level-debug', className)} />
}

export function LevelDot({ level, className }: { level: Level | string; className?: string }) {
  return <span aria-hidden className={cn('inline-block size-2 shrink-0 rounded-full', LEVEL_COLOR[level] ?? 'bg-level-debug', className)} />
}

export function LevelBadge({ level, className }: { level: Level | string; className?: string }) {
  return (
    <span className={cn('inline-flex items-center gap-1.5 rounded-sm border px-1.5 py-px text-[11px] font-medium capitalize', className)}>
      <LevelDot level={level} />
      {level}
    </span>
  )
}
