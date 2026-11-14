import { Link } from '@tanstack/react-router'
import { Bookmark } from 'lucide-react'
import { memo } from 'react'
import type { Assignee, Issue } from '@/api/types'
import { LevelBar } from '@/components/common/level'
import { PlatformIcon } from '@/components/common/platform-icon'
import { Sparkline } from '@/components/common/sparkline'
import { RelativeTime } from '@/components/common/time'
import { Checkbox } from '@/components/ui/checkbox'
import { Tip } from '@/components/ui/tooltip'
import { compact, thousands } from '@/lib/format'
import { cn } from '@/lib/utils'
import { AssigneeAvatar, AssigneePicker } from './actions'

export function StatusBadge({ issue, className }: { issue: Pick<Issue, 'status' | 'substatus'>; className?: string }) {
  const map: Record<string, [string, string]> = {
    resolved: ['Resolved', 'bg-success-soft text-success'],
    ignored: ['Ignored', 'bg-muted text-muted-foreground'],
    new: ['New', 'bg-brand-soft text-brand-text'],
    regressed: ['Regressed', 'bg-destructive/12 text-destructive'],
    escalating: ['Escalating', 'bg-warning-soft text-warning'],
  }
  const key = issue.status !== 'unresolved' ? issue.status : (issue.substatus ?? '')
  const v = map[key]
  if (!v) return null
  return (
    <span className={cn('inline-flex items-center rounded-sm px-1 text-[10.5px] leading-4 font-semibold tracking-wide uppercase', v[1], className)}>
      {v[0]}
    </span>
  )
}

export const ROW_HEIGHT = 60

interface RowProps {
  issue: Issue
  org: string
  selected: boolean
  focused: boolean
  onToggle: (id: string, shift: boolean) => void
  onFocus: (id: string) => void
  onAssign: (issue: Issue, a: Assignee | null) => void
  statsPeriod: string
}

/** One dense row in the issue stream. Memoised: the list re-renders often. */
export const IssueRow = memo(function IssueRow({ issue, org, selected, focused, onToggle, onFocus, onAssign, statsPeriod }: RowProps) {
  const t = issue.metadata.type
  const value = issue.metadata.value
  const done = issue.status !== 'unresolved'
  const total = issue.stats.reduce((s, [, v]) => s + v, 0)
  return (
    <div
      role="row"
      aria-selected={selected}
      data-focused={focused || undefined}
      data-issue-id={issue.id}
      className={cn(
        'group/row relative flex h-full items-center gap-3 border-b pr-3 pl-0 transition-colors hover:bg-row-hover sm:pr-4',
        selected && 'bg-row-selected hover:bg-row-selected',
        focused && 'shadow-[inset_2px_0_0_var(--brand)]',
        done && 'opacity-60',
      )}
      onMouseDown={() => onFocus(issue.id)}
    >
      <div role="gridcell" className="flex h-full items-center gap-2.5 self-stretch py-2 pl-2 sm:pl-3">
        <LevelBar level={issue.level} className="h-full" />
        <Checkbox
          checked={selected}
          onClick={(e) => {
            e.stopPropagation()
            onToggle(issue.id, e.shiftKey)
          }}
          aria-label={`Select ${issue.short_id}`}
        />
      </div>
      <div role="gridcell" className="min-w-0 flex-1 py-1.5">
        <Link
          to="/$org/issues/$issueId"
          params={{ org, issueId: issue.id }}
          className="block min-w-0 rounded-sm focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          data-issue-link
        >
          <div className="flex min-w-0 items-baseline gap-1.5 text-[13.5px] leading-5">
            {issue.is_bookmarked && <Bookmark className="size-3 shrink-0 self-center fill-brand text-brand" aria-label="Bookmarked" />}
            {t ? (
              <>
                <span className={cn('shrink-0 font-semibold', !issue.has_seen && 'text-foreground')}>{t}</span>
                <span className="truncate text-foreground/80">{value}</span>
              </>
            ) : (
              <span className="truncate font-semibold">{issue.title}</span>
            )}
          </div>
          <div className="mt-0.5 flex min-w-0 items-center gap-2 text-xs leading-4 text-muted-foreground">
            <PlatformIcon platform={issue.project.platform} size="sm" />
            <span className="shrink-0 font-mono text-[11.5px] font-medium text-foreground/70">{issue.short_id}</span>
            <StatusBadge issue={issue} />
            {issue.culprit && <span className="hidden min-w-0 truncate md:inline">{issue.culprit}</span>}
            <span className="ml-auto hidden shrink-0 items-center gap-1 sm:flex lg:hidden">
              <RelativeTime date={issue.last_seen} />
            </span>
          </div>
        </Link>
      </div>
      <div role="gridcell" className="hidden w-24 shrink-0 text-right text-xs text-muted-foreground lg:block">
        <div>
          <RelativeTime date={issue.last_seen} suffix />
        </div>
        <div className="text-[11px] opacity-80">
          <RelativeTime date={issue.first_seen} /> old
        </div>
      </div>
      <div role="gridcell" className="hidden w-[100px] shrink-0 md:block">
        <Sparkline
          data={issue.stats}
          width={100}
          height={26}
          label={`${total} events in the last ${statsPeriod}`}
          className={cn(issue.substatus === 'escalating' && 'text-warning/80', issue.level === 'fatal' && 'text-level-fatal/60')}
        />
      </div>
      <div role="gridcell" className="w-12 shrink-0 text-right text-[13px] font-medium tabular sm:w-16">
        <Tip content={`${thousands(issue.count)} events`}>
          <span>{compact(issue.count)}</span>
        </Tip>
      </div>
      <div role="gridcell" className="hidden w-14 shrink-0 text-right text-[13px] text-muted-foreground tabular sm:block">
        <Tip content={`${thousands(issue.user_count)} users`}>
          <span>{compact(issue.user_count)}</span>
        </Tip>
      </div>
      <div role="gridcell" className="flex w-8 shrink-0 justify-end">
        <AssigneePicker
          org={org}
          value={issue.assignee}
          onChange={(a) => onAssign(issue, a)}
          trigger={
            <button
              type="button"
              className="rounded-full focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              aria-label={issue.assignee ? `Assigned to ${issue.assignee.name}. Change assignee` : 'Assign'}
              title={issue.assignee?.name ?? 'Unassigned'}
            >
              <AssigneeAvatar assignee={issue.assignee} />
            </button>
          }
        />
      </div>
    </div>
  )
})
