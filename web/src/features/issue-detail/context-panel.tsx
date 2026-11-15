// The sticky right column of an issue: actions, volume, first and last
// seen, user impact, top tags, the current event's context, its trace and
// replay, and the people involved. Sections are separate components so
// more can be added (for example linked tickets) without touching the rest.

import { useQuery } from '@tanstack/react-query'
import { Link, useRouterState } from '@tanstack/react-router'
import { ChevronRight, Film, Route as RouteIcon, UserPlus } from 'lucide-react'
import type { ReactNode } from 'react'
import { api } from '@/api/endpoints'
import { eventQuery, qk } from '@/api/queries'
import type { Assignee, IssueDetail, Priority, RuptaEvent } from '@/api/types'
import { UserAvatar } from '@/components/common/avatar'
import { RelativeTime } from '@/components/common/time'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import { compact, percent, shortVersion, thousands } from '@/lib/format'
import { formatFilter } from '@/lib/search/parse'
import { cn } from '@/lib/utils'
import { AssigneeAvatar, AssigneePicker, IgnoreButton, ResolveButton, type ActionHandler } from '@/features/issues/actions'
import { LinkedTickets } from '@/features/integrations/linked-tickets'
import { IssueChart } from './issue-chart'

const PRIORITY_DOT: Record<Priority, string> = { high: 'bg-level-error', medium: 'bg-level-warning', low: 'bg-level-debug' }
const BAR = ['bg-palm', 'bg-series-2', 'bg-series-3', 'bg-subtle-foreground']

export function PanelSection({ title, children, action, className }: { title: string; children: ReactNode; action?: ReactNode; className?: string }) {
  return (
    <section className={cn('border-b px-4 py-3 last:border-b-0', className)} aria-label={title}>
      <div className="mb-2 flex items-center justify-between gap-2">
        <h2 className="text-[11px] font-medium tracking-wide text-subtle-foreground uppercase">{title}</h2>
        {action}
      </div>
      {children}
    </section>
  )
}

function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <div className="text-[11px] text-muted-foreground">{label}</div>
      <div className="mt-0.5 text-[13px]">{children}</div>
    </div>
  )
}

/** The event the details tab shows (its `event` search parameter). */
function useCurrentEvent(issueId: string) {
  const which = useRouterState({ select: (s) => ((s.location.search as Record<string, unknown>).event as string | undefined) ?? 'recommended' })
  return useQuery({ ...eventQuery(issueId, which), placeholderData: (p) => p })
}

function TopTags({ org, issueId }: { org: string; issueId: string }) {
  const q = useQuery({ queryKey: qk.issues.tags(issueId), queryFn: () => api.issues.tags(issueId), staleTime: 60_000 })
  if (q.isPending) return <Skeleton className="h-24 w-full" />
  const tags = (q.data ?? []).filter((t) => t.top_values.length).slice(0, 5)
  if (!tags.length) return <p className="text-xs text-muted-foreground">No tags.</p>
  return (
    <ul className="space-y-2.5">
      {tags.map((t) => {
        const total = Math.max(1, t.total_values)
        const top = t.top_values[0]
        return (
          <li key={t.key}>
            <div className="mb-1 flex items-baseline justify-between gap-2 text-[12.5px]">
              <Link
                to="/$org/issues/$issueId/tags/$tagKey"
                params={{ org, issueId, tagKey: t.key }}
                className="font-mono text-muted-foreground hover:text-brand"
              >
                {t.key}
              </Link>
              <Link
                to="/$org/issues"
                params={{ org }}
                search={{ query: `is:unresolved ${formatFilter(t.key, top.value)}` }}
                className="min-w-0 truncate hover:underline"
                title={top.value}
              >
                {top.name ?? top.value} <span className="text-muted-foreground tabular">{percent((top.count / total) * 100, 0)}</span>
              </Link>
            </div>
            <div className="flex h-1.5 gap-px overflow-hidden rounded-full bg-muted" aria-hidden>
              {t.top_values.slice(0, 4).map((v, i) => (
                <span key={v.value} className={BAR[i]} style={{ width: `${(v.count / total) * 100}%` }} />
              ))}
            </div>
          </li>
        )
      })}
    </ul>
  )
}

function ContextSummary({ event }: { event: RuptaEvent }) {
  const c = event.contexts ?? {}
  const rows: [string, string][] = []
  const fmt = (o: Record<string, unknown> | undefined, keys: string[]) =>
    o
      ? keys
          .map((k) => o[k])
          .filter(Boolean)
          .join(' ')
      : ''
  if (c.browser) rows.push(['Browser', fmt(c.browser, ['name', 'version'])])
  if (c.os) rows.push(['OS', fmt(c.os, ['name', 'version'])])
  if (c.runtime) rows.push(['Runtime', fmt(c.runtime, ['name', 'version'])])
  if (c.device) rows.push(['Device', fmt(c.device, ['model', 'family', 'arch']) || 'unknown'])
  if (event.environment) rows.push(['Environment', event.environment])
  if (event.user?.email || event.user?.id) rows.push(['User', String(event.user.email ?? event.user.id)])
  if (!rows.length) return <p className="text-xs text-muted-foreground">No context.</p>
  return (
    <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-[12.5px]">
      {rows.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-muted-foreground">{k}</dt>
          <dd className="truncate" title={v}>
            {v}
          </dd>
        </div>
      ))}
    </dl>
  )
}

function Linked({ event, org }: { event: RuptaEvent; org: string }) {
  const trace = (event.trace_id ?? (event.contexts?.trace?.trace_id as string | undefined)) || null
  const span = (event.span_id ?? (event.contexts?.trace?.span_id as string | undefined)) || undefined
  const replay = (event.replay_id ?? (event.contexts?.replay?.replay_id as string | undefined)) || null
  if (!trace && !replay) return <p className="text-xs text-muted-foreground">This event has no trace or replay.</p>
  const cls = 'flex items-center gap-2 rounded-md border bg-background/40 px-2.5 py-1.5 text-[13px] hover:border-palm/60 hover:bg-row-hover'
  return (
    <div className="space-y-1.5">
      {trace && (
        <Link
          to="/$org/traces/$traceId"
          params={{ org, traceId: trace }}
          search={span ? { span } : {}}
          className={cls}
          aria-label={`Trace ${trace.slice(0, 8)}`}
        >
          <RouteIcon className="size-4 text-palm-text" />
          <span className="flex-1">Trace</span>
          <span className="font-mono text-xs text-muted-foreground">{trace.slice(0, 8)}</span>
          <ChevronRight className="size-3.5 text-muted-foreground" />
        </Link>
      )}
      {replay && (
        <Link to="/$org/replays/$project/$replayId" params={{ org, project: String(event.project_id), replayId: replay }} className={cls}>
          <Film className="size-4 text-palm-text" />
          <span className="flex-1">Session replay</span>
          <ChevronRight className="size-3.5 text-muted-foreground" />
        </Link>
      )}
    </div>
  )
}

function Participants({ issueId, assignee }: { issueId: string; assignee: Assignee | null }) {
  const q = useQuery({ queryKey: qk.issues.activity(issueId), queryFn: () => api.issues.activity(issueId), staleTime: 30_000 })
  const people = new Map<string, string>()
  if (assignee?.type === 'user') people.set(assignee.id, assignee.name)
  for (const a of q.data ?? []) if (a.user) people.set(a.user.id, a.user.name || a.user.email)
  if (!people.size) return <p className="text-xs text-muted-foreground">Nobody yet.</p>
  return (
    <ul className="flex flex-wrap gap-1.5">
      {[...people.entries()].slice(0, 12).map(([id, name]) => (
        <li key={id} title={name}>
          <UserAvatar name={name} className="size-6 text-[10px]" />
        </li>
      ))}
    </ul>
  )
}

export function IssueContextPanel({ d, org, act }: { d: IssueDetail; org: string; act: ActionHandler }) {
  const ev = useCurrentEvent(d.id)
  return (
    <aside className="min-w-0 xl:sticky xl:top-0 xl:max-h-[calc(100svh-3rem)] xl:overflow-y-auto" aria-label="Issue context" data-testid="issue-context">
      <div className="xl:border-l">
        <PanelSection title="Status">
          <div className="grid grid-cols-2 gap-2">
            <ResolveButton resolved={d.status === 'resolved'} onAction={act} currentRelease={d.last_release?.version} stretch />
            <IgnoreButton ignored={d.status === 'ignored'} onAction={act} stretch />
          </div>
          <div className="mt-2 grid grid-cols-2 gap-2">
            <AssigneePicker
              org={org}
              value={d.assignee}
              align="start"
              onChange={(a) => act({ assignee: a ? { type: a.type, id: a.id } : null }, a ? `Assigned to ${a.name}` : 'Unassigned', { assignee: a })}
              trigger={
                <Button variant="outline" size="sm" className="min-w-0 justify-start font-normal">
                  {d.assignee ? <AssigneeAvatar assignee={d.assignee} className="size-4" /> : <UserPlus />}
                  <span className="truncate">{d.assignee ? d.assignee.name : 'Assign'}</span>
                </Button>
              }
            />
            <Select value={d.priority} onValueChange={(v) => act({ priority: v as Priority }, `Priority set to ${v}`)}>
              <SelectTrigger size="sm" className="w-full gap-1.5" aria-label="Priority">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {(['high', 'medium', 'low'] as const).map((p) => (
                  <SelectItem key={p} value={p}>
                    <span className={cn('size-2 rounded-full', PRIORITY_DOT[p])} />
                    <span>
                      <span className="capitalize">{p}</span> priority
                    </span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </PanelSection>
        <PanelSection title="Volume">
          <div className="mb-2 grid grid-cols-2 gap-3">
            <Stat label="Events">
              <span className="text-xl font-semibold tabular">{thousands(d.count)}</span>
            </Stat>
            <Stat label="Users affected">
              <span className="text-xl font-semibold tabular">{thousands(d.user_count)}</span>
            </Stat>
          </div>
          <IssueChart issueId={d.id} />
          {d.user_count > 0 && (
            <p className="mt-2 text-xs text-muted-foreground">{compact(Math.round((d.count / d.user_count) * 10) / 10)} events per affected user</p>
          )}
        </PanelSection>
        <PanelSection title="Seen">
          <div className="grid grid-cols-2 gap-3">
            <Stat label="First seen">
              <RelativeTime date={d.first_seen} long />
              {d.first_release && (
                <Link
                  to="/$org/releases/$version"
                  params={{ org, version: d.first_release.version }}
                  className="block truncate font-mono text-xs text-palm-text hover:text-brand hover:underline"
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
                  className="block truncate font-mono text-xs text-palm-text hover:text-brand hover:underline"
                >
                  {shortVersion(d.last_release.version)}
                </Link>
              )}
            </Stat>
          </div>
        </PanelSection>
        <PanelSection title="Linked tickets">
          <LinkedTickets org={org} issueId={d.id} projectSlug={d.project.slug} />
        </PanelSection>
        <PanelSection title="Trace & replay">{ev.data ? <Linked event={ev.data} org={org} /> : <Skeleton className="h-9 w-full" />}</PanelSection>
        <PanelSection
          title="Top tags"
          action={
            <Link to="/$org/issues/$issueId/tags" params={{ org, issueId: d.id }} className="text-xs text-palm-text hover:text-brand">
              All tags
            </Link>
          }
        >
          <TopTags org={org} issueId={d.id} />
        </PanelSection>
        <PanelSection title="This event">{ev.data ? <ContextSummary event={ev.data} /> : <Skeleton className="h-16 w-full" />}</PanelSection>
        <PanelSection title="Participants">
          <Participants issueId={d.id} assignee={d.assignee} />
        </PanelSection>
      </div>
    </aside>
  )
}
