import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ChevronRight } from 'lucide-react'
import { api } from '@/api/endpoints'
import { qk, useApiMutation, useProjects } from '@/api/queries'
import { CodeBlock } from '@/components/common/code-block'
import { PlatformIcon } from '@/components/common/platform-icon'
import { ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { absolute, duration, percent } from '@/lib/format'
import { cn } from '@/lib/utils'
import { CHECKIN_COLOR, CHECKIN_LABEL, CheckinTimeline } from './checkin-timeline'
import { describeSchedule } from './cron-utils'
import { HealthBadge } from './health-badge'

export function MonitorPage({ org, project, slug }: { org: string; project: string; slug: string }) {
  const projects = useProjects(org)
  const ref = projects.data?.find((p) => p.slug === project)
  const m = useQuery({ queryKey: qk.monitor(org, project, slug), queryFn: () => api.projects.monitor(org, ref!, slug), enabled: Boolean(ref) })
  const checkins = useInfiniteQuery({
    queryKey: qk.checkins(org, project, slug),
    queryFn: ({ pageParam }) => api.projects.checkins(org, project, slug, { cursor: pageParam ?? undefined }),
    initialPageParam: null as string | null,
    getNextPageParam: (l) => l.next_cursor,
  })
  const toggle = useApiMutation((status: 'active' | 'disabled') => api.projects.updateMonitor(org, project, slug, { status }), {
    invalidate: [qk.monitor(org, project, slug), qk.monitors(org, project)],
    success: (_d, s) => (s === 'disabled' ? 'Monitor disabled' : 'Monitor enabled'),
  })
  if (m.isError) return <ErrorState error={m.error} retry={() => m.refetch()} className="min-h-[60vh]" />
  const d = m.data
  const rows = checkins.data?.pages.flatMap((p) => p.data) ?? []
  const okRate = rows.length ? (rows.filter((c) => c.status === 'ok').length / rows.length) * 100 : null
  const durations = rows.filter((c) => c.duration_ms != null).map((c) => c.duration_ms!)
  const avg = durations.length ? durations.reduce((a, b) => a + b, 0) / durations.length : null

  return (
    <div className="pb-10">
      <header className="border-b px-4 pt-3 pb-4 sm:px-6">
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Link to="/$org/crons" params={{ org }} className="hover:text-foreground">
            Crons
          </Link>
          <ChevronRight className="size-3" />
          <span className="font-mono">{slug}</span>
        </div>
        {d ? (
          <div className="mt-2 flex flex-wrap items-start justify-between gap-3">
            <div>
              <div className="flex items-center gap-2">
                <h1 className="text-xl font-semibold tracking-tight">{d.name}</h1>
                <HealthBadge monitor={d} />
              </div>
              <p className="mt-1 flex flex-wrap items-center gap-x-3 text-[13px] text-muted-foreground">
                <span className="inline-flex items-center gap-1">
                  <PlatformIcon platform={d.project.platform} size="sm" /> {d.project.slug}
                </span>
                <span>{describeSchedule(d.schedule)}</span>
                <span className="font-mono text-xs">{d.schedule.type === 'crontab' ? d.schedule.value : `${d.schedule.value} ${d.schedule.unit}`}</span>
                <span>{d.timezone}</span>
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={() => toggle.mutate(d.status === 'active' ? 'disabled' : 'active')} disabled={toggle.isPending}>
              {d.status === 'active' ? 'Disable' : 'Enable'}
            </Button>
          </div>
        ) : (
          <Skeleton className="mt-2 h-7 w-72" />
        )}
      </header>
      {d && (
        <div className="space-y-6 px-4 pt-5 sm:px-6">
          <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
            {(
              [
                ['Last check-in', d.last_checkin ? <RelativeTime key="l" date={d.last_checkin} suffix /> : '—'],
                ['Next expected', d.next_checkin && d.status === 'active' ? absolute(d.next_checkin) : '—'],
                ['Success rate', percent(okRate, 1)],
                ['Average duration', duration(avg)],
              ] as const
            ).map(([label, value]) => (
              <div key={label} className="rounded-lg border bg-card px-3 py-2.5">
                <div className="text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{label}</div>
                <div className="mt-1 text-[15px] font-semibold">{value}</div>
              </div>
            ))}
          </div>
          <section>
            <h2 className="mb-2 text-[13px] font-semibold">Timeline</h2>
            <div className="overflow-x-auto rounded-lg border bg-card p-3">
              <CheckinTimeline checkins={rows.length ? rows : d.recent_checkins} max={60} tall />
              <div className="mt-2 flex flex-wrap gap-3 text-xs text-muted-foreground">
                {(['ok', 'error', 'missed', 'timeout'] as const).map((s) => (
                  <span key={s} className="inline-flex items-center gap-1.5">
                    <span className={cn('size-2 rounded-sm', CHECKIN_COLOR[s])} /> {CHECKIN_LABEL[s]}
                  </span>
                ))}
                <span className="ml-auto">
                  Margin {d.checkin_margin ?? 0} min · max runtime {d.max_runtime ?? '—'} min
                </span>
              </div>
            </div>
          </section>
          <section>
            <h2 className="mb-2 text-[13px] font-semibold">Check-ins</h2>
            <div className="overflow-hidden rounded-lg border bg-card">
              {checkins.isPending ? (
                <ListSkeleton rows={5} />
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Status</TableHead>
                      <TableHead>Started</TableHead>
                      <TableHead>Duration</TableHead>
                      <TableHead className="hidden sm:table-cell">Environment</TableHead>
                      <TableHead className="hidden md:table-cell">Check-in id</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {rows.map((c) => (
                      <TableRow key={c.id}>
                        <TableCell>
                          <span className="inline-flex items-center gap-2">
                            <span className={cn('size-2 rounded-full', CHECKIN_COLOR[c.status])} />
                            {CHECKIN_LABEL[c.status]}
                          </span>
                        </TableCell>
                        <TableCell className="text-muted-foreground">{absolute(c.created_at, { seconds: true })}</TableCell>
                        <TableCell className="tabular">{duration(c.duration_ms)}</TableCell>
                        <TableCell className="hidden sm:table-cell">{c.environment ?? '—'}</TableCell>
                        <TableCell className="hidden font-mono text-xs text-muted-foreground md:table-cell">{c.id.slice(0, 12)}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </div>
            {checkins.hasNextPage && (
              <Button variant="outline" size="sm" className="mt-2" onClick={() => checkins.fetchNextPage()}>
                Load older check-ins
              </Button>
            )}
          </section>
          <section>
            <h2 className="mb-2 text-[13px] font-semibold">Sending check-ins</h2>
            <p className="mb-2 text-[13px] text-muted-foreground">
              Wrap the job with the SDK's cron monitoring helper and the monitor slug. Example for Python:
            </p>
            <CodeBlock lang="python" code={`from sentry_sdk.crons import monitor\n\n@monitor(monitor_slug="${d.slug}")\ndef run():\n    ...`} />
          </section>
        </div>
      )}
    </div>
  )
}
