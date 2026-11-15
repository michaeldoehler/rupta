import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  CircleAlert,
  CircleCheck,
  CircleDot,
  CircleSlash,
  Flag,
  Pencil,
  RefreshCw,
  RotateCcw,
  Sparkles,
  Ticket,
  Trash2,
  TrendingUp,
  Unlink,
  UserPlus,
} from 'lucide-react'
import { lazy, Suspense, useState } from 'react'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { qk, useSession } from '@/api/queries'
import type { Activity } from '@/api/types'
import { BrandIcon } from '@/components/brand/brand-icon'
import { UserAvatar } from '@/components/common/avatar'
import { ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

const Markdown = lazy(() => import('./markdown'))

/** "OPS-42 in Jira" as a link with the tracker's mark, from ticket activity data. */
function ticketRef(d: Record<string, unknown>) {
  const key = String(d.external_key ?? d.key ?? d.external_id ?? 'a ticket')
  // The server sends the integration's name as `integration`; older entries carry `integration_name`.
  const where = d.integration_name ?? (typeof d.integration === 'string' ? d.integration : (d.integration as { name?: string } | undefined)?.name)
  const kind = typeof d.kind === 'string' ? d.kind : null
  const label = (
    <>
      {kind && <BrandIcon kind={kind} size="xs" className="mr-1 align-[-3px]" />}
      <span className="font-mono">{key}</span>
      {where ? ` in ${String(where)}` : ''}
    </>
  )
  return typeof d.url === 'string' ? (
    <a href={d.url} target="_blank" rel="noreferrer noopener" className="text-palm-text hover:text-brand">
      {label}
    </a>
  ) : (
    label
  )
}

function describe(a: Activity): { icon: React.ReactNode; text: React.ReactNode } {
  const v = a.data.version as string | undefined
  const assignee = a.data.assignee as { name?: string } | undefined
  switch (a.kind) {
    case 'first_seen':
      return { icon: <Sparkles className="text-brand-text" />, text: 'First seen' }
    case 'set_resolved':
      return { icon: <CircleCheck className="text-success" />, text: 'marked this issue as resolved' }
    case 'set_resolved_in_release':
      return {
        icon: <CircleCheck className="text-success" />,
        text: <>marked this issue as resolved in {v ? <span className="font-mono">{v}</span> : 'the next release'}</>,
      }
    case 'set_unresolved':
      return { icon: <RotateCcw />, text: 'marked this issue as unresolved' }
    case 'set_ignored':
      return { icon: <CircleSlash />, text: 'ignored this issue' }
    case 'set_regression':
      return {
        icon: <TrendingUp className="text-destructive" />,
        text: (
          <>
            Regressed
            {v && (
              <>
                {' '}
                in <span className="font-mono">{v}</span>
              </>
            )}
          </>
        ),
      }
    case 'set_escalating':
      return { icon: <TrendingUp className="text-warning" />, text: 'Escalating: event volume rose sharply' }
    case 'status_change':
      return {
        icon: a.data.to === 'resolved' ? <CircleCheck className="text-success" /> : <RotateCcw />,
        text: (
          <>
            {a.data.to === 'resolved' ? 'resolved' : a.data.to === 'unresolved' ? 'reopened' : `set to ${String(a.data.to ?? 'a new status')}`}
            {a.data.source === 'integration' && <> because {ticketRef(a.data)} changed</>}
          </>
        ),
      }
    case 'ticket_linked':
      return { icon: <Ticket className="text-palm-text" />, text: <>linked {ticketRef(a.data)}</> }
    case 'ticket_unlinked':
      return { icon: <Unlink />, text: <>unlinked {ticketRef(a.data)}</> }
    case 'ticket_status_change':
      return {
        icon: <Ticket className="text-palm-text" />,
        text: (
          <>
            {ticketRef(a.data)} moved to <span className="font-medium">{String(a.data.status ?? a.data.to ?? 'a new status')}</span>
          </>
        ),
      }
    case 'ticket_sync':
      return a.data.ok === false
        ? {
            icon: <CircleAlert className="text-destructive" />,
            text: (
              <>
                could not update {ticketRef(a.data)}: {String(a.data.error ?? 'unknown error')}
              </>
            ),
          }
        : {
            icon: <RefreshCw />,
            text: (
              <>
                updated {ticketRef(a.data)}
                {a.data.action ? ` (${String(a.data.action)})` : ''}
              </>
            ),
          }
    case 'set_ongoing':
      return { icon: <CircleDot />, text: 'Ongoing: no longer new or escalating' }
    case 'assigned':
      return { icon: <UserPlus />, text: <>assigned this issue to {assignee?.name ?? 'someone'}</> }
    case 'unassigned':
      return { icon: <UserPlus />, text: 'unassigned this issue' }
    case 'set_priority':
      return { icon: <Flag />, text: <>set the priority to {String(a.data.priority)}</> }
    default:
      return { icon: <CircleDot />, text: a.kind.replace(/_/g, ' ') }
  }
}

function Comment({ a, issueId, mine }: { a: Activity; issueId: string; mine: boolean }) {
  const qc = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState(a.data.text ?? '')
  const save = useMutation({
    mutationFn: () => api.issues.updateComment(issueId, a.id, text),
    onSuccess: () => {
      setEditing(false)
      void qc.invalidateQueries({ queryKey: qk.issues.activity(issueId) })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const remove = useMutation({
    mutationFn: () => api.issues.deleteComment(issueId, a.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.issues.activity(issueId) }),
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <li className="relative flex gap-3 pb-5" id={`comment-${a.id}`}>
      <UserAvatar name={a.user?.name ?? 'Unknown'} className="relative z-10 size-7 ring-4 ring-background" />
      <div className="min-w-0 flex-1 rounded-lg border bg-card">
        <div className="flex items-center gap-2 border-b px-3 py-1.5 text-xs">
          <span className="font-medium">{a.user?.name ?? 'Unknown'}</span>
          <span className="text-muted-foreground">
            commented <RelativeTime date={a.created_at} suffix />
            {a.data.edited ? ' (edited)' : ''}
          </span>
          {mine && !editing && (
            <span className="ml-auto flex gap-0.5">
              <Button variant="ghost" size="icon-xs" aria-label="Edit comment" onClick={() => setEditing(true)}>
                <Pencil />
              </Button>
              <Button variant="ghost" size="icon-xs" aria-label="Delete comment" onClick={() => remove.mutate()}>
                <Trash2 />
              </Button>
            </span>
          )}
        </div>
        <div className="px-3 py-2">
          {editing ? (
            <div className="space-y-2">
              <Textarea value={text} onChange={(e) => setText(e.target.value)} rows={4} aria-label="Edit comment" />
              <div className="flex gap-2">
                <Button size="sm" onClick={() => save.mutate()} disabled={!text.trim() || save.isPending}>
                  Save
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
                  Cancel
                </Button>
              </div>
            </div>
          ) : (
            <Suspense fallback={<p className="text-[13px] whitespace-pre-wrap">{a.data.text}</p>}>
              <Markdown text={a.data.text ?? ''} />
            </Suspense>
          )}
        </div>
      </div>
    </li>
  )
}

export function ActivityTab({ issueId }: { issueId: string }) {
  const q = useQuery({ queryKey: qk.issues.activity(issueId), queryFn: () => api.issues.activity(issueId) })
  const { data: session } = useSession()
  const qc = useQueryClient()
  const [text, setText] = useState('')
  const [mode, setMode] = useState<'write' | 'preview'>('write')
  const add = useMutation({
    mutationFn: () => api.issues.addComment(issueId, text.trim()),
    onMutate: async () => {
      await qc.cancelQueries({ queryKey: qk.issues.activity(issueId) })
      const prev = qc.getQueryData<Activity[]>(qk.issues.activity(issueId))
      const optimistic: Activity = {
        id: `tmp-${Date.now()}`,
        kind: 'comment',
        user: session?.user ?? null,
        data: { text: text.trim() },
        created_at: new Date().toISOString(),
      }
      qc.setQueryData<Activity[]>(qk.issues.activity(issueId), (old) => [optimistic, ...(old ?? [])])
      return { prev }
    },
    onError: (e, _v, ctx) => {
      qc.setQueryData(qk.issues.activity(issueId), ctx?.prev)
      toast.error('Could not post the comment', { description: errorMessage(e) })
    },
    onSuccess: () => {
      setText('')
      setMode('write')
    },
    onSettled: () => qc.invalidateQueries({ queryKey: qk.issues.activity(issueId) }),
  })

  return (
    <div className="max-w-3xl px-4 py-5 sm:px-6">
      <form
        className="mb-6 rounded-lg border bg-card"
        onSubmit={(e) => {
          e.preventDefault()
          if (text.trim()) add.mutate()
        }}
      >
        <div className="flex items-center justify-between border-b px-2 py-1">
          <ToggleGroup type="single" value={mode} onValueChange={(v) => v && setMode(v as 'write' | 'preview')} className="h-6 border-0 shadow-none">
            <ToggleGroupItem value="write">Write</ToggleGroupItem>
            <ToggleGroupItem value="preview">Preview</ToggleGroupItem>
          </ToggleGroup>
          <span className="text-[11px] text-muted-foreground">Markdown supported</span>
        </div>
        {mode === 'write' ? (
          <Textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === 'Enter' && text.trim()) add.mutate()
            }}
            placeholder="Add a comment: what you found, links, next steps…"
            rows={3}
            className="rounded-none border-0 shadow-none focus-visible:ring-0"
            aria-label="Comment"
          />
        ) : (
          <div className="min-h-[76px] px-3 py-2">
            <Suspense fallback={null}>
              {text.trim() ? <Markdown text={text} /> : <p className="text-[13px] text-muted-foreground">Nothing to preview.</p>}
            </Suspense>
          </div>
        )}
        <div className="flex items-center justify-end gap-2 border-t px-2 py-1.5">
          <span className="text-[11px] text-muted-foreground">⌘ Enter to post</span>
          <Button type="submit" size="sm" disabled={!text.trim() || add.isPending}>
            Comment
          </Button>
        </div>
      </form>
      {q.isPending ? (
        <ListSkeleton rows={4} />
      ) : q.isError ? (
        <ErrorState error={q.error} retry={() => q.refetch()} />
      ) : (
        <ol className="relative before:absolute before:top-2 before:bottom-6 before:left-[13px] before:w-px before:bg-border">
          {q.data.map((a) => {
            if (a.kind === 'comment') return <Comment key={a.id} a={a} issueId={issueId} mine={a.user?.id === session?.user.id} />
            const { icon, text: label } = describe(a)
            return (
              <li key={a.id} className="relative flex items-center gap-3 pb-5 text-[13px]">
                <span className="relative z-10 flex size-7 items-center justify-center rounded-full border bg-card ring-4 ring-background [&_svg]:size-3.5">
                  {icon}
                </span>
                <span className="min-w-0">
                  {a.user && <span className="font-medium">{a.user.name} </span>}
                  <span className="text-muted-foreground">{label}</span>
                </span>
                <span className="ml-auto shrink-0 text-xs text-muted-foreground">
                  <RelativeTime date={a.created_at} suffix />
                </span>
              </li>
            )
          })}
        </ol>
      )}
    </div>
  )
}
