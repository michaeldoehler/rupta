import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { ArrowRight, Route as RouteIcon, Search } from 'lucide-react'
import { useDeferredValue, useMemo, useState } from 'react'
import { telemetryApi, tk } from '@/api/telemetry'
import type { SpanGroupBy } from '@/api/telemetry-types'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { compact } from '@/lib/format'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { ms, opColor } from '@/features/telemetry/format'
import { ExploreHeader, SectionTabs, ShareBar } from '@/features/telemetry/ui'
import { opCategory } from './span-tree'
import { PERFORMANCE_TABS } from './transactions-page'

const TRACE_ID = /^[0-9a-f]{32}$/i

export function SpansPage({ groupBy, query, onChange }: { groupBy: SpanGroupBy; query: string; onChange: (p: { group_by?: SpanGroupBy; q?: string }) => void }) {
  const { org, projects, environments, period } = useGlobalFilters()
  const navigate = useNavigate()
  const [traceInput, setTraceInput] = useState('')
  const [text, setText] = useState(query)
  const deferred = useDeferredValue(text.trim())
  const params = { project: projects, environment: environments, period, group_by: groupBy, query: deferred }
  const grouped = groupBy !== 'none'
  const q = useQuery({ queryKey: tk.spans(org, params), queryFn: ({ signal }) => telemetryApi.spans.groups(org, params, signal), placeholderData: keepPreviousData, staleTime: 30_000, enabled: grouped })
  const samples = useQuery({ queryKey: tk.spans(org, { ...params, samples: true }), queryFn: ({ signal }) => telemetryApi.spans.samples(org, { project: projects, environment: environments, period, query: deferred }, signal), placeholderData: keepPreviousData, staleTime: 30_000, enabled: !grouped })
  const total = useMemo(() => (q.data ?? []).reduce((s, r) => s + r.sum, 0), [q.data])
  const traceId = traceInput.trim().replace(/-/g, '')

  return (
    <div>
      <ExploreHeader title="Traces" tabs={<SectionTabs label="Performance sections" items={PERFORMANCE_TABS(org)} />}>
        <div className="flex flex-wrap items-center gap-2">
          <form
            className="flex min-w-64 flex-1 gap-2 sm:max-w-md"
            onSubmit={(e) => {
              e.preventDefault()
              if (TRACE_ID.test(traceId)) void navigate({ to: '/$org/traces/$traceId', params: { org, traceId: traceId.toLowerCase() } })
            }}
          >
            <div className="relative flex-1">
              <RouteIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input value={traceInput} onChange={(e) => setTraceInput(e.target.value)} placeholder="Open a trace by id (32 hex characters)" className="pl-8 font-mono text-[13px]" aria-label="Trace id" spellCheck={false} />
            </div>
            <Button type="submit" variant="outline" disabled={!TRACE_ID.test(traceId)}>
              Open <ArrowRight />
            </Button>
          </form>
        </div>
      </ExploreHeader>
      <div className="flex flex-wrap items-center gap-2 border-b px-3 py-2 sm:px-5">
        <h2 className="mr-2 text-[13px] font-semibold">Span explorer</h2>
        <ToggleGroup type="single" value={groupBy} onValueChange={(v) => v && onChange({ group_by: v as SpanGroupBy })} aria-label="Group spans by">
          <ToggleGroupItem value="op">By operation</ToggleGroupItem>
          <ToggleGroupItem value="description">By description</ToggleGroupItem>
          <ToggleGroupItem value="transaction">By transaction</ToggleGroupItem>
          <ToggleGroupItem value="none">Spans</ToggleGroupItem>
        </ToggleGroup>
        <div className="relative ml-auto w-72 max-w-full">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={text}
            onChange={(e) => setText(e.target.value)}
            onBlur={() => onChange({ q: text.trim() || undefined })}
            placeholder="Filter, e.g. op:db.query duration:>100"
            className="h-7 pl-7 text-[13px]"
            aria-label="Filter spans"
          />
        </div>
      </div>
      {!grouped ? (
        samples.isPending ? (
          <ListSkeleton rows={10} />
        ) : samples.isError ? (
          <ErrorState error={samples.error} retry={() => samples.refetch()} />
        ) : samples.data.length === 0 ? (
          <EmptyState icon={<RouteIcon />} title="No spans match" description="Try another filter or a longer period." />
        ) : (
          <div className="overflow-x-auto">
            <Table aria-label="Spans">
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead className="pl-4 sm:pl-6">Operation</TableHead>
                  <TableHead>Description</TableHead>
                  <TableHead className="hidden lg:table-cell">Transaction</TableHead>
                  <TableHead className="text-right">Duration</TableHead>
                  <TableHead className="pr-4 text-right sm:pr-6">When</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {samples.data.map((r) => (
                  <TableRow key={`${r.trace_id}-${r.span_id}`}>
                    <TableCell className="pl-4 sm:pl-6">
                      <span className="inline-flex items-center gap-2 font-mono text-[12.5px]">
                        <span className="size-2 rounded-[2px]" style={{ background: opColor(opCategory(r.op)) }} />
                        {r.op ?? '(none)'}
                      </span>
                    </TableCell>
                    <TableCell className="max-w-[36vw] truncate font-mono text-[12.5px]">
                      <Link to="/$org/traces/$traceId" params={{ org, traceId: r.trace_id }} search={{ span: r.span_id }} className="hover:text-brand-text hover:underline" title={r.description ?? undefined}>
                        {r.description ?? '(no description)'}
                      </Link>
                    </TableCell>
                    <TableCell className="hidden max-w-64 truncate font-mono text-xs text-muted-foreground lg:table-cell">{r.transaction ?? '—'}</TableCell>
                    <TableCell className={r.status && r.status !== 'ok' ? 'text-right font-medium text-destructive tabular' : 'text-right font-medium tabular'}>{ms(r.duration_ms)}</TableCell>
                    <TableCell className="pr-4 text-right text-muted-foreground sm:pr-6">{r.timestamp ? <RelativeTime date={r.timestamp} suffix plain /> : '—'}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )
      ) : q.isPending ? (
        <ListSkeleton rows={10} />
      ) : q.isError ? (
        <ErrorState error={q.error} retry={() => q.refetch()} />
      ) : q.data.length === 0 ? (
        <EmptyState icon={<RouteIcon />} title="No spans" description="Spans appear once SDKs send transactions or streamed spans for the selected projects and period." />
      ) : (
        <div className="overflow-x-auto">
          <Table aria-label="Span groups">
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                {groupBy === 'op' && <TableHead className="pl-4 sm:pl-6">Operation</TableHead>}
                {groupBy !== 'op' && <TableHead className="pl-4 sm:pl-6">{groupBy === 'transaction' ? 'Transaction' : 'Description'}</TableHead>}
                <TableHead className="text-right">Count</TableHead>
                <TableHead className="text-right">p50</TableHead>
                <TableHead className="text-right">p95</TableHead>
                <TableHead className="w-48 pr-4 sm:pr-6">Time spent</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {q.data.map((r) => (
                <TableRow
                  key={`${r.op}-${r.description}`}
                  className={groupBy === 'op' ? 'cursor-pointer' : undefined}
                  onClick={() => {
                    if (groupBy !== 'op' || !r.op) return
                    setText(`op:${r.op}`)
                    onChange({ group_by: 'description', q: `op:${r.op}` })
                  }}
                >
{groupBy === 'op' && (
                  <TableCell className="pl-4 sm:pl-6">
                    <span className="inline-flex items-center gap-2 font-mono text-[12.5px]">
                      <span className="size-2 rounded-[2px]" style={{ background: opColor(opCategory(r.op)) }} />
                      {r.op ?? '(none)'}
                    </span>
                  </TableCell>
                  )}
                  {groupBy !== 'op' && (
                    <TableCell className="max-w-[40vw] truncate pl-4 font-mono text-[12.5px] sm:pl-6" title={r.group ?? undefined}>
                      {groupBy === 'transaction' && r.group ? (
                        <Link to="/$org/performance/summary" params={{ org }} search={(prev: Record<string, unknown>) => ({ ...prev, name: r.group! })} className="hover:text-brand-text hover:underline" onClick={(e) => e.stopPropagation()}>
                          {r.group}
                        </Link>
                      ) : (
                        (r.group ?? <span className="text-muted-foreground">(none)</span>)
                      )}
                    </TableCell>
                  )}
                  <TableCell className="text-right tabular">{compact(r.count)}</TableCell>
                  <TableCell className="text-right tabular">{ms(r.p50)}</TableCell>
                  <TableCell className="text-right font-medium tabular">{ms(r.p95)}</TableCell>
                  <TableCell className="pr-4 sm:pr-6">
                    <div className="flex items-center gap-2">
                      <ShareBar value={total ? r.sum / total : 0} color={opColor(opCategory(r.op))} />
                      <span className="w-12 shrink-0 text-right text-xs text-muted-foreground tabular">{total ? `${((r.sum / total) * 100).toFixed(1)}%` : '—'}</span>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  )
}
