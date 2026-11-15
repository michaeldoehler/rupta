import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import { ArrowUpRight, ChevronRight, CircleAlert, Filter, Minus, Pause, Play, Plus, Radio, Route as RouteIcon, ScrollText, X } from 'lucide-react'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useProjects } from '@/api/queries'
import { telemetryApi, tk, traceQuery, useInfiniteList, type LogParams } from '@/api/telemetry'
import type { LogRow } from '@/api/telemetry-types'
import { LevelDot } from '@/components/common/level'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/components/ui/sheet'
import { Tip } from '@/components/ui/tooltip'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { absolute } from '@/lib/format'
import { computeFacets, matchesLog, parseLogQuery, suggestLogKeys, suggestLogValues, toggleLogFilter, type Facet } from '@/lib/logs/query'
import type { CompletionContext } from '@/lib/search/autocomplete'
import { cn } from '@/lib/utils'
import { SearchBar, type SearchBarHandle } from '@/features/issues/search-bar'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { LOG_LEVEL_DOT, LOG_LEVEL_TEXT } from '@/features/telemetry/format'
import { Toggle } from '@/features/telemetry/toggle'
import { ExploreHeader } from '@/features/telemetry/ui'
import { LogHistogram } from './log-histogram'
import { useLiveTail, type TailStatus } from './use-live-tail'

const ROW = 30
const LIVE_MAX = 2000

function time(ts: number) {
  const d = new Date(ts)
  return `${absolute(d, { date: false, ms: true })}`
}

function AttrValue({ value }: { value: unknown }) {
  if (value === null || value === undefined) return <span className="text-muted-foreground italic">null</span>
  if (typeof value === 'object') return <span className="break-all">{JSON.stringify(value)}</span>
  if (typeof value === 'boolean' || typeof value === 'number') return <span className="text-level-info">{String(value)}</span>
  return <span className="break-all">{String(value)}</span>
}

function TraceIssues({ org, traceId }: { org: string; traceId: string }) {
  const q = useQuery({ ...traceQuery(org, traceId), retry: false })
  if (q.isPending) return <span className="text-xs text-muted-foreground">Looking for errors in the trace…</span>
  if (q.isError || !q.data.errors.length) return <span className="text-xs text-muted-foreground">No errors in this trace.</span>
  return (
    <ul className="flex flex-wrap gap-1.5">
      {q.data.errors.slice(0, 4).map((e) =>
        e.issue_id ? (
          <li key={e.event_id}>
            <Link
              to="/$org/issues/$issueId"
              params={{ org, issueId: e.issue_id }}
              className="inline-flex max-w-96 items-center gap-1.5 rounded-md border bg-card px-2 py-0.5 text-xs hover:bg-row-hover"
            >
              <LevelDot level={e.level} />
              {e.short_id && <span className="font-mono text-muted-foreground">{e.short_id}</span>}
              <span className="truncate">{e.title}</span>
            </Link>
          </li>
        ) : null,
      )}
    </ul>
  )
}

const LogLine = memo(function LogLine({
  row,
  org,
  expanded,
  focused,
  service,
  onToggle,
  onFilter,
  index,
}: {
  row: LogRow
  org: string
  expanded: boolean
  focused: boolean
  service: string | null
  onToggle: (id: string) => void
  onFilter: (key: string, value: string, mode: 'include' | 'exclude') => void
  index: number
}) {
  const attrs = Object.entries(row.attributes).sort(([a], [b]) => a.localeCompare(b))
  return (
    <div className={cn('border-b border-border/60', focused && 'bg-row-selected', expanded && 'bg-muted/30')} data-log-id={row.id}>
      <div
        role="row"
        aria-rowindex={index + 1}
        aria-expanded={expanded}
        className={cn(
          'grid h-[30px] cursor-default grid-cols-[18px_96px_52px_minmax(0,1fr)_auto] items-center gap-2 px-3 font-mono text-[12.5px] sm:grid-cols-[18px_170px_56px_110px_minmax(0,1fr)_auto] sm:px-4',
          !focused && 'hover:bg-row-hover',
        )}
        onClick={() => onToggle(row.id)}
      >
        <ChevronRight className={cn('size-3.5 text-muted-foreground transition-transform', expanded && 'rotate-90')} aria-hidden />
        <span role="gridcell" className="truncate text-muted-foreground tabular" title={absolute(row.timestamp, { ms: true })}>
          <span className="hidden sm:inline">{new Date(row.timestamp).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })} </span>
          {time(row.timestamp)}
        </span>
        <span role="gridcell" className={cn('flex items-center gap-1.5 text-[11.5px] font-medium uppercase', LOG_LEVEL_TEXT[row.level])}>
          <span className={cn('size-1.5 shrink-0 rounded-full', LOG_LEVEL_DOT[row.level])} />
          {row.level}
        </span>
        <span role="gridcell" className="hidden truncate text-muted-foreground sm:block" title={service ?? undefined}>
          {String(row.attributes['service.name'] ?? service ?? '')}
        </span>
        <span role="gridcell" className="truncate" title={row.body}>
          {row.body}
        </span>
        <span role="gridcell" className="flex items-center gap-1">
          {row.trace_id && (
            <Tip content="Open trace">
              <Link
                to="/$org/traces/$traceId"
                params={{ org, traceId: row.trace_id }}
                search={row.span_id ? { span: row.span_id } : {}}
                onClick={(e) => e.stopPropagation()}
                className="rounded p-0.5 text-muted-foreground hover:bg-accent hover:text-foreground"
                aria-label="Open trace"
              >
                <RouteIcon className="size-3.5" />
              </Link>
            </Tip>
          )}
        </span>
      </div>
      {expanded && (
        <div className="space-y-3 px-4 pt-1 pb-3 sm:pl-[46px]">
          <pre className="rounded-md border bg-code-bg p-2.5 font-mono text-[12.5px] break-words whitespace-pre-wrap">{row.body}</pre>
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <span className="text-muted-foreground">{absolute(row.timestamp, { ms: true })}</span>
            {row.environment && <Badge variant="outline">{row.environment}</Badge>}
            {row.release && (
              <Badge variant="outline" className="font-mono">
                {row.release}
              </Badge>
            )}
            {row.sdk && <span className="font-mono text-muted-foreground">{row.sdk}</span>}
            {row.trace_id && (
              <Link
                to="/$org/traces/$traceId"
                params={{ org, traceId: row.trace_id }}
                search={row.span_id ? { span: row.span_id } : {}}
                className="inline-flex items-center gap-1 font-medium text-brand-text hover:underline"
              >
                <RouteIcon className="size-3.5" /> View trace {row.trace_id.slice(0, 8)} <ArrowUpRight className="size-3" />
              </Link>
            )}
            {row.trace_id && (
              <button
                type="button"
                className="text-muted-foreground hover:text-foreground hover:underline"
                onClick={() => onFilter('trace', row.trace_id!, 'include')}
              >
                All logs of this trace
              </button>
            )}
          </div>
          {row.trace_id && (
            <div className="flex items-start gap-2">
              <span className="mt-0.5 flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
                <CircleAlert className="size-3.5" /> Issues
              </span>
              <TraceIssues org={org} traceId={row.trace_id} />
            </div>
          )}
          <div className="overflow-hidden rounded-md border bg-card">
            <table className="w-full table-fixed text-[12.5px]">
              <caption className="sr-only">Attributes</caption>
              <tbody className="divide-y">
                {attrs.map(([k, v]) => (
                  <tr key={k} className="group align-top">
                    <th scope="row" className="w-[34%] truncate bg-muted/40 px-2.5 py-1 text-left font-mono font-normal text-muted-foreground" title={k}>
                      {k}
                    </th>
                    <td className="px-2.5 py-1 font-mono">
                      <AttrValue value={v} />
                    </td>
                    <td className="w-16 px-1 py-0.5 text-right">
                      {(typeof v !== 'object' || v === null) && (
                        <span className="inline-flex opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                          <Button variant="ghost" size="icon-xs" aria-label={`Show only ${k} = ${String(v)}`} onClick={() => onFilter(k, String(v), 'include')}>
                            <Plus />
                          </Button>
                          <Button variant="ghost" size="icon-xs" aria-label={`Exclude ${k} = ${String(v)}`} onClick={() => onFilter(k, String(v), 'exclude')}>
                            <Minus />
                          </Button>
                        </span>
                      )}
                    </td>
                  </tr>
                ))}
                {attrs.length === 0 && (
                  <tr>
                    <td className="px-2.5 py-1.5 text-muted-foreground">No attributes</td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  )
})

function Facets({
  facets,
  query,
  onFilter,
  sample,
}: {
  facets: Facet[]
  query: string
  onFilter: (key: string, value: string, mode: 'include' | 'exclude') => void
  sample: number
}) {
  const parsed = useMemo(() => parseLogQuery(query), [query])
  const active = (key: string, value: string) =>
    key === 'level' ? parsed.levels.includes(value as LogRow['level']) : parsed.filters.some((f) => f.key === key && !f.negated && f.values.includes(value))
  return (
    <div className="space-y-4 p-3">
      <p className="text-[11px] text-muted-foreground">Top values in the latest {sample.toLocaleString('en-US')} lines shown.</p>
      {facets.map((f) => {
        const max = f.values[0]?.count ?? 1
        return (
          <section key={f.key} aria-label={`Facet ${f.key}`}>
            <h3 className="mb-1 truncate font-mono text-[11.5px] font-medium text-muted-foreground" title={f.key}>
              {f.key}
            </h3>
            <ul className="space-y-0.5">
              {f.values.map((v) => (
                <li key={v.value}>
                  <button
                    type="button"
                    onClick={(e) => onFilter(f.key, v.value, e.altKey ? 'exclude' : 'include')}
                    className={cn(
                      'group relative flex w-full items-center gap-2 overflow-hidden rounded-sm px-1.5 py-0.5 text-left text-[12.5px] hover:bg-accent',
                      active(f.key, v.value) && 'bg-brand-soft text-brand-text',
                    )}
                    title={`${f.key}:${v.value} (Alt+click to exclude)`}
                  >
                    <span className="absolute inset-y-0 left-0 bg-muted" style={{ width: `${(v.count / max) * 100}%` }} aria-hidden />
                    {f.key === 'level' && <span className={cn('relative size-1.5 shrink-0 rounded-full', LOG_LEVEL_DOT[v.value as LogRow['level']])} />}
                    <span className="relative min-w-0 flex-1 truncate font-mono">{v.value}</span>
                    <span className="relative text-[11px] text-muted-foreground tabular">{v.count}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )
      })}
      {facets.length === 0 && <p className="text-xs text-muted-foreground">No attributes yet.</p>}
    </div>
  )
}

const STATUS_LABEL: Record<TailStatus, string> = {
  off: '',
  connecting: 'Connecting…',
  live: 'Live',
  reconnecting: 'Reconnecting…',
  error: 'Live tail unavailable',
}

export function LogsPage({
  query,
  live,
  start,
  end,
  onChange,
}: {
  query: string
  live: boolean
  start?: string
  end?: string
  onChange: (p: { query?: string; live?: boolean; start?: string; end?: string }) => void
}) {
  const { org, projects, environments, period } = useGlobalFilters()
  const { data: projectList } = useProjects(org)
  const searchRef = useRef<SearchBarHandle>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const params: LogParams = useMemo(
    () => ({ project: projects, environment: environments, query, ...(start ? { start, end } : { period }) }),
    [projects, environments, query, start, end, period],
  )
  const list = useInfiniteList(tk.logs(org, params), (cursor, signal) => telemetryApi.logs.list(org, { ...params, cursor, limit: 200 }, signal))
  const hist = useQuery({
    queryKey: tk.logHistogram(org, params),
    queryFn: ({ signal }) => telemetryApi.logs.histogram(org, params, signal),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
    refetchInterval: live ? 15_000 : false,
  })

  // ---- live tail
  const [liveRows, setLiveRows] = useState<LogRow[]>([])
  const [pending, setPending] = useState<LogRow[]>([])
  const [paused, setPaused] = useState(false)
  const pausedRef = useRef(false)
  pausedRef.current = paused
  useEffect(() => {
    setLiveRows([])
    setPending([])
    setPaused(false)
  }, [live, query, projects, environments])
  const parsed = useMemo(() => parseLogQuery(query), [query])
  const onLive = useCallback(
    (rows: LogRow[]) => {
      // The server filters; filter again so a lagging server cannot add noise.
      const kept = rows.filter((r) => matchesLog(r, parsed)).reverse()
      if (!kept.length) return
      if (pausedRef.current) setPending((p) => [...kept, ...p].slice(0, LIVE_MAX))
      else setLiveRows((p) => [...kept, ...p].slice(0, LIVE_MAX))
    },
    [parsed],
  )
  const tailUrl = live ? telemetryApi.logs.streamUrl(org, { project: projects, environment: environments, query }) : null
  const { status: tail, skipped, message: tailMessage } = useLiveTail(tailUrl, onLive)

  const rows = useMemo(() => {
    const base = list.data?.pages.flatMap((p) => p.data) ?? []
    if (!live || !liveRows.length) return base
    const seen = new Set(liveRows.map((r) => r.id))
    return [...liveRows, ...base.filter((r) => !seen.has(r.id))]
  }, [list.data, live, liveRows])

  const facets = useMemo(() => computeFacets(rows.slice(0, 1000)), [rows])
  const attributeKeys = useMemo(() => facets.map((f) => f.key), [facets])
  const suggest = useCallback(
    (ctx: CompletionContext) => {
      if (ctx.mode === 'key') return suggestLogKeys(ctx.partial, attributeKeys)
      const values = facets.find((f) => f.key === ctx.key)?.values.map((v) => v.value) ?? []
      return suggestLogValues(ctx.key!, ctx.partial, values)
    },
    [facets, attributeKeys],
  )

  // ---- expansion, focus, virtualisation
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [focused, setFocused] = useState(-1)
  const toggle = useCallback((id: string) => {
    setExpanded((s) => {
      const next = new Set(s)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }, [])
  const filter = useCallback(
    (key: string, value: string, mode: 'include' | 'exclude') => onChange({ query: toggleLogFilter(query, key, value, mode) }),
    [onChange, query],
  )
  const virtualizer = useVirtualizer({
    count: rows.length + (list.hasNextPage ? 1 : 0),
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => (rows[i] && expanded.has(rows[i].id) ? 320 : ROW),
    initialRect: { width: 1200, height: 800 },
    overscan: 14,
    getItemKey: (i) => rows[i]?.id ?? `loader-${i}`,
  })
  const items = virtualizer.getVirtualItems()
  const last = items[items.length - 1]
  useEffect(() => {
    if (last && last.index >= rows.length - 30 && list.hasNextPage && !list.isFetchingNextPage) void list.fetchNextPage()
  }, [last, rows.length, list])
  useEffect(() => {
    if (focused >= 0) virtualizer.scrollToIndex(focused, { align: 'auto' })
  }, [focused, virtualizer])

  // Pause live updates while the reader scrolls down; resume at the top.
  const onScroll = () => {
    if (!live) return
    const top = (scrollRef.current?.scrollTop ?? 0) < 24
    if (!top && !paused) setPaused(true)
    if (top && paused) resume()
  }
  const resume = () => {
    setLiveRows((p) => [...pending, ...p].slice(0, LIVE_MAX))
    setPending([])
    setPaused(false)
    scrollRef.current?.scrollTo({ top: 0 })
  }

  useHotkeys(
    {
      j: () => setFocused((f) => Math.min(rows.length - 1, f + 1)),
      k: () => setFocused((f) => Math.max(0, f - 1)),
      Enter: () => rows[focused] && toggle(rows[focused].id),
      o: () => rows[focused] && toggle(rows[focused].id),
      t: () => onChange({ live: !live }),
      '/': () => searchRef.current?.focus(),
      Escape: () => setExpanded(new Set()),
    },
    [rows, focused, live],
  )

  const [facetsOpen, setFacetsOpen] = useState(false)
  const serviceName = (id: string | null) => projectList?.find((p) => p.id === id)?.slug ?? null
  const range = start ? `${absolute(start)} – ${end ? absolute(end, { date: false }) : 'now'}` : null

  return (
    <div className="flex h-full min-h-[560px] flex-col">
      <ExploreHeader
        title="Logs"
        actions={
          <>
            <Tip content="Live tail: stream new lines as they arrive (t)">
              <span>
                <Toggle pressed={live} onPressedChange={(v) => onChange({ live: v })} aria-label="Live tail">
                  {live ? <Radio className={cn(tail === 'live' && 'animate-pulse text-brand')} /> : <Play />} Live
                </Toggle>
              </span>
            </Tip>
          </>
        }
      >
        <div className="flex items-center gap-2">
          <Button variant="outline" size="icon" className="size-10 shrink-0 lg:hidden" onClick={() => setFacetsOpen(true)} aria-label="Show attribute facets">
            <Filter />
          </Button>
          <SearchBar
            ref={searchRef}
            org={org}
            projects={projects}
            value={query}
            onSubmit={(q) => onChange({ query: q })}
            label="Search logs"
            placeholder='Search logs: level:error service.name:api trace:… "timeout"'
            suggest={suggest}
            className="min-w-0 flex-1"
          />
        </div>
        {range && (
          <div className="flex items-center gap-2 text-xs">
            <Badge variant="brand" className="gap-1">
              {range}
              <button
                type="button"
                onClick={() => onChange({ start: undefined, end: undefined })}
                aria-label="Clear time range"
                className="hover:text-foreground"
              >
                <X />
              </button>
            </Badge>
          </div>
        )}
        <LogHistogram
          data={hist.data}
          loading={hist.isPending}
          onPick={(s, e) => onChange({ start: new Date(s).toISOString(), end: new Date(e).toISOString() })}
        />
      </ExploreHeader>
      <div className="flex min-h-0 flex-1">
        <aside className="hidden w-64 shrink-0 overflow-y-auto border-r lg:block" aria-label="Attribute facets">
          <Facets facets={facets} query={query} onFilter={filter} sample={Math.min(1000, rows.length)} />
        </aside>
        <Sheet open={facetsOpen} onOpenChange={setFacetsOpen}>
          <SheetContent side="left" className="w-72 overflow-y-auto bg-background">
            <SheetTitle>Attributes</SheetTitle>
            <SheetDescription>Filter by attribute values</SheetDescription>
            <Facets facets={facets} query={query} onFilter={(k, v, m) => (filter(k, v, m), setFacetsOpen(false))} sample={Math.min(1000, rows.length)} />
          </SheetContent>
        </Sheet>
        <div className="relative flex min-w-0 flex-1 flex-col">
          {live && (
            <div className="flex items-center gap-2 border-b bg-muted/40 px-4 py-1.5 text-xs" role="status">
              <span className={cn('size-2 rounded-full', tail === 'live' ? 'bg-success' : tail === 'error' ? 'bg-destructive' : 'bg-warning')} />
              <span>{paused ? 'Paused while you scroll' : STATUS_LABEL[tail]}</span>
              {!paused && tail === 'live' && !tailMessage && <span className="text-muted-foreground">new lines appear at the top</span>}
              {tailMessage && <span className="text-destructive">{tailMessage}</span>}
              {skipped > 0 && <span className="text-warning">{skipped.toLocaleString('en-US')} lines skipped while lines arrived faster than shown</span>}
              {paused ? (
                <Button size="xs" variant="outline" className="ml-auto" onClick={resume}>
                  <Play /> Resume{pending.length ? ` (${pending.length} new)` : ''}
                </Button>
              ) : (
                <Button size="xs" variant="ghost" className="ml-auto" onClick={() => setPaused(true)}>
                  <Pause /> Pause
                </Button>
              )}
            </div>
          )}
          {list.isPending ? (
            <ListSkeleton rows={14} />
          ) : list.isError ? (
            <ErrorState error={list.error} retry={() => list.refetch()} />
          ) : rows.length === 0 ? (
            <EmptyState
              icon={<ScrollText />}
              title={query ? 'No log lines match' : 'No logs yet'}
              description={
                query
                  ? 'Try another query, a longer period or other projects.'
                  : 'Enable logs in your SDK (enableLogs and the logger API, or the logging integration) and lines show up here within seconds.'
              }
              action={
                query ? (
                  <Button variant="outline" size="sm" onClick={() => onChange({ query: '' })}>
                    Clear the query
                  </Button>
                ) : undefined
              }
            />
          ) : (
            <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto" onScroll={onScroll} data-testid="logs-scroll">
              <div className="sticky top-0 z-10 hidden h-7 grid-cols-[18px_170px_56px_110px_minmax(0,1fr)_auto] items-center gap-2 border-b bg-background/95 px-4 text-[11px] font-medium tracking-wide text-muted-foreground uppercase backdrop-blur sm:grid">
                <span />
                <span>Time</span>
                <span>Level</span>
                <span>Service</span>
                <span>Message</span>
                <span />
              </div>
              <div role="grid" aria-label="Log lines" aria-rowcount={rows.length} className="relative" style={{ height: virtualizer.getTotalSize() }}>
                {items.map((v) => {
                  const row = rows[v.index]
                  return (
                    <div
                      key={v.key}
                      data-index={v.index}
                      ref={virtualizer.measureElement}
                      className="absolute inset-x-0"
                      style={{ transform: `translateY(${v.start}px)` }}
                    >
                      {row ? (
                        <LogLine
                          row={row}
                          org={org}
                          index={v.index}
                          expanded={expanded.has(row.id)}
                          focused={v.index === focused}
                          service={serviceName(row.project_id)}
                          onToggle={toggle}
                          onFilter={filter}
                        />
                      ) : (
                        <div className="flex h-[30px] items-center px-4 text-xs text-muted-foreground">Loading more…</div>
                      )}
                    </div>
                  )
                })}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
