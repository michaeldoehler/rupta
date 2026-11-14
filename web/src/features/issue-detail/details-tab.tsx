import { useQuery } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Braces, ChevronFirst, ChevronLast, ChevronLeft, ChevronRight, Download, Sparkles } from 'lucide-react'
import { useState } from 'react'
import { eventQuery, useIssue } from '@/api/queries'
import { CopyButton } from '@/components/common/copy-button'
import { ErrorState } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Tip } from '@/components/ui/tooltip'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { absolute } from '@/lib/format'
import { EventView } from '@/features/event/event-view'
import { downloadJson, JsonDialog } from '@/features/event/json-dialog'

export function DetailsTab({ org, issueId, event: which }: { org: string; issueId: string; event: string }) {
  const issue = useIssue(issueId)
  const ev = useQuery({ ...eventQuery(issueId, which), placeholderData: (prev) => prev })
  const navigate = useNavigate()
  const [json, setJson] = useState(false)
  const go = (e: string | null | undefined) =>
    e &&
    void navigate({
      to: '/$org/issues/$issueId',
      params: { org, issueId },
      search: (prev: Record<string, unknown>) => ({ ...prev, event: e === 'recommended' ? undefined : e }),
      replace: true,
    })
  const e = ev.data

  useHotkeys(
    {
      h: () => go(e?.previous_event_id),
      l: () => go(e?.next_event_id),
      'shift+h': () => go('oldest'),
      'shift+l': () => go('latest'),
    },
    [e],
  )

  if (ev.isError) return <ErrorState error={ev.error} retry={() => ev.refetch()} />

  return (
    <div>
      <div className="flex flex-wrap items-center gap-2 border-b bg-muted/30 px-4 py-2 sm:px-6">
        <div className="flex min-w-0 items-center gap-2 text-[13px]">
          <span className="font-medium">Event</span>
          {e ? (
            <>
              <span className="font-mono text-xs text-muted-foreground">{e.event_id.slice(0, 12)}</span>
              <CopyButton value={e.event_id} label="Copy event id" toastLabel="Event id copied" />
              <span className="hidden text-xs text-muted-foreground sm:inline">
                {absolute(e.timestamp, { seconds: true })} · <RelativeTime date={e.timestamp} suffix />
              </span>
            </>
          ) : (
            <Skeleton className="h-4 w-40" />
          )}
        </div>
        <div className="ml-auto flex items-center gap-1">
          <Tip content="Recommended event: the most recent one with the most complete data">
            <Button variant={which === 'recommended' ? 'secondary' : 'ghost'} size="xs" onClick={() => go('recommended')}>
              <Sparkles /> Recommended
            </Button>
          </Tip>
          <div className="inline-flex rounded-md border bg-card shadow-xs" role="group" aria-label="Event navigation">
            <Tip content="Oldest event (shift+h)">
              <Button
                variant="ghost"
                size="icon-xs"
                className="rounded-none rounded-l-md"
                disabled={!e?.previous_event_id}
                onClick={() => go('oldest')}
                aria-label="Oldest event"
              >
                <ChevronFirst />
              </Button>
            </Tip>
            <Tip content="Previous event (h)">
              <Button
                variant="ghost"
                size="icon-xs"
                className="rounded-none border-l"
                disabled={!e?.previous_event_id}
                onClick={() => go(e?.previous_event_id)}
                aria-label="Previous event"
              >
                <ChevronLeft />
              </Button>
            </Tip>
            <Tip content="Next event (l)">
              <Button
                variant="ghost"
                size="icon-xs"
                className="rounded-none border-l"
                disabled={!e?.next_event_id}
                onClick={() => go(e?.next_event_id)}
                aria-label="Next event"
              >
                <ChevronRight />
              </Button>
            </Tip>
            <Tip content="Latest event (shift+l)">
              <Button
                variant="ghost"
                size="icon-xs"
                className="rounded-none rounded-r-md border-l"
                disabled={!e?.next_event_id}
                onClick={() => go('latest')}
                aria-label="Latest event"
              >
                <ChevronLast />
              </Button>
            </Tip>
          </div>
          <Tip content="View JSON">
            <Button variant="ghost" size="icon-xs" disabled={!e} onClick={() => setJson(true)} aria-label="View event JSON">
              <Braces />
            </Button>
          </Tip>
          <Tip content="Download JSON">
            <Button variant="ghost" size="icon-xs" disabled={!e} onClick={() => e && downloadJson(e)} aria-label="Download event JSON">
              <Download />
            </Button>
          </Tip>
        </div>
      </div>
      {e && issue.data ? (
        <div className={ev.isPlaceholderData ? 'opacity-60 transition-opacity' : undefined}>
          <EventView event={e} issue={issue.data} org={org} related={false} />
          <JsonDialog event={e} open={json} onOpenChange={setJson} />
        </div>
      ) : (
        <div className="space-y-4 p-6" aria-busy="true" aria-label="Loading event">
          <Skeleton className="h-20 w-full" />
          <Skeleton className="h-64 w-full" />
          <Skeleton className="h-40 w-full" />
        </div>
      )}
    </div>
  )
}
