import { Link, useNavigate } from '@tanstack/react-router'
import { Gauge, Search } from 'lucide-react'
import { useDeferredValue, useMemo, useState } from 'react'
import { useProjects } from '@/api/queries'
import { useTransactions } from '@/api/telemetry'
import type { TransactionRow, TransactionSort } from '@/api/telemetry-types'
import { PlatformIcon } from '@/components/common/platform-icon'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tip } from '@/components/ui/tooltip'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { compact, thousands } from '@/lib/format'
import { cn } from '@/lib/utils'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { apdexGrade, GRADE_TEXT, ms, pct, rate } from '@/features/telemetry/format'
import { ExploreHeader, SectionTabs, SortButton, StatTile } from '@/features/telemetry/ui'

const KINDS = [
  ['all', 'All', null],
  ['frontend', 'Page loads & navigations', ['pageload', 'navigation']],
  ['backend', 'Requests', ['http.server']],
  ['jobs', 'Jobs & tasks', ['queue.process', 'task']],
] as const

type ClientSort = 'p75' | 'apdex'

export const PERFORMANCE_TABS = (org: string) => [
  { to: '/$org/performance' as const, params: { org }, label: 'Transactions', exact: true },
  { to: '/$org/traces' as const, params: { org }, label: 'Spans & traces' },
]

export function TransactionsPage({ sort, onSort }: { sort: TransactionSort; onSort: (s: TransactionSort) => void }) {
  const { org, projects, environments, period } = useGlobalFilters()
  const navigate = useNavigate()
  const { data: projectList } = useProjects(org)
  const [text, setText] = useState('')
  const [kind, setKind] = useState<(typeof KINDS)[number][0]>('all')
  const [clientSort, setClientSort] = useState<ClientSort | null>(null)
  const [focused, setFocused] = useState(-1)
  const query = useDeferredValue(text.trim())
  const q = useTransactions(org, { project: projects, environment: environments, period, query, sort })

  const rows = useMemo(() => {
    const ops = KINDS.find((k) => k[0] === kind)?.[2]
    let r = (q.data ?? []).filter((t) => !ops || (t.op !== null && (ops as readonly string[]).includes(t.op)))
    if (clientSort === 'p75') r = [...r].sort((a, b) => b.p75 - a.p75)
    if (clientSort === 'apdex') r = [...r].sort((a, b) => a.apdex - b.apdex)
    return r
  }, [q.data, kind, clientSort])

  const totals = useMemo(() => {
    const count = rows.reduce((s, r) => s + r.count, 0)
    const tpm = rows.reduce((s, r) => s + r.tpm, 0)
    const failures = rows.reduce((s, r) => s + r.count * r.failure_rate, 0)
    const apdex = count ? rows.reduce((s, r) => s + r.apdex * r.count, 0) / count : 0
    const slowest = rows.reduce<TransactionRow | null>((m, r) => (!m || r.p95 > m.p95 ? r : m), null)
    return { count, tpm, failure: count ? failures / count : 0, apdex, slowest }
  }, [rows])

  const project = (id: string | null) => projectList?.find((p) => p.id === id)
  const open = (r: TransactionRow) => void navigate({ to: '/$org/performance/summary', params: { org }, search: (prev: Record<string, unknown>) => ({ ...prev, name: r.name, tx_project: r.project_id ?? undefined }) })

  useHotkeys(
    {
      j: () => setFocused((f) => Math.min(rows.length - 1, f + 1)),
      k: () => setFocused((f) => Math.max(0, f - 1)),
      Enter: () => rows[focused] && open(rows[focused]),
      o: () => rows[focused] && open(rows[focused]),
    },
    [rows, focused],
  )

  const sortHead = (key: TransactionSort | ClientSort, label: string, align = 'text-right') => {
    const active = clientSort ? clientSort === key : sort === key
    return (
      <TableHead className={align}>
        <SortButton
          label={label}
          active={active}
          desc={key !== 'apdex'}
          onClick={() => {
            if (key === 'p75' || key === 'apdex') setClientSort(key)
            else {
              setClientSort(null)
              onSort(key)
            }
          }}
        />
      </TableHead>
    )
  }

  return (
    <div>
      <ExploreHeader title="Performance" tabs={<SectionTabs label="Performance sections" items={PERFORMANCE_TABS(org)} />}>
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative max-w-md min-w-48 flex-1">
            <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input value={text} onChange={(e) => setText(e.target.value)} placeholder="Search transactions" className="pl-8" aria-label="Search transactions" />
          </div>
          <ToggleGroup type="single" value={kind} onValueChange={(v) => v && setKind(v as typeof kind)} aria-label="Transaction kind" className="h-8">
            {KINDS.map(([id, label]) => (
              <ToggleGroupItem key={id} value={id} className="px-2.5">
                {label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>
      </ExploreHeader>
      {q.isPending ? (
        <ListSkeleton rows={10} />
      ) : q.isError ? (
        <ErrorState error={q.error} retry={() => q.refetch()} />
      ) : rows.length === 0 ? (
        <EmptyState
          icon={<Gauge />}
          title={text || kind !== 'all' ? 'No transactions match' : 'No transactions yet'}
          description={
            text || kind !== 'all'
              ? 'Try another name, kind, period or project.'
              : 'Enable tracing in your SDK (for example tracesSampleRate or traces_sample_rate) and transactions show up here within seconds.'
          }
        />
      ) : (
        <>
          <div className="grid grid-cols-2 gap-2 px-3 pt-3 sm:grid-cols-4 sm:px-5">
            <StatTile label="Throughput" value={`${rate(totals.tpm)} tpm`} hint={`${compact(totals.count)} transactions`} />
            <StatTile label="Failure rate" value={pct(totals.failure)} valueClassName={totals.failure > 0.01 ? 'text-destructive' : undefined} hint="Status other than ok" />
            <StatTile label="Apdex" value={totals.apdex.toFixed(2)} valueClassName={GRADE_TEXT[apdexGrade(totals.apdex)]} hint="Weighted by count, T = 300 ms" />
            <StatTile label="Slowest p95" value={ms(totals.slowest?.p95)} hint={totals.slowest?.name} />
          </div>
          <div className="mt-3 overflow-x-auto border-t">
            <Table aria-label="Transactions">
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead className="pl-4 sm:pl-6">Transaction</TableHead>
                  {sortHead('tpm', 'TPM')}
                  {sortHead('p50', 'p50')}
                  <TableHead className="hidden text-right md:table-cell">
                    <SortButton label="p75" active={clientSort === 'p75'} onClick={() => setClientSort('p75')} />
                  </TableHead>
                  {sortHead('p95', 'p95')}
                  <TableHead className="hidden text-right lg:table-cell">
                    <SortButton label="p99" active={!clientSort && sort === 'p99'} onClick={() => (setClientSort(null), onSort('p99'))} />
                  </TableHead>
                  {sortHead('failure_rate', 'Failures')}
                  <TableHead className="hidden text-right sm:table-cell">
                    <Tip content="Share of satisfied (≤ 300 ms) plus half the tolerated (≤ 1.2 s) transactions">
                      <span>
                        <SortButton label="Apdex" active={clientSort === 'apdex'} desc={false} onClick={() => setClientSort('apdex')} />
                      </span>
                    </Tip>
                  </TableHead>
                  <TableHead className="hidden pr-4 text-right xl:table-cell sm:pr-6">
                    <SortButton label="Count" active={!clientSort && sort === 'count'} onClick={() => (setClientSort(null), onSort('count'))} />
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((r, i) => {
                  const p = project(r.project_id)
                  return (
                    <TableRow key={`${r.project_id}-${r.name}`} data-focused={i === focused || undefined} className={cn(i === focused && 'bg-row-selected')} onMouseEnter={() => setFocused(i)}>
                      <TableCell className="max-w-0 pl-4 sm:pl-6">
                        <Link
                          to="/$org/performance/summary"
                          params={{ org }}
                          search={(prev: Record<string, unknown>) => ({ ...prev, name: r.name, tx_project: r.project_id ?? undefined })}
                          className="group flex min-w-0 items-center gap-2"
                        >
                          <PlatformIcon platform={p?.platform} size="sm" />
                          <span className="truncate font-mono text-[13px] font-medium group-hover:text-brand-text group-hover:underline">{r.name}</span>
                          {r.op && (
                            <Badge variant="outline" className="hidden font-mono font-normal sm:inline-flex">
                              {r.op}
                            </Badge>
                          )}
                        </Link>
                      </TableCell>
                      <TableCell className="text-right tabular">{rate(r.tpm)}</TableCell>
                      <TableCell className="text-right tabular">{ms(r.p50)}</TableCell>
                      <TableCell className="hidden text-right tabular md:table-cell">{ms(r.p75)}</TableCell>
                      <TableCell className={cn('text-right font-medium tabular', r.p95 > 3000 && 'text-warning')}>{ms(r.p95)}</TableCell>
                      <TableCell className="hidden text-right tabular text-muted-foreground lg:table-cell">{ms(r.p99)}</TableCell>
                      <TableCell className={cn('text-right tabular', r.failure_rate >= 0.02 ? 'text-destructive' : r.failure_rate >= 0.005 ? 'text-warning' : 'text-muted-foreground')}>{pct(r.failure_rate)}</TableCell>
                      <TableCell className={cn('hidden text-right font-medium tabular sm:table-cell', GRADE_TEXT[apdexGrade(r.apdex)])}>{r.apdex.toFixed(2)}</TableCell>
                      <TableCell className="hidden pr-4 text-right tabular text-muted-foreground xl:table-cell sm:pr-6">{thousands(r.count)}</TableCell>
                    </TableRow>
                  )
                })}
              </TableBody>
            </Table>
          </div>
        </>
      )}
    </div>
  )
}
