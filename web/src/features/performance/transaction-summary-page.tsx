import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ChevronRight, CircleCheck, CircleX, Flame, Film, Gauge, TriangleAlert } from 'lucide-react'
import { useProjects } from '@/api/queries'
import { telemetryApi, tk } from '@/api/telemetry'
import type { TransactionSummary, VitalName } from '@/api/telemetry-types'
import { LevelDot } from '@/components/common/level'
import { PlatformIcon } from '@/components/common/platform-icon'
import { EmptyState, ErrorState } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tip } from '@/components/ui/tooltip'
import { compact } from '@/lib/format'
import { cn } from '@/lib/utils'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { HistogramChart, TimeSeriesChart } from '@/features/telemetry/charts'
import { apdexGrade, GRADE_TEXT, ms, pct, rate } from '@/features/telemetry/format'
import { ExploreHeader, Panel, StatTile, StatTilesSkeleton } from '@/features/telemetry/ui'
import { RATING_LABEL, rateVital, VITALS, type VitalRating } from './vitals'

const RATING_STYLE: Record<VitalRating, { text: string; bar: string; icon: typeof CircleCheck }> = {
  good: { text: 'text-success', bar: 'bg-success', icon: CircleCheck },
  'needs-improvement': { text: 'text-warning', bar: 'bg-warning', icon: TriangleAlert },
  poor: { text: 'text-destructive', bar: 'bg-destructive', icon: CircleX },
}

function VitalCard({ k, value, count }: { k: VitalName; value: number | null; count: number }) {
  const v = VITALS.find((x) => x.key === k)!
  if (value === null)
    return (
      <div className="rounded-lg border bg-card p-3">
        <div className="text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{v.label}</div>
        <div className="mt-1 text-sm text-muted-foreground">No data</div>
      </div>
    )
  const rating = rateVital(k, value)
  const style = RATING_STYLE[rating]
  const Icon = style.icon
  // Position on a scale where the "poor" threshold sits at 2/3.
  const pos = Math.min(1, value / (v.poor * 1.5))
  return (
    <div className="rounded-lg border bg-card p-3" data-testid={`vital-${k}`}>
      <div className="flex items-center justify-between gap-2">
        <Tip content={v.name}>
          <span className="text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{v.label}</span>
        </Tip>
        <span className={cn('inline-flex items-center gap-1 text-xs font-medium', style.text)}>
          <Icon className="size-3.5" /> {RATING_LABEL[rating]}
        </span>
      </div>
      <div className="mt-1 text-xl font-semibold tabular">{v.unit === 'ms' ? ms(value) : value.toFixed(2)}</div>
      <div className="relative mt-2 flex h-1.5 gap-0.5" aria-hidden>
        <span className="h-full rounded-l-full bg-success/70" style={{ width: `${(v.good / (v.poor * 1.5)) * 100}%` }} />
        <span className="h-full bg-warning/70" style={{ width: `${((v.poor - v.good) / (v.poor * 1.5)) * 100}%` }} />
        <span className="h-full flex-1 rounded-r-full bg-destructive/70" />
        <span className="absolute -top-1 h-3.5 w-0.5 rounded-full bg-foreground ring-2 ring-card" style={{ left: `calc(${pos * 100}% - 1px)` }} />
      </div>
      <div className="mt-1.5 flex justify-between text-[11px] text-muted-foreground tabular">
        <span>p75 of {compact(count)}</span>
        <span>good ≤ {v.unit === 'ms' ? ms(v.good) : v.good}</span>
      </div>
    </div>
  )
}

export function TransactionSummaryPage({ name, projectId }: { name: string; projectId?: string }) {
  const { org, projects, environments, period } = useGlobalFilters()
  const { data: projectList } = useProjects(org)
  const project = projectList?.find((p) => p.id === projectId)
  const scope = projectId ? [projectId] : projects
  const params = { project: scope, environment: environments, period }
  const q = useQuery({
    queryKey: tk.summary(org, name, params),
    queryFn: ({ signal }) => telemetryApi.performance.summary(org, name, params, signal),
    placeholderData: (p) => p,
    staleTime: 30_000,
  })
  const d: TransactionSummary | undefined = q.data
  const isBrowser = d && (d.op === 'pageload' || d.op === 'navigation' || Object.values(d.vitals).some((v) => v && v.count > 0))

  return (
    <div>
      <ExploreHeader
        title={
          <span className="flex min-w-0 flex-wrap items-center gap-2">
            <span className="font-mono">{name}</span>
            {d?.op && (
              <Badge variant="outline" className="font-mono font-normal">
                {d.op}
              </Badge>
            )}
          </span>
        }
        description={
          <span className="flex items-center gap-1.5">
            <Link to="/$org/performance" params={{ org }} className="hover:text-foreground">
              Performance
            </Link>
            <ChevronRight className="size-3" />
            <span>Transaction summary</span>
            {project && (
              <>
                <ChevronRight className="size-3" />
                <PlatformIcon platform={project.platform} size="sm" /> {project.slug}
              </>
            )}
          </span>
        }
        actions={
          <Button asChild variant="outline" size="sm">
            <Link
              to="/$org/profiles/aggregate"
              params={{ org }}
              search={(prev: Record<string, unknown>) => ({ ...prev, transaction: name, tx_project: projectId })}
            >
              <Flame /> Aggregated flamegraph
            </Link>
          </Button>
        }
      />
      {q.isError ? (
        <ErrorState error={q.error} retry={() => q.refetch()} />
      ) : (
        <div className="space-y-3 p-3 sm:p-5">
          {!d ? (
            <StatTilesSkeleton count={6} />
          ) : (
            d.totals && (
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-6">
                <StatTile label="Throughput" value={`${rate(d.totals.tpm)} tpm`} />
                <StatTile label="Count" value={compact(d.totals.count)} />
                <StatTile label="p50" value={ms(d.totals.p50)} />
                <StatTile label="p95" value={ms(d.totals.p95)} />
                <StatTile
                  label="Failure rate"
                  value={pct(d.totals.failure_rate)}
                  valueClassName={d.totals.failure_rate >= 0.02 ? 'text-destructive' : undefined}
                />
                <StatTile label="Apdex" value={d.totals.apdex.toFixed(2)} valueClassName={GRADE_TEXT[apdexGrade(d.totals.apdex)]} />
              </div>
            )
          )}
          <div className="grid gap-3 lg:grid-cols-2">
            <Panel title="Throughput" description="Transactions per interval">
              <TimeSeriesChart
                loading={!d}
                type="bar"
                label="Throughput over time"
                series={[{ name: 'count', data: (d?.series ?? []).map((p) => [p.ts, p.count]), color: 'var(--series-2)' }]}
                format={(v) => compact(Math.round(v))}
                height={180}
              />
            </Panel>
            <Panel title="Duration" description="p50 and p95 per interval">
              <TimeSeriesChart
                loading={!d}
                label="Duration percentiles over time"
                series={[
                  { name: 'p50', data: (d?.series ?? []).map((p) => [p.ts, p.p50]) },
                  { name: 'p95', data: (d?.series ?? []).map((p) => [p.ts, p.p95]) },
                ]}
                format={ms}
                height={180}
              />
            </Panel>
          </div>
          {isBrowser && d && (
            <section aria-labelledby="vitals-title">
              <h2 id="vitals-title" className="mb-2 text-[13px] font-semibold">
                Web vitals <span className="font-normal text-muted-foreground">· 75th percentile</span>
              </h2>
              <div className="grid grid-cols-2 gap-2 md:grid-cols-3 xl:grid-cols-5">
                {VITALS.map((v) => (
                  <VitalCard key={v.key} k={v.key} value={d.vitals[v.key]?.p75 ?? null} count={d.vitals[v.key]?.count ?? 0} />
                ))}
              </div>
            </section>
          )}
          <div className="grid gap-3 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
            <Panel title="Duration distribution" description={d?.totals ? `p50 ${ms(d.totals.p50)} · p95 ${ms(d.totals.p95)}` : undefined}>
              {d ? (
                <HistogramChart
                  label="Duration histogram"
                  buckets={d.histogram}
                  format={ms}
                  lines={
                    d.totals
                      ? [
                          { value: d.totals.p50, label: 'p50' },
                          { value: d.totals.p95, label: 'p95' },
                        ]
                      : []
                  }
                />
              ) : (
                <Skeleton className="h-[180px] w-full" />
              )}
            </Panel>
            <Panel title="Related issues" description="Unresolved issues in the same project" bodyClassName="p-0">
              {!d ? (
                <div className="space-y-2 p-3">
                  <Skeleton className="h-5 w-full" />
                  <Skeleton className="h-5 w-4/5" />
                </div>
              ) : d.related_issues.length === 0 ? (
                <EmptyState compact title="No related issues" description="No errors were linked to this transaction in the period." />
              ) : (
                <ul className="divide-y">
                  {d.related_issues.map((i) => (
                    <li key={i.id}>
                      <Link
                        to="/$org/issues/$issueId"
                        params={{ org, issueId: i.id }}
                        className="flex items-center gap-2 px-3 py-2 text-[13px] hover:bg-row-hover"
                      >
                        <LevelDot level={i.level} />
                        <span className="min-w-0 flex-1 truncate">{i.title}</span>
                        <span className="font-mono text-[11px] text-muted-foreground">{i.short_id}</span>
                        <span className="w-12 text-right text-xs text-muted-foreground tabular">{compact(i.count)}</span>
                      </Link>
                    </li>
                  ))}
                </ul>
              )}
            </Panel>
          </div>
          <Panel title="Slowest samples" description="Open a trace to see where the time went" bodyClassName="p-0">
            {!d ? (
              <div className="space-y-2 p-3">
                {Array.from({ length: 4 }, (_, i) => (
                  <Skeleton key={i} className="h-5 w-full" />
                ))}
              </div>
            ) : d.slowest.length === 0 ? (
              <EmptyState compact icon={<Gauge />} title="No samples" />
            ) : (
              <div className="overflow-x-auto">
                <Table aria-label="Slowest samples">
                  <TableHeader>
                    <TableRow className="hover:bg-transparent">
                      <TableHead className="pl-3">Trace</TableHead>
                      <TableHead className="text-right">Duration</TableHead>
                      <TableHead>When</TableHead>
                      <TableHead className="hidden md:table-cell">User</TableHead>
                      <TableHead className="hidden lg:table-cell">Release</TableHead>
                      <TableHead className="pr-3 text-right">Also</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {d.slowest.map((s) => (
                      <TableRow key={s.trace_id}>
                        <TableCell className="pl-3">
                          <Link
                            to="/$org/traces/$traceId"
                            params={{ org, traceId: s.trace_id }}
                            className="inline-flex items-center gap-1.5 font-mono text-[12.5px] text-brand-text hover:underline"
                          >
                            {s.status && s.status !== 'ok' && <span className="size-1.5 rounded-full bg-destructive" aria-label="failed" />}
                            {s.trace_id.slice(0, 16)}
                          </Link>
                        </TableCell>
                        <TableCell className="text-right font-medium tabular">{ms(s.duration_ms)}</TableCell>
                        <TableCell className="text-muted-foreground">{s.timestamp && <RelativeTime date={s.timestamp} suffix plain />}</TableCell>
                        <TableCell className="hidden max-w-48 truncate text-muted-foreground md:table-cell">{s.user ?? '—'}</TableCell>
                        <TableCell className="hidden font-mono text-xs text-muted-foreground lg:table-cell">{s.release ?? '—'}</TableCell>
                        <TableCell className="pr-3 text-right">
                          <span className="inline-flex gap-1">
                            {s.profile_id && s.project_id && (
                              <Tip content="Profile">
                                <Button asChild variant="ghost" size="icon-xs">
                                  <Link
                                    to="/$org/profiles/$project/$profileId"
                                    params={{ org, project: s.project_id, profileId: s.profile_id }}
                                    aria-label="Open profile"
                                  >
                                    <Flame />
                                  </Link>
                                </Button>
                              </Tip>
                            )}
                            {s.replay_id && s.project_id && (
                              <Tip content="Session replay">
                                <Button asChild variant="ghost" size="icon-xs">
                                  <Link
                                    to="/$org/replays/$project/$replayId"
                                    params={{ org, project: s.project_id, replayId: s.replay_id }}
                                    aria-label="Open session replay"
                                  >
                                    <Film />
                                  </Link>
                                </Button>
                              </Tip>
                            )}
                          </span>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            )}
          </Panel>
        </div>
      )}
    </div>
  )
}
