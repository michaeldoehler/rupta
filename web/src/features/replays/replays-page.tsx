import { useQuery } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { ArrowDownUp, ChevronRight, Film, Globe, Monitor, MousePointerClick, Route as RouteIcon, Smartphone, TriangleAlert } from 'lucide-react'
import { lazy, Suspense, useCallback, useMemo, useState } from 'react'
import { useProjects } from '@/api/queries'
import { telemetryApi, tk, useInfiniteList } from '@/api/telemetry'
import type { ReplayRow } from '@/api/telemetry-types'
import { UserAvatar } from '@/components/common/avatar'
import { LevelDot } from '@/components/common/level'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Skeleton } from '@/components/ui/skeleton'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { absolute, bytes } from '@/lib/format'
import { quoteValue } from '@/lib/search/parse'
import type { Suggestion } from '@/lib/search/autocomplete'
import { cn } from '@/lib/utils'
import { SearchBar } from '@/features/issues/search-bar'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { ms } from '@/features/telemetry/format'
import { ExploreHeader, InfiniteVirtualList } from '@/features/telemetry/ui'
import { buildMarkers, clock, extractBreadcrumbs, extractConsole, extractNetwork, type Crumb } from './timeline'

const ReplayPlayer = lazy(() => import('./replay-player'))

const SORTS = [
  ['-started', 'Newest'],
  ['-duration', 'Longest'],
  ['-count_errors', 'Most errors'],
  ['-count_clicks', 'Most clicks'],
] as const

const REPLAY_KEYS: { key: string; description: string; values?: string[] }[] = [
  { key: 'url', description: 'A visited URL contains' },
  { key: 'user.email', description: 'User email' },
  { key: 'browser', description: 'Browser name', values: ['Chrome', 'Safari', 'Firefox', 'Edge', 'Mobile Safari'] },
  { key: 'count_errors', description: 'Number of errors, e.g. >0', values: ['>0', '0'] },
]

function suggestReplay(ctx: { mode: 'key' | 'value'; partial: string; key?: string }): Suggestion[] {
  const p = ctx.partial.toLowerCase()
  if (ctx.mode === 'key')
    return REPLAY_KEYS.filter((k) => k.key.startsWith(p)).map((k) => ({
      kind: 'key',
      label: k.key,
      description: k.description,
      insert: `${k.key}:`,
      key: k.key,
    }))
  const vals = REPLAY_KEYS.find((k) => k.key === ctx.key)?.values ?? []
  return vals
    .filter((v) => v.toLowerCase().startsWith(p))
    .map((v) => ({ kind: 'value', label: v, insert: v.startsWith('>') ? v : quoteValue(v), key: ctx.key }))
}

export function DeviceIcon({ device, className }: { device: string | null; className?: string }) {
  const Icon = device && /phone|mobile|tablet|ipad/i.test(device) ? Smartphone : Monitor
  return <Icon className={cn('size-3.5', className)} aria-label={device ?? 'device'} />
}

const COLS = 'grid grid-cols-[minmax(0,1fr)_64px_56px] items-center gap-3 px-4 md:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_96px_72px_64px_64px_150px] sm:px-6'

export function ReplayRowView({ r, org, focused = false }: { r: ReplayRow; org: string; focused?: boolean }) {
  const name = r.user.name || r.user.email || r.user.ip || 'Anonymous user'
  const path = (r.urls[0] ?? '').replace(/^https?:\/\/[^/]+/, '') || '/'
  return (
    <Link
      to="/$org/replays/$project/$replayId"
      params={{ org, project: r.project_id, replayId: r.replay_id }}
      className={cn(COLS, 'h-full border-b text-[13px] hover:bg-row-hover focus-visible:bg-row-hover focus-visible:outline-none', focused && 'bg-row-selected')}
    >
      <span role="gridcell" className="flex min-w-0 items-center gap-2.5">
        <UserAvatar name={name} className="size-7 shrink-0" />
        <span className="min-w-0">
          <span className="block truncate font-medium">{name}</span>
          <span className="block truncate font-mono text-[11.5px] text-muted-foreground md:hidden">{path}</span>
          {r.user.email && r.user.name && <span className="hidden truncate text-xs text-muted-foreground md:block">{r.user.email}</span>}
        </span>
      </span>
      <span role="gridcell" className="hidden min-w-0 md:block">
        <span className="block truncate font-mono text-[12px]">{path}</span>
        <span className="text-[11px] text-muted-foreground">
          {r.urls.length} {r.urls.length === 1 ? 'page' : 'pages'}
        </span>
      </span>
      <span role="gridcell" className="hidden min-w-0 items-center gap-1.5 text-xs text-muted-foreground md:flex">
        <DeviceIcon device={r.device} />
        <span className="truncate">{r.browser ?? '—'}</span>
      </span>
      <span role="gridcell" className="text-right tabular md:text-left">
        {clock(r.duration_ms)}
      </span>
      <span
        role="gridcell"
        className={cn(
          'flex items-center justify-end gap-1 tabular md:justify-start',
          r.count_errors ? 'font-medium text-destructive' : 'text-muted-foreground',
        )}
      >
        <TriangleAlert className="size-3.5" aria-label="errors" />
        {r.count_errors}
      </span>
      <span role="gridcell" className="hidden items-center gap-1 text-muted-foreground tabular md:flex">
        <MousePointerClick className="size-3.5" aria-label="clicks" />
        {r.count_clicks}
      </span>
      <span role="gridcell" className="hidden text-right text-muted-foreground md:block">
        <RelativeTime date={r.started} suffix plain />
      </span>
    </Link>
  )
}

export function ReplayListHeader() {
  return (
    <div
      className={cn(
        COLS,
        'sticky top-0 z-10 hidden h-8 border-b bg-background/95 text-[11px] font-medium tracking-wide text-muted-foreground uppercase backdrop-blur md:grid',
      )}
      role="row"
    >
      <span role="columnheader">User</span>
      <span role="columnheader">Pages</span>
      <span role="columnheader">Browser</span>
      <span role="columnheader">Duration</span>
      <span role="columnheader">Errors</span>
      <span role="columnheader">Clicks</span>
      <span role="columnheader" className="text-right">
        Started
      </span>
    </div>
  )
}

export function ReplaysPage({ query, sort, onChange }: { query: string; sort: string; onChange: (p: { query?: string; sort?: string }) => void }) {
  const { org, projects, environments, period } = useGlobalFilters()
  const navigate = useNavigate()
  const params = { project: projects, environment: environments, period, query, sort }
  const list = useInfiniteList(tk.replays(org, params), (cursor, signal) => telemetryApi.replays.list(org, { ...params, cursor, limit: 50 }, signal))
  const rows = useMemo(() => list.data?.pages.flatMap((p) => p.data) ?? [], [list.data])
  const [focused, setFocused] = useState(-1)
  useHotkeys(
    {
      j: () => setFocused((f) => Math.min(rows.length - 1, f + 1)),
      k: () => setFocused((f) => Math.max(0, f - 1)),
      Enter: () =>
        rows[focused] &&
        void navigate({ to: '/$org/replays/$project/$replayId', params: { org, project: rows[focused].project_id, replayId: rows[focused].replay_id } }),
    },
    [rows, focused],
  )
  return (
    <div className="flex h-full flex-col">
      <ExploreHeader title="Session replays">
        <div className="flex flex-wrap items-center gap-2">
          <SearchBar
            org={org}
            projects={projects}
            value={query}
            onSubmit={(q) => onChange({ query: q || undefined })}
            label="Search replays"
            placeholder="Search replays: url:/checkout user.email:… browser:Safari count_errors:>0"
            suggest={suggestReplay}
            className="min-w-64 flex-1"
          />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" className="font-normal">
                <ArrowDownUp className="opacity-70" /> {SORTS.find(([k]) => k === sort)?.[1] ?? 'Newest'}
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuRadioGroup value={sort} onValueChange={(v) => onChange({ sort: v === '-started' ? undefined : v })}>
                {SORTS.map(([k, label]) => (
                  <DropdownMenuRadioItem key={k} value={k}>
                    {label}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </ExploreHeader>
      {list.isPending ? (
        <ListSkeleton rows={12} />
      ) : list.isError ? (
        <ErrorState error={list.error} retry={() => list.refetch()} />
      ) : rows.length === 0 ? (
        <EmptyState
          icon={<Film />}
          title={query ? 'No replays match' : 'No session replays yet'}
          description={
            query
              ? 'Try another search, a longer period or other projects.'
              : 'Add the replay integration to the browser SDK (replaysSessionSampleRate, replaysOnErrorSampleRate); recordings appear here once sessions end or are flushed.'
          }
        />
      ) : (
        <InfiniteVirtualList
          label="Replays"
          rows={rows}
          rowHeight={52}
          getKey={(r) => r.replay_id}
          hasNextPage={list.hasNextPage}
          fetchNextPage={() => void list.fetchNextPage()}
          isFetchingNextPage={list.isFetchingNextPage}
          focusedIndex={focused}
          header={<ReplayListHeader />}
          render={(r, i) => <ReplayRowView r={r} org={org} focused={i === focused} />}
        />
      )}
    </div>
  )
}

// ---- replay detail -----------------------------------------------------------

function PanelList<T>({
  items,
  empty,
  render,
  activeIndex,
}: {
  items: T[]
  empty: string
  render: (item: T, i: number) => React.ReactNode
  activeIndex: number
}) {
  if (!items.length) return <p className="p-4 text-center text-xs text-muted-foreground">{empty}</p>
  return (
    <ul className="divide-y">
      {items.map((it, i) => (
        <li key={i} className={cn(i === activeIndex && 'bg-row-selected')}>
          {render(it, i)}
        </li>
      ))}
    </ul>
  )
}

function lastIndexAtOrBefore<T extends { offset: number }>(items: T[], t: number) {
  let idx = -1
  for (let i = 0; i < items.length; i++) {
    if (items[i].offset <= t) idx = i
    else break
  }
  return idx
}

const CRUMB_ICON: Record<string, string> = { 'ui.click': 'click', navigation: 'nav', console: 'log', 'ui.input': 'input' }

export function ReplayPage({ project, replayId }: { project: string; replayId: string }) {
  const { org } = useGlobalFilters()
  const { data: projectList } = useProjects(org)
  const meta = useQuery({ queryKey: tk.replay(org, project, replayId), queryFn: () => telemetryApi.replays.get(org, project, replayId) })
  const rec = useQuery({
    queryKey: tk.recording(org, project, replayId),
    queryFn: ({ signal }) => telemetryApi.replays.recording(org, project, replayId, signal),
    staleTime: Infinity,
  })
  const events = rec.data
  const markers = useMemo(() => (events ? buildMarkers(events, meta.data?.errors ?? []) : []), [events, meta.data])
  const crumbs = useMemo(() => (events ? extractBreadcrumbs(events) : []), [events])
  const consoleItems = useMemo(() => (events ? extractConsole(events) : []), [events])
  const network = useMemo(() => (events ? extractNetwork(events) : []), [events])
  const start = events?.[0]?.timestamp ?? 0
  const errors = useMemo(() => (meta.data?.errors ?? []).map((e) => ({ ...e, offset: Math.max(0, e.timestamp - start) })), [meta.data, start])
  const [time, setTime] = useState(0)
  const [seek, setSeek] = useState<{ at: number; nonce: number } | null>(null)
  const jump = useCallback((at: number) => setSeek({ at, nonce: Date.now() }), [])
  const onTime = useCallback((t: number) => setTime(t), [])
  const m = meta.data
  const proj = projectList?.find((p) => p.id === project || p.slug === project)

  if (meta.isError) return <ErrorState error={meta.error} retry={() => meta.refetch()} className="min-h-[60vh]" />

  const crumbRow = (c: Crumb) => (
    <button type="button" onClick={() => jump(c.offset)} className="flex w-full items-start gap-2 px-3 py-1.5 text-left text-[12.5px] hover:bg-row-hover">
      <span className="w-10 shrink-0 pt-px text-[11px] text-muted-foreground tabular">{clock(c.offset)}</span>
      <Badge variant="outline" className="shrink-0 font-mono font-normal">
        {CRUMB_ICON[c.category] ?? c.category}
      </Badge>
      <span className="min-w-0 flex-1 break-words">{c.category === 'navigation' ? String(c.data.to ?? c.message) : c.message || c.category}</span>
    </button>
  )

  return (
    <div>
      <header className="space-y-2 border-b px-3 pt-3 pb-3 sm:px-5">
        <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
          <Link to="/$org/replays" params={{ org }} className="hover:text-foreground">
            Session replays
          </Link>
          <ChevronRight className="size-3" />
          <span className="font-mono text-foreground">{replayId.slice(0, 12)}</span>
          {proj && <span className="ml-1">· {proj.slug}</span>}
        </div>
        {!m ? (
          <div className="space-y-2" aria-busy="true" aria-label="Loading replay">
            <Skeleton className="h-6 w-64" />
            <Skeleton className="h-4 w-96" />
          </div>
        ) : (
          <div className="flex flex-wrap items-center gap-3">
            <UserAvatar name={m.user.name || m.user.email || 'Anonymous'} className="size-9" />
            <div className="min-w-0">
              <h1 className="truncate text-lg font-semibold tracking-tight">{m.user.name || m.user.email || m.user.ip || 'Anonymous user'}</h1>
              <p className="flex flex-wrap items-center gap-x-2 text-[13px] text-muted-foreground">
                {m.user.email && m.user.name && <span>{m.user.email}</span>}
                <span>{absolute(m.started, { seconds: true })}</span>
                <span>
                  · <RelativeTime date={m.started} suffix />
                </span>
              </p>
            </div>
            <div className="ml-auto flex flex-wrap items-center gap-1.5">
              <Badge variant="secondary" className="gap-1">
                <DeviceIcon device={m.device} /> {m.browser ?? 'browser'} · {m.os ?? 'os'}
              </Badge>
              <Badge variant="outline">{clock(m.duration_ms)}</Badge>
              {m.environment && <Badge variant="outline">{m.environment}</Badge>}
              {m.release && (
                <Badge variant="outline" className="font-mono">
                  {m.release}
                </Badge>
              )}
              <Badge variant={m.count_errors ? 'destructive' : 'secondary'}>
                {m.count_errors} {m.count_errors === 1 ? 'error' : 'errors'}
              </Badge>
              {m.trace_ids[0] && (
                <Button asChild variant="outline" size="xs">
                  <Link to="/$org/traces/$traceId" params={{ org, traceId: m.trace_ids[0] }}>
                    <RouteIcon /> Trace
                  </Link>
                </Button>
              )}
            </div>
          </div>
        )}
      </header>
      <div className="grid gap-3 p-3 sm:p-5 xl:grid-cols-[minmax(0,1fr)_400px]">
        <div className="min-w-0">
          {rec.isError ? (
            <ErrorState error={rec.error} retry={() => rec.refetch()} title="Could not load the recording" />
          ) : !events ? (
            <Skeleton className="aspect-video w-full rounded-lg" aria-busy="true" aria-label="Loading recording" />
          ) : events.length < 2 ? (
            <EmptyState
              icon={<Film />}
              title="No recording data"
              description="The replay has metadata but no recording segments (yet). Segments arrive while the session is active."
            />
          ) : (
            <Suspense fallback={<Skeleton className="aspect-video w-full rounded-lg" aria-busy="true" aria-label="Loading player" />}>
              <ReplayPlayer events={events} markers={markers} onTime={onTime} seek={seek} />
            </Suspense>
          )}
          {m && m.urls.length > 0 && (
            <div className="mt-3 rounded-lg border bg-card p-3">
              <h2 className="mb-1.5 flex items-center gap-1.5 text-[13px] font-semibold">
                <Globe className="size-3.5" /> Pages visited
              </h2>
              <ol className="flex flex-wrap gap-1.5 font-mono text-[12px]">
                {m.urls.map((u, i) => (
                  <li key={i} className="rounded border bg-muted/40 px-1.5 py-0.5">
                    {u.replace(/^https?:\/\//, '')}
                  </li>
                ))}
              </ol>
            </div>
          )}
        </div>
        <Tabs defaultValue={m?.count_errors ? 'errors' : 'breadcrumbs'} className="min-w-0 rounded-lg border bg-card">
          <TabsList className="w-full justify-start rounded-none border-b bg-transparent px-2">
            <TabsTrigger value="breadcrumbs">Breadcrumbs</TabsTrigger>
            <TabsTrigger value="console">
              Console {consoleItems.length > 0 && <span className="text-muted-foreground tabular">{consoleItems.length}</span>}
            </TabsTrigger>
            <TabsTrigger value="network">Network {network.length > 0 && <span className="text-muted-foreground tabular">{network.length}</span>}</TabsTrigger>
            <TabsTrigger value="errors">Errors {errors.length > 0 && <span className="text-destructive tabular">{errors.length}</span>}</TabsTrigger>
          </TabsList>
          <div className="max-h-[560px] overflow-y-auto">
            <TabsContent value="breadcrumbs" className="mt-0">
              <PanelList items={crumbs} empty="No breadcrumbs in this recording." activeIndex={lastIndexAtOrBefore(crumbs, time)} render={crumbRow} />
            </TabsContent>
            <TabsContent value="console" className="mt-0">
              <PanelList
                items={consoleItems}
                empty="No console messages were recorded."
                activeIndex={lastIndexAtOrBefore(consoleItems, time)}
                render={(c) => (
                  <button
                    type="button"
                    onClick={() => jump(c.offset)}
                    className={cn(
                      'flex w-full items-start gap-2 px-3 py-1.5 text-left font-mono text-[12px] hover:bg-row-hover',
                      c.level === 'error' && 'bg-destructive/5 text-destructive',
                      c.level === 'warning' && 'bg-warning-soft/60',
                    )}
                  >
                    <span className="w-10 shrink-0 text-[11px] text-muted-foreground tabular">{clock(c.offset)}</span>
                    <span className="w-12 shrink-0 text-[11px] uppercase">{c.level}</span>
                    <span className="min-w-0 flex-1 break-words">{c.message}</span>
                  </button>
                )}
              />
            </TabsContent>
            <TabsContent value="network" className="mt-0">
              <PanelList
                items={network}
                empty="No requests were recorded. Network details need the replay integration's network options."
                activeIndex={lastIndexAtOrBefore(network, time)}
                render={(n) => (
                  <button
                    type="button"
                    onClick={() => jump(n.offset)}
                    className="grid w-full grid-cols-[40px_44px_minmax(0,1fr)_48px_56px] items-center gap-2 px-3 py-1.5 text-left text-[12px] hover:bg-row-hover"
                  >
                    <span className="text-[11px] text-muted-foreground tabular">{clock(n.offset)}</span>
                    <span className="font-mono text-[11px] font-medium">{n.method}</span>
                    <span className="truncate font-mono" title={n.url}>
                      {n.url.replace(/^https?:\/\/[^/]+/, '')}
                    </span>
                    <span className={cn('text-right font-mono tabular', n.status && n.status >= 400 ? 'text-destructive' : 'text-muted-foreground')}>
                      {n.status ?? '—'}
                    </span>
                    <span className="text-right text-muted-foreground tabular" title={n.size !== null ? bytes(n.size) : undefined}>
                      {ms(n.duration)}
                    </span>
                  </button>
                )}
              />
            </TabsContent>
            <TabsContent value="errors" className="mt-0">
              <PanelList
                items={errors}
                empty="No errors were captured during this session."
                activeIndex={-1}
                render={(e) => (
                  <div className="flex items-center gap-2 px-3 py-2 text-[13px]">
                    <button
                      type="button"
                      onClick={() => jump(e.offset)}
                      className="w-10 shrink-0 text-left text-[11px] text-muted-foreground tabular hover:text-foreground"
                      aria-label={`Jump to ${clock(e.offset)}`}
                    >
                      {clock(e.offset)}
                    </button>
                    <LevelDot level="error" />
                    {e.issue_id ? (
                      <Link to="/$org/issues/$issueId" params={{ org, issueId: e.issue_id }} className="min-w-0 flex-1 truncate hover:underline">
                        {e.title}
                      </Link>
                    ) : (
                      <span className="min-w-0 flex-1 truncate">{e.title}</span>
                    )}
                    {e.short_id && <span className="font-mono text-[11px] text-muted-foreground">{e.short_id}</span>}
                  </div>
                )}
              />
            </TabsContent>
          </div>
        </Tabs>
      </div>
    </div>
  )
}

/** The "Replays" tab of an issue: sessions that captured its errors. */
export function IssueReplays({ org, issueId }: { org: string; issueId: string }) {
  const q = useQuery({ queryKey: tk.issueReplays(issueId), queryFn: () => telemetryApi.replays.forIssue(issueId), staleTime: 60_000 })
  if (q.isPending) return <ListSkeleton rows={4} />
  if (q.isError) return <ErrorState error={q.error} retry={() => q.refetch()} />
  if (!q.data.length)
    return (
      <EmptyState
        icon={<Film />}
        title="No replays for this issue"
        description="Replays appear here when the browser SDK's replay integration recorded a session in which this error happened."
      />
    )
  return (
    <div>
      <p className="border-b px-4 py-2 text-[13px] text-muted-foreground sm:px-6">
        {q.data.length} {q.data.length === 1 ? 'session' : 'sessions'} with this error were recorded.
      </p>
      <ReplayListHeader />
      <div role="grid" aria-label="Replays">
        {q.data.map((r) => (
          <div key={r.replay_id} role="row" className="h-[52px]">
            <ReplayRowView r={r} org={org} />
          </div>
        ))}
      </div>
    </div>
  )
}
