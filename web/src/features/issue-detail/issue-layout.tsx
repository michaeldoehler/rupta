import { useQuery } from '@tanstack/react-query'
import { Link, Outlet, useNavigate } from '@tanstack/react-router'
import { Bookmark, ChevronRight, Ellipsis, Share2, Trash2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { api } from '@/api/endpoints'
import { qk, useDeleteIssues, useIssue, useSession, useUpdateIssues } from '@/api/queries'
import type { Assignee, IssueDetail } from '@/api/types'
import { CopyButton } from '@/components/common/copy-button'
import { LevelBadge } from '@/components/common/level'
import { PlatformIcon } from '@/components/common/platform-icon'
import { ErrorState } from '@/components/common/states'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Skeleton } from '@/components/ui/skeleton'
import { Tip } from '@/components/ui/tooltip'
import { copyText } from '@/hooks/use-copy'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { compact, shortVersion, thousands } from '@/lib/format'
import { cn } from '@/lib/utils'
import { type ActionHandler } from '@/features/issues/actions'
import { StatusBadge } from '@/features/issues/issue-row'
import { IssueContextPanel } from './context-panel'

function PanelSkeleton() {
  return (
    <div className="space-y-3 p-4 xl:border-l" aria-busy="true" aria-label="Loading issue context">
      <Skeleton className="h-8 w-full" />
      <Skeleton className="h-24 w-full" />
      <Skeleton className="h-16 w-full" />
    </div>
  )
}

function HeaderSkeleton() {
  return (
    <div className="space-y-3 border-b px-4 py-4 sm:px-6" aria-busy="true" aria-label="Loading issue">
      <Skeleton className="h-3.5 w-48" />
      <Skeleton className="h-6 w-2/3" />
      <Skeleton className="h-4 w-1/3" />
      <div className="flex gap-2">
        <Skeleton className="h-7 w-24" />
        <Skeleton className="h-7 w-24" />
        <Skeleton className="h-7 w-32" />
      </div>
    </div>
  )
}

export function IssueLayout({ org, issueId }: { org: string; issueId: string }) {
  const issue = useIssue(issueId)
  const navigate = useNavigate()
  const { data: session } = useSession()
  const update = useUpdateIssues(org)
  const del = useDeleteIssues(org)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const activity = useQuery({ queryKey: qk.issues.activity(issueId), queryFn: () => api.issues.activity(issueId), enabled: issue.isSuccess })
  const comments = activity.data?.filter((a) => a.kind === 'comment').length ?? 0

  // Mark as seen once per visit.
  const seenRef = useRef<string | null>(null)
  useEffect(() => {
    if (issue.data && !issue.data.has_seen && seenRef.current !== issueId) {
      seenRef.current = issueId
      void api.issues.update(issueId, { has_seen: true }).catch(() => undefined)
    }
  }, [issue.data, issueId])

  const act: ActionHandler = (u, message, opts) => update.mutate({ ids: [issueId], update: u, message, assignee: opts?.assignee, undo: opts?.undo })
  const d = issue.data
  const me: Assignee | null = session ? { type: 'user', id: session.user.id, name: session.user.name || session.user.email } : null
  const permalink = `${location.origin}/${org}/issues/${issueId}`

  useHotkeys(
    {
      e: () =>
        d &&
        act(
          d.status === 'resolved' ? { status: 'unresolved' } : { status: 'resolved' },
          d.status === 'resolved' ? 'Issue marked unresolved' : 'Issue resolved',
          { undo: { status: d.status } },
        ),
      i: () =>
        d &&
        act(
          d.status === 'ignored' ? { status: 'unresolved' } : { status: 'ignored', substatus: 'archived_until_escalating' },
          d.status === 'ignored' ? 'Issue restored' : 'Issue ignored',
        ),
      a: () => me && act({ assignee: { type: 'user', id: me.id } }, 'Assigned to you', { assignee: me }),
      b: () => d && act({ is_bookmarked: !d.is_bookmarked }, d.is_bookmarked ? 'Bookmark removed' : 'Bookmarked'),
      c: () => d && void copyText(d.short_id, `Copied ${d.short_id}`),
      Escape: () => void navigate({ to: '/$org/issues', params: { org } }),
    },
    [d, me, org],
  )

  if (issue.isError) return <ErrorState error={issue.error} retry={() => issue.refetch()} className="min-h-[60vh]" />

  return (
    <div className="flex min-h-full flex-col">
      {!d ? <HeaderSkeleton /> : <Header d={d} org={org} act={act} permalink={permalink} onDelete={() => setConfirmDelete(true)} />}
      <nav aria-label="Issue sections" className="flex gap-1 overflow-x-auto border-b px-3 sm:px-5">
        {(
          [
            ['/$org/issues/$issueId', 'Details', undefined],
            ['/$org/issues/$issueId/events', 'Events', d ? compact(d.count) : undefined],
            ['/$org/issues/$issueId/tags', 'Tags', undefined],
            ['/$org/issues/$issueId/activity', 'Activity', comments || undefined],
            ['/$org/issues/$issueId/feedback', 'User feedback', undefined],
            ['/$org/issues/$issueId/replays', 'Replays', undefined],
          ] as const
        ).map(([to, label, badge]) => (
          <Link
            key={to}
            to={to}
            params={{ org, issueId }}
            activeOptions={{ exact: to === '/$org/issues/$issueId', includeSearch: false }}
            className="flex shrink-0 items-center gap-1.5 border-b-2 border-transparent px-2.5 py-2 text-[13px] font-medium text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
            activeProps={{ className: 'border-brand! text-foreground', 'aria-current': 'page' }}
          >
            {label}
            {badge !== undefined && <span className="rounded-full bg-muted px-1.5 text-[11px] text-muted-foreground tabular">{badge}</span>}
          </Link>
        ))}
      </nav>
      <div className="grid flex-1 xl:grid-cols-[minmax(0,1fr)_360px]">
        <div className="order-2 min-w-0 xl:order-1">
          <Outlet />
        </div>
        <div className="order-1 border-b xl:order-2 xl:border-b-0">{d ? <IssueContextPanel d={d} org={org} act={act} /> : <PanelSkeleton />}</div>
      </div>
      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this issue?</AlertDialogTitle>
            <AlertDialogDescription>
              All of its events are deleted. If the error happens again, a new issue is created. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction destructive onClick={() => del.mutate([issueId], { onSuccess: () => void navigate({ to: '/$org/issues', params: { org } }) })}>
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

function Header({ d, org, act, permalink, onDelete }: { d: IssueDetail; org: string; act: ActionHandler; permalink: string; onDelete: () => void }) {
  const t = d.metadata.type
  return (
    <header className="border-b px-4 pt-3 pb-4 sm:px-6">
      <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
        <Link to="/$org/issues" params={{ org }} className="hover:text-foreground">
          Issues
        </Link>
        <ChevronRight className="size-3" />
        <span className="inline-flex items-center gap-1.5">
          <PlatformIcon platform={d.project.platform} size="sm" /> {d.project.slug}
        </span>
        <ChevronRight className="size-3" />
        <span className="inline-flex items-center font-mono font-medium text-foreground">
          {d.short_id}
          <CopyButton value={d.short_id} label="Copy short id (c)" toastLabel={`Copied ${d.short_id}`} />
        </span>
        <div className="ml-auto flex items-center gap-1">
          <Tip content={d.is_bookmarked ? 'Remove bookmark (b)' : 'Bookmark (b)'}>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-pressed={d.is_bookmarked}
              aria-label="Bookmark"
              onClick={() => act({ is_bookmarked: !d.is_bookmarked }, d.is_bookmarked ? 'Bookmark removed' : 'Bookmarked')}
            >
              <Bookmark className={cn(d.is_bookmarked && 'fill-brand text-brand')} />
            </Button>
          </Tip>
          <Tip content="Copy link">
            <Button variant="ghost" size="icon-sm" aria-label="Copy link to this issue" onClick={() => void copyText(permalink, 'Link copied')}>
              <Share2 />
            </Button>
          </Tip>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon-sm" aria-label="More issue actions">
                <Ellipsis />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onSelect={() => void copyText(d.title, 'Title copied')}>Copy title</DropdownMenuItem>
              <DropdownMenuItem onSelect={() => void copyText(`[${d.short_id}] ${d.title}\n${permalink}`, 'Copied as text')}>Copy as text</DropdownMenuItem>
              <DropdownMenuItem onSelect={() => void copyText(`[${d.short_id}: ${d.title}](${permalink})`, 'Markdown link copied')}>
                Copy as Markdown link
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem variant="destructive" onSelect={onDelete}>
                <Trash2 /> Delete issue…
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      <div className="mt-2 min-w-0 space-y-2">
        <div className="flex flex-wrap items-center gap-2">
          <LevelBadge level={d.level} />
          <StatusBadge issue={d} />
          {d.status === 'resolved' && d.resolved_in_release && (
            <span className="text-xs text-muted-foreground">
              in <span className="font-mono">{d.resolved_in_release === 'next' ? 'the next release' : shortVersion(d.resolved_in_release)}</span>
            </span>
          )}
          <span className="text-xs text-muted-foreground tabular">
            {thousands(d.count)} events · {thousands(d.user_count)} users
          </span>
        </div>
        <h1 className="text-xl leading-tight font-semibold tracking-tight break-words">
          {t ? (
            <>
              {t}
              <span className="font-normal text-foreground/80">: {d.metadata.value}</span>
            </>
          ) : (
            d.title
          )}
        </h1>
        {d.culprit && <p className="font-mono text-[12.5px] break-all text-muted-foreground">{d.culprit}</p>}
      </div>
    </header>
  )
}
