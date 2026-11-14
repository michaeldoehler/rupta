// The issue preview of the split view (wide screens): summary, actions and
// the recommended event. A lazy chunk, so the stream itself stays small.

import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ArrowUpRight } from 'lucide-react'
import { eventQuery, useIssue, useUpdateIssues } from '@/api/queries'
import { LevelBadge } from '@/components/common/level'
import { PlatformIcon } from '@/components/common/platform-icon'
import { ErrorState } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'
import { Skeleton } from '@/components/ui/skeleton'
import { thousands } from '@/lib/format'
import { EventView } from '@/features/event/event-view'
import { IssueChart } from '@/features/issue-detail/issue-chart'
import { LinkedTickets } from '@/features/integrations/linked-tickets'
import { IgnoreButton, ResolveButton, type ActionHandler } from './actions'
import { StatusBadge } from './issue-row'

export default function IssuePreview({ org, issueId }: { org: string; issueId: string }) {
  const issue = useIssue(issueId)
  const ev = useQuery(eventQuery(issueId, 'recommended'))
  const update = useUpdateIssues(org)
  const act: ActionHandler = (u, message, opts) => update.mutate({ ids: [issueId], update: u, message, assignee: opts?.assignee, undo: opts?.undo })
  const d = issue.data
  if (issue.isError) return <ErrorState error={issue.error} retry={() => issue.refetch()} />
  return (
    <div data-testid="issue-preview">
      <header className="sticky top-0 z-10 space-y-2 border-b bg-background/95 px-5 pt-4 pb-3 backdrop-blur">
        {!d ? (
          <div className="space-y-2" aria-busy="true" aria-label="Loading preview">
            <Skeleton className="h-4 w-40" />
            <Skeleton className="h-6 w-3/4" />
          </div>
        ) : (
          <>
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <PlatformIcon platform={d.project.platform} size="sm" />
              <span>{d.project.slug}</span>
              <span className="font-mono font-medium text-foreground">{d.short_id}</span>
              <Button asChild variant="outline" size="xs" className="ml-auto">
                <Link to="/$org/issues/$issueId" params={{ org, issueId }}>
                  Open <ArrowUpRight /> <Kbd className="ml-0.5">↵</Kbd>
                </Link>
              </Button>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <LevelBadge level={d.level} />
              <StatusBadge issue={d} />
              <span className="text-xs text-muted-foreground">
                {thousands(d.count)} events · {thousands(d.user_count)} users · last <RelativeTime date={d.last_seen} suffix plain />
              </span>
            </div>
            <h2 className="text-lg leading-snug font-semibold tracking-tight break-words">
              {d.metadata.type ? (
                <>
                  {d.metadata.type}
                  <span className="font-normal text-foreground/80">: {d.metadata.value}</span>
                </>
              ) : (
                d.title
              )}
            </h2>
            <div className="flex flex-wrap gap-2">
              <ResolveButton resolved={d.status === 'resolved'} onAction={act} currentRelease={d.last_release?.version} />
              <IgnoreButton ignored={d.status === 'ignored'} onAction={act} />
            </div>
          </>
        )}
      </header>
      {d && (
        <div className="grid gap-4 border-b px-5 py-3 2xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
          <IssueChart issueId={issueId} />
          <div>
            <h3 className="mb-1.5 text-[11px] font-medium tracking-wide text-subtle-foreground uppercase">Linked tickets</h3>
            <LinkedTickets org={org} issueId={issueId} projectSlug={d.project.slug} />
          </div>
        </div>
      )}
      {ev.data && d ? (
        <EventView event={ev.data} issue={d} org={org} />
      ) : ev.isError ? (
        <ErrorState error={ev.error} retry={() => ev.refetch()} />
      ) : (
        <div className="space-y-3 p-5" aria-busy="true" aria-label="Loading event">
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-48 w-full" />
        </div>
      )}
    </div>
  )
}
