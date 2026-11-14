import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ChevronDown, ChevronRight, ChevronsDownUp, ChevronsUpDown, CircleAlert, Route as RouteIcon, ScrollText, Search, ZoomIn, ZoomOut } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useProjects } from '@/api/queries'
import { telemetryApi, tk, traceQuery } from '@/api/telemetry'
import type { TraceError, TraceLogMarker } from '@/api/telemetry-types'
import { CopyButton } from '@/components/common/copy-button'
import { ErrorState } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Kbd } from '@/components/ui/kbd'
import { Skeleton } from '@/components/ui/skeleton'
import { Toggle } from '@/features/telemetry/toggle'
import { Tip } from '@/components/ui/tooltip'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { absolute } from '@/lib/format'
import { ms, opColor } from '@/features/telemetry/format'
import { SpanDrawer } from './span-drawer'
import { ancestorsOf, buildSpanTree, criticalPath, flattenTree, opBreakdown, traceBounds, walk, type SpanNode } from './span-tree'
import { clampView, Minimap, Waterfall, zoomView, type ViewWindow } from './waterfall'

function Breakdown({ roots, total }: { roots: SpanNode[]; total: number }) {
  const parts = useMemo(() => opBreakdown(roots), [roots])
  const sum = parts.reduce((s, p) => s + p.ms, 0) || 1
  const shown = parts.filter((p) => p.ms / sum >= 0.01).slice(0, 6)
  return (
    <div className="min-w-0 flex-1">
      <div
        className="flex h-2 overflow-hidden rounded-full bg-muted"
        role="img"
        aria-label={`Self time by operation: ${shown.map((p) => `${p.category} ${Math.round((p.ms / sum) * 100)}%`).join(', ')}`}
      >
        {shown.map((p) => (
          <span
            key={p.category}
            className="h-full border-r-2 border-card last:border-r-0"
            style={{ width: `${(p.ms / sum) * 100}%`, background: opColor(p.category) }}
          />
        ))}
      </div>
      <ul className="mt-1.5 flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
        {shown.map((p) => (
          <li key={p.category} className="flex items-center gap-1.5">
            <span className="size-2 rounded-[2px]" style={{ background: opColor(p.category) }} />
            {p.category} <span className="tabular">{Math.round((p.ms / sum) * 100)}%</span>
          </li>
        ))}
        <li className="ml-auto hidden sm:block">self time across {ms(total)}</li>
      </ul>
    </div>
  )
}

export function TracePage({ org, traceId, initialSpan }: { org: string; traceId: string; initialSpan?: string }) {
  const q = useQuery(traceQuery(org, traceId))
  const { data: projects } = useProjects(org)
  const trace = q.data
  const logsQ = useQuery({
    queryKey: tk.traceLogs(org, traceId),
    queryFn: () => telemetryApi.logs.list(org, { query: `trace:${traceId}`, limit: 500, period: '90d' }),
    enabled: Boolean(trace && trace.logs_count > 0),
    staleTime: 60_000,
  })

  const roots = useMemo(() => (trace ? buildSpanTree(trace.spans) : []), [trace])
  const bounds = useMemo(() => traceBounds(trace?.spans ?? []), [trace])
  const duration = Math.max(0.1, bounds.end - bounds.start)
  const spanMap = useMemo(() => new Map((trace?.spans ?? []).map((s) => [s.span_id, s])), [trace])
  const nodeMap = useMemo(() => {
    const m = new Map<string, SpanNode>()
    walk(roots, (n) => m.set(n.span.span_id, n))
    return m
  }, [roots])

  const [collapsed, setCollapsed] = useState<Set<string>>(new Set())
  const [selectedId, setSelectedId] = useState<string | null>(initialSpan ?? null)
  const [drawer, setDrawer] = useState(Boolean(initialSpan))
  const [criticalOn, setCriticalOn] = useState(false)
  const [search, setSearch] = useState('')
  const [view, setView] = useState<ViewWindow>({ start: 0, end: 1 })
  const [scrollTo, setScrollTo] = useState<number | null>(null)
  const searchRef = useRef<HTMLInputElement>(null)

  // Reset the window and collapse very large traces below the transactions.
  useEffect(() => {
    if (!trace) return
    setView({ start: 0, end: duration })
    if (trace.spans.length > 1500) {
      const c = new Set<string>()
      walk(roots, (n) => {
        if (n.depth >= 2 && n.children.length > 0) c.add(n.span.span_id)
      })
      setCollapsed(c)
    }
    // Jump to the span from the URL (for example an error's span).
    if (initialSpan && spanMap.has(initialSpan)) {
      setCollapsed((c) => {
        const next = new Set(c)
        ancestorsOf(initialSpan, spanMap).forEach((a) => next.delete(a))
        return next
      })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [trace])

  const rows = useMemo(() => flattenTree(roots, collapsed), [roots, collapsed])
  const critical = useMemo(() => (criticalOn ? criticalPath(roots) : null), [criticalOn, roots])
  const matches = useMemo(() => {
    const t = search.trim().toLowerCase()
    const out = new Set<string>()
    if (!t) return out
    for (const s of trace?.spans ?? [])
      if (`${s.op ?? ''} ${s.description ?? ''} ${s.transaction ?? ''} ${s.span_id}`.toLowerCase().includes(t)) out.add(s.span_id)
    return out
  }, [search, trace])

  const errorsBySpan = useMemo(() => {
    const m = new Map<string, TraceError[]>()
    for (const e of trace?.errors ?? []) {
      const key = e.span_id && spanMap.has(e.span_id) ? e.span_id : roots[0]?.span.span_id
      if (key) m.set(key, [...(m.get(key) ?? []), e])
    }
    return m
  }, [trace, spanMap, roots])
  const logsBySpan = useMemo(() => {
    const m = new Map<string, TraceLogMarker[]>()
    for (const l of logsQ.data?.data ?? []) {
      const key = l.span_id && spanMap.has(l.span_id) ? l.span_id : roots[0]?.span.span_id
      if (key) m.set(key, [...(m.get(key) ?? []), { span_id: l.span_id, timestamp: l.timestamp, level: l.level, body: l.body }])
    }
    return m
  }, [logsQ.data, spanMap, roots])

  const projectName = useCallback((id: string | null) => projects?.find((p) => p.id === id)?.slug ?? null, [projects])
  const toggle = useCallback(
    (id: string) =>
      setCollapsed((c) => {
        const next = new Set(c)
        if (next.has(id)) next.delete(id)
        else next.add(id)
        return next
      }),
    [],
  )
  const select = useCallback((id: string, open = false) => {
    setSelectedId(id)
    if (open) setDrawer(true)
  }, [])
  const reveal = useCallback(
    (id: string) => {
      const anc = ancestorsOf(id, spanMap)
      setCollapsed((c) => {
        if (!anc.some((a) => c.has(a))) return c
        const next = new Set(c)
        anc.forEach((a) => next.delete(a))
        return next
      })
      setSelectedId(id)
    },
    [spanMap],
  )
  // Keep the selected row in view.
  useEffect(() => {
    if (!selectedId) return
    const i = rows.findIndex((r) => r.node.span.span_id === selectedId)
    if (i >= 0) setScrollTo(i)
  }, [selectedId, rows])

  const selIndex = rows.findIndex((r) => r.node.span.span_id === selectedId)
  const move = (d: number) => {
    if (!rows.length) return
    const next = rows[Math.max(0, Math.min(rows.length - 1, (selIndex < 0 ? -1 : selIndex) + d))]
    setSelectedId(next.node.span.span_id)
  }
  const matchList = useMemo(() => rows.filter((r) => matches.has(r.node.span.span_id)).map((r) => r.node.span.span_id), [rows, matches])
  const nextMatch = () => {
    const all = (trace?.spans ?? []).filter((s) => matches.has(s.span_id)).sort((a, b) => a.start - b.start)
    if (!all.length) return
    const cur = all.findIndex((s) => s.span_id === selectedId)
    reveal(all[(cur + 1) % all.length].span_id)
  }

  useHotkeys(
    {
      j: () => move(1),
      k: () => move(-1),
      ArrowDown: () => move(1),
      ArrowUp: () => move(-1),
      ArrowLeft: () => {
        const n = selectedId ? nodeMap.get(selectedId) : undefined
        if (!n) return
        if (n.children.length && !collapsed.has(n.span.span_id)) toggle(n.span.span_id)
        else if (n.span.parent_span_id && nodeMap.has(n.span.parent_span_id)) setSelectedId(n.span.parent_span_id)
      },
      ArrowRight: () => {
        const n = selectedId ? nodeMap.get(selectedId) : undefined
        if (n && collapsed.has(n.span.span_id)) toggle(n.span.span_id)
        else if (n?.children[0]) setSelectedId(n.children[0].span.span_id)
      },
      Enter: () => selectedId && setDrawer(true),
      o: () => selectedId && setDrawer(true),
      Escape: () => setDrawer(false),
      c: () => setCriticalOn((v) => !v),
      '/': () => searchRef.current?.focus(),
      '0': () => setView({ start: 0, end: duration }),
      '=': () => setView((v) => zoomView(v, 0.5, 0.5, duration)),
      '-': () => setView((v) => zoomView(v, 2, 0.5, duration)),
      e: () => setCollapsed(new Set()),
    },
    [rows, selectedId, nodeMap, collapsed, duration, selIndex],
  )

  if (q.isError) return <ErrorState error={q.error} retry={() => q.refetch()} className="min-h-[60vh]" title={undefined} />

  const root = roots.reduce<SpanNode | null>((a, b) => (!a || b.span.duration_ms > a.span.duration_ms ? b : a), null)
  const services = new Set((trace?.spans ?? []).map((s) => s.project_id).filter(Boolean))
  const selectedNode = selectedId ? nodeMap.get(selectedId) : undefined

  return (
    <div className="relative flex h-full min-h-[600px] flex-col">
      <header className="space-y-3 border-b px-3 pt-3 pb-3 sm:px-5">
        <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
          <Link to="/$org/traces" params={{ org }} className="hover:text-foreground">
            Traces
          </Link>
          <ChevronRight className="size-3" />
          <span className="inline-flex items-center font-mono text-foreground">
            {traceId}
            <CopyButton value={traceId} label="Copy trace id" toastLabel="Trace id copied" />
          </span>
        </div>
        {!trace ? (
          <div className="space-y-2" aria-busy="true" aria-label="Loading trace">
            <Skeleton className="h-6 w-1/2" />
            <Skeleton className="h-4 w-1/3" />
          </div>
        ) : (
          <>
            <div className="flex flex-wrap items-end justify-between gap-3">
              <div className="min-w-0">
                <h1 className="truncate text-lg font-semibold tracking-tight">
                  {root?.span.transaction ?? root?.span.description ?? 'Trace'}
                  {root?.span.op && (
                    <Badge variant="outline" className="ml-2 align-middle font-mono font-normal">
                      {root.span.op}
                    </Badge>
                  )}
                </h1>
                <p className="mt-0.5 text-[13px] text-muted-foreground">
                  {absolute(bounds.start, { seconds: true })} · <RelativeTime date={bounds.start} suffix />
                </p>
              </div>
              <dl className="flex flex-wrap gap-x-5 gap-y-1 text-[13px]">
                <div>
                  <dt className="text-[11px] text-muted-foreground uppercase">Duration</dt>
                  <dd className="font-semibold tabular">{ms(duration)}</dd>
                </div>
                <div>
                  <dt className="text-[11px] text-muted-foreground uppercase">Spans</dt>
                  <dd className="font-semibold tabular">{trace.spans.length.toLocaleString('en-US')}</dd>
                </div>
                <div>
                  <dt className="text-[11px] text-muted-foreground uppercase">Services</dt>
                  <dd className="font-semibold tabular">{services.size}</dd>
                </div>
                <div>
                  <dt className="text-[11px] text-muted-foreground uppercase">Errors</dt>
                  <dd className={trace.errors.length ? 'font-semibold text-destructive tabular' : 'font-semibold tabular'}>{trace.errors.length}</dd>
                </div>
                <div>
                  <dt className="text-[11px] text-muted-foreground uppercase">Logs</dt>
                  <dd className="font-semibold tabular">
                    {trace.logs_count > 0 ? (
                      <Link
                        to="/$org/logs"
                        params={{ org }}
                        search={(prev: Record<string, unknown>) => ({ ...prev, query: `trace:${traceId}`, period: '90d' as const })}
                        className="text-brand-text hover:underline"
                      >
                        {trace.logs_count}
                      </Link>
                    ) : (
                      0
                    )}
                  </dd>
                </div>
              </dl>
            </div>
            {trace.errors.length > 0 && (
              <div className="flex flex-wrap items-center gap-2 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-1.5 text-[13px]">
                <CircleAlert className="size-4 text-destructive" />
                {trace.errors.slice(0, 3).map((e) => (
                  <button
                    key={e.event_id}
                    type="button"
                    className="min-w-0 truncate text-left hover:underline"
                    onClick={() => e.span_id && (reveal(e.span_id), setDrawer(true))}
                  >
                    {e.short_id && <span className="mr-1 font-mono text-xs text-muted-foreground">{e.short_id}</span>}
                    {e.title}
                  </button>
                ))}
              </div>
            )}
            <Breakdown roots={roots} total={duration} />
          </>
        )}
      </header>
      <div className="flex flex-wrap items-center gap-2 border-b px-3 py-2 sm:px-5" role="toolbar" aria-label="Trace view">
        <div className="relative w-64 max-w-full">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            ref={searchRef}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') nextMatch()
              if (e.key === 'Escape') (e.target as HTMLInputElement).blur()
            }}
            placeholder="Find spans"
            className="h-7 pl-7 text-[13px]"
            aria-label="Find spans"
          />
        </div>
        {search && (
          <span className="text-xs text-muted-foreground tabular">
            {matches.size} {matches.size === 1 ? 'match' : 'matches'}
            {matchList.length < matches.size && ` (${matches.size - matchList.length} in collapsed spans)`}
            {matches.size > 0 && (
              <Button variant="ghost" size="xs" className="ml-1" onClick={nextMatch}>
                <ChevronDown /> Next
              </Button>
            )}
          </span>
        )}
        <div className="ml-auto flex flex-wrap items-center gap-1">
          <Tip content="Highlight the critical path (c)">
            <span>
              <Toggle pressed={criticalOn} onPressedChange={setCriticalOn} aria-label="Critical path">
                <RouteIcon /> Critical path
              </Toggle>
            </span>
          </Tip>
          <Tip content="Expand all (e)">
            <Button variant="ghost" size="icon-sm" aria-label="Expand all" onClick={() => setCollapsed(new Set())}>
              <ChevronsUpDown />
            </Button>
          </Tip>
          <Tip content="Collapse to transactions">
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Collapse to transactions"
              onClick={() => {
                const c = new Set<string>()
                walk(roots, (n) => {
                  if (n.span.is_segment && n.children.some((x) => !x.span.is_segment)) c.add(n.span.span_id)
                })
                setCollapsed(c)
              }}
            >
              <ChevronsDownUp />
            </Button>
          </Tip>
          <span className="mx-1 h-4 w-px bg-border" />
          <Tip content="Zoom in (=)">
            <Button variant="ghost" size="icon-sm" aria-label="Zoom in" onClick={() => setView((v) => zoomView(v, 0.5, 0.5, duration))}>
              <ZoomIn />
            </Button>
          </Tip>
          <Tip content="Zoom out (-)">
            <Button variant="ghost" size="icon-sm" aria-label="Zoom out" onClick={() => setView((v) => zoomView(v, 2, 0.5, duration))}>
              <ZoomOut />
            </Button>
          </Tip>
          <Button variant="ghost" size="xs" onClick={() => setView({ start: 0, end: duration })} disabled={view.start === 0 && view.end >= duration}>
            Reset <Kbd className="ml-1">0</Kbd>
          </Button>
          {trace && trace.logs_count > 0 && (
            <Button asChild variant="outline" size="xs" className="ml-1">
              <Link
                to="/$org/logs"
                params={{ org }}
                search={(prev: Record<string, unknown>) => ({ ...prev, query: `trace:${traceId}`, period: '90d' as const })}
              >
                <ScrollText /> Logs
              </Link>
            </Button>
          )}
        </div>
      </div>
      {trace && (
        <div className="border-b px-3 py-2 sm:px-5">
          <Minimap rows={rows} traceStart={bounds.start} traceDuration={duration} view={view} onView={(v) => setView(clampView(v, duration))} />
          <p className="mt-1 hidden text-[11px] text-muted-foreground md:block">
            Drag on the overview to zoom, ⌘/Ctrl + scroll on the timeline to zoom, Shift + scroll to pan, double-click to reset.
          </p>
        </div>
      )}
      {!trace ? (
        <div className="space-y-1.5 p-4" aria-busy="true" aria-label="Loading spans">
          {Array.from({ length: 12 }, (_, i) => (
            <Skeleton key={i} className="h-5" style={{ marginLeft: `${(i % 4) * 16}px`, width: `${90 - (i % 5) * 8}%` }} />
          ))}
        </div>
      ) : (
        <div className="relative flex min-h-0 flex-1">
          <Waterfall
            rows={rows}
            traceStart={bounds.start}
            traceDuration={duration}
            view={view}
            onView={setView}
            selectedId={selectedId}
            onSelect={select}
            onToggle={toggle}
            critical={critical}
            matches={matches}
            errorsBySpan={errorsBySpan}
            logsBySpan={logsBySpan}
            projectName={projectName}
            scrollToIndex={scrollTo}
          />
          {drawer && selectedNode && (
            <SpanDrawer
              org={org}
              node={selectedNode}
              traceStart={bounds.start}
              traceId={traceId}
              errors={errorsBySpan.get(selectedNode.span.span_id) ?? []}
              logs={logsBySpan.get(selectedNode.span.span_id) ?? []}
              projectName={projectName}
              onClose={() => setDrawer(false)}
              onSelect={reveal}
            />
          )}
        </div>
      )}
    </div>
  )
}
