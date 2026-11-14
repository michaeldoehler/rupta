import { useQuery } from '@tanstack/react-query'
import { Link, Outlet, useNavigate } from '@tanstack/react-router'
import { Bookmark, ChevronRight, Ellipsis, Share2, Trash2, UserPlus } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { api } from '@/api/endpoints'
import { qk, useDeleteIssues, useIssue, useSession, useUpdateIssues } from '@/api/queries'
import type { Assignee, IssueDetail, Priority } from '@/api/types'
import { CopyButton } from '@/components/common/copy-button'
import { LevelBadge } from '@/components/common/level'
import { PlatformIcon } from '@/components/common/platform-icon'
import { ErrorState } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import { Tip } from '@/components/ui/tooltip'
import { copyText } from '@/hooks/use-copy'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { compact, shortVersion, thousands } from '@/lib/format'
import { cn } from '@/lib/utils'
import { AssigneeAvatar, AssigneePicker, IgnoreButton, ResolveButton, type ActionHandler } from '@/features/issues/actions'
import { StatusBadge } from '@/features/issues/issue-row'
import { IssueChart } from './issue-chart'

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

function Stat({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <div className="text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{label}</div>
      <div className="mt-0.5 text-[13px]">{children}</div>
    </div>
  )
}

const PRIORITY_DOT: Record<Priority, string> = { high: 'bg-level-error', medium: 'bg-level-warning', low: 'bg-level-debug' }

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
      <div className="flex-1">
        <Outlet />
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

      <div className="mt-2 grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(260px,340px)]">
        <div className="min-w-0 space-y-2">
          <div className="flex flex-wrap items-center gap-2">
            <LevelBadge level={d.level} />
            <StatusBadge issue={d} />
            {d.status === 'resolved' && d.resolved_in_release && (
              <span className="text-xs text-muted-foreground">
                in <span className="font-mono">{d.resolved_in_release === 'next' ? 'the next release' : shortVersion(d.resolved_in_release)}</span>
              </span>
            )}
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
          <div className="flex flex-wrap items-center gap-2 pt-1">
            <ResolveButton resolved={d.status === 'resolved'} onAction={act} currentRelease={d.last_release?.version} />
            <IgnoreButton ignored={d.status === 'ignored'} onAction={act} />
            <AssigneePicker
              org={org}
              value={d.assignee}
              align="start"
              onChange={(a) => act({ assignee: a ? { type: a.type, id: a.id } : null }, a ? `Assigned to ${a.name}` : 'Unassigned', { assignee: a })}
              trigger={
                <Button variant="outline" size="sm" className="font-normal">
                  {d.assignee ? <AssigneeAvatar assignee={d.assignee} className="size-4" /> : <UserPlus />}
                  {d.assignee ? d.assignee.name : 'Assign'}
                </Button>
              }
            />
            <Select value={d.priority} onValueChange={(v) => act({ priority: v as Priority }, `Priority set to ${v}`)}>
              <SelectTrigger size="sm" className="w-auto gap-1.5" aria-label="Priority">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {(['high', 'medium', 'low'] as const).map((p) => (
                  <SelectItem key={p} value={p}>
                    <span className={cn('size-2 rounded-full', PRIORITY_DOT[p])} />
                    <span className="capitalize">{p}</span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
        <div className="grid grid-cols-2 gap-x-4 gap-y-3 rounded-lg border bg-card p-3">
          <Stat label="Events">
            <span className="text-lg font-semibold tabular">{thousands(d.count)}</span>
          </Stat>
          <Stat label="Users">
            <span className="text-lg font-semibold tabular">{thousands(d.user_count)}</span>
          </Stat>
          <div className="col-span-2">
            <IssueChart issueId={d.id} />
          </div>
          <Stat label="First seen">
            <RelativeTime date={d.first_seen} long />
            {d.first_release && (
              <Link
                to="/$org/releases/$version"
                params={{ org, version: d.first_release.version }}
                className="block truncate font-mono text-xs text-brand-text hover:underline"
              >
                {shortVersion(d.first_release.version)}
              </Link>
            )}
          </Stat>
          <Stat label="Last seen">
            <RelativeTime date={d.last_seen} long />
            {d.last_release && (
              <Link
                to="/$org/releases/$version"
                params={{ org, version: d.last_release.version }}
                className="block truncate font-mono text-xs text-brand-text hover:underline"
              >
                {shortVersion(d.last_release.version)}
              </Link>
            )}
          </Stat>
        </div>
      </div>
    </header>
  )
}
