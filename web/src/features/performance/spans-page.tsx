import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { ArrowRight, Route as RouteIcon, Search } from 'lucide-react'
import { useDeferredValue, useMemo, useState } from 'react'
import { telemetryApi, tk } from '@/api/telemetry'
import type { SpanGroupBy } from '@/api/telemetry-types'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
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
  const q = useQuery({ queryKey: tk.spans(org, params), queryFn: ({ signal }) => telemetryApi.spans.groups(org, params, signal), placeholderData: keepPreviousData, staleTime: 30_000 })
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
        </ToggleGroup>
        <div className="relative ml-auto w-72 max-w-full">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={text}
            onChange={(e) => setText(e.target.value)}
            onBlur={() => onChange({ q: text.trim() || undefined })}
            placeholder={groupBy === 'op' ? 'Filter operations' : 'Filter, e.g. db.query or SELECT'}
            className="h-7 pl-7 text-[13px]"
            aria-label="Filter spans"
          />
        </div>
      </div>
      {q.isPending ? (
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
                <TableHead className="pl-4 sm:pl-6">Operation</TableHead>
                {groupBy === 'description' && <TableHead>Description</TableHead>}
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
                  onClick={() => groupBy === 'op' && r.op && (setText(r.op), onChange({ group_by: 'description', q: r.op }))}
                >
                  <TableCell className="pl-4 sm:pl-6">
                    <span className="inline-flex items-center gap-2 font-mono text-[12.5px]">
                      <span className="size-2 rounded-[2px]" style={{ background: opColor(opCategory(r.op)) }} />
                      {r.op ?? '(none)'}
                    </span>
                  </TableCell>
                  {groupBy === 'description' && (
                    <TableCell className="max-w-[40vw] truncate font-mono text-[12.5px]" title={r.description ?? undefined}>
                      {r.description ?? <span className="text-muted-foreground">(none)</span>}
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
