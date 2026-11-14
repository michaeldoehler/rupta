import { useQueries } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { Clock } from 'lucide-react'
import { api } from '@/api/endpoints'
import { qk, useProjects } from '@/api/queries'
import type { Monitor } from '@/api/types'
import { PlatformIcon } from '@/components/common/platform-icon'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { FilterBar } from '@/features/shell/filter-bar'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { CheckinTimeline } from './checkin-timeline'
import { describeSchedule } from './cron-utils'
import { HealthBadge } from './health-badge'

export function CronsPage() {
  const { org, projects: selected } = useGlobalFilters()
  const projects = useProjects(org)
  const targets = (projects.data ?? []).filter((p) => !selected.length || selected.includes(p.id))
  const results = useQueries({ queries: targets.map((p) => ({ queryKey: qk.monitors(org, p.slug), queryFn: () => api.projects.monitors(org, p) })) })
  const pending = projects.isPending || results.some((r) => r.isPending)
  const error = projects.error ?? results.find((r) => r.error)?.error
  const order: Record<string, number> = { error: 0, timeout: 0, missed: 1, ok: 2, unknown: 3 }
  const monitors: Monitor[] = results
    .flatMap((r) => r.data ?? [])
    .sort((a, b) => (a.status === 'disabled' ? 9 : order[a.health]) - (b.status === 'disabled' ? 9 : order[b.health]) || a.name.localeCompare(b.name))
  const failing = monitors.filter((m) => m.status === 'active' && m.health !== 'ok' && m.health !== 'unknown').length

  return (
    <div>
      <div className="flex flex-wrap items-center justify-between gap-2 border-b px-3 py-3 sm:px-5">
        <div>
          <h1 className="text-lg font-semibold tracking-tight">Crons</h1>
          {!pending && (
            <p className="text-[13px] text-muted-foreground">
              {monitors.length} monitors
              {failing > 0 && (
                <>
                  , <span className="font-medium text-destructive">{failing} need attention</span>
                </>
              )}
            </p>
          )}
        </div>
        <FilterBar showEnvironment={false} showPeriod={false} />
      </div>
      {pending ? (
        <ListSkeleton rows={6} />
      ) : error ? (
        <ErrorState error={error} />
      ) : monitors.length === 0 ? (
        <EmptyState
          icon={<Clock />}
          title="No cron monitors"
          description="Monitors are created automatically when an SDK sends its first check-in for a monitor slug, for example from a scheduled job wrapped with the SDK's cron helper."
        />
      ) : (
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="pl-4 sm:pl-6">Monitor</TableHead>
              <TableHead className="hidden md:table-cell">Schedule</TableHead>
              <TableHead>Recent check-ins</TableHead>
              <TableHead className="hidden lg:table-cell">Last check-in</TableHead>
              <TableHead className="hidden lg:table-cell">Next expected</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {monitors.map((m) => (
              <TableRow key={m.id} className={m.status === 'disabled' ? 'opacity-60' : undefined}>
                <TableCell className="pl-4 sm:pl-6">
                  <Link to="/$org/crons/$project/$slug" params={{ org, project: m.project.slug, slug: m.slug }} className="group flex items-center gap-2.5">
                    <HealthBadge monitor={m} className="w-[68px] justify-center" />
                    <div className="min-w-0">
                      <div className="truncate font-medium group-hover:underline">{m.name}</div>
                      <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                        <PlatformIcon platform={m.project.platform} size="sm" /> {m.project.slug} · <span className="font-mono">{m.slug}</span>
                      </div>
                    </div>
                  </Link>
                </TableCell>
                <TableCell className="hidden md:table-cell">
                  <div>{describeSchedule(m.schedule)}</div>
                  <div className="font-mono text-xs text-muted-foreground">
                    {m.schedule.type === 'crontab' ? m.schedule.value : `interval`} · {m.timezone}
                  </div>
                </TableCell>
                <TableCell>
                  <CheckinTimeline checkins={m.recent_checkins} max={30} />
                </TableCell>
                <TableCell className="hidden text-muted-foreground lg:table-cell">
                  {m.last_checkin ? <RelativeTime date={m.last_checkin} suffix /> : '—'}
                </TableCell>
                <TableCell className="hidden text-muted-foreground lg:table-cell">
                  {m.next_checkin && m.status === 'active' ? <RelativeTime date={m.next_checkin} /> : '—'}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  )
}
