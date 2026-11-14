import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ChevronRight, GitCommitHorizontal, Rocket } from 'lucide-react'
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { api } from '@/api/endpoints'
import { qk } from '@/api/queries'
import { CopyButton } from '@/components/common/copy-button'
import { LevelBar } from '@/components/common/level'
import { PlatformIcon } from '@/components/common/platform-icon'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Badge } from '@/components/ui/badge'
import { Skeleton } from '@/components/ui/skeleton'
import { absolute, compact, shortVersion, thousands } from '@/lib/format'
import { Adoption, CrashFree } from './health'

function StatCard({ label, children, hint }: { label: string; children: React.ReactNode; hint?: string }) {
  return (
    <div className="rounded-lg border bg-card px-3 py-2.5">
      <div className="text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{label}</div>
      <div className="mt-1 text-lg font-semibold">{children}</div>
      {hint && <div className="text-xs text-muted-foreground">{hint}</div>}
    </div>
  )
}

export function ReleaseDetailPage({ org, version }: { org: string; version: string }) {
  const q = useQuery({ queryKey: qk.release(org, version), queryFn: () => api.releases.get(org, version) })
  const issues = useQuery({
    queryKey: qk.issues.list(org, { query: `first_release:${version}`, sort: 'freq' }),
    queryFn: () => api.issues.list(org, { query: `first_release:${version}`, sort: 'freq', limit: 25 }),
  })
  if (q.isError) return <ErrorState error={q.error} retry={() => q.refetch()} className="min-h-[60vh]" />
  const r = q.data
  const chart = (r?.health ?? []).map((h) => ({
    ts: h.ts * 1000,
    healthy: Math.max(0, h.total - h.errored - h.crashed),
    errored: h.errored,
    crashed: h.crashed,
  }))
  return (
    <div className="pb-10">
      <header className="border-b px-4 pt-3 pb-4 sm:px-6">
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Link to="/$org/releases" params={{ org }} className="hover:text-foreground">
            Releases
          </Link>
          <ChevronRight className="size-3" />
          <span className="font-mono text-foreground">{version}</span>
          <CopyButton value={version} label="Copy version" />
        </div>
        {r ? (
          <div className="mt-2 flex flex-wrap items-end justify-between gap-3">
            <div>
              <h1 className="font-mono text-xl font-semibold tracking-tight">{shortVersion(r.version)}</h1>
              <p className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[13px] text-muted-foreground">
                <span>
                  Created <RelativeTime date={r.date_created} suffix />
                </span>
                {r.date_released && <span>Released {absolute(r.date_released)}</span>}
                {r.ref && (
                  <span className="inline-flex items-center gap-1 font-mono text-xs">
                    <GitCommitHorizontal className="size-3.5" />
                    {r.ref.slice(0, 10)}
                  </span>
                )}
                {r.projects.map((p) => (
                  <span key={p.id} className="inline-flex items-center gap-1">
                    <PlatformIcon platform={p.platform} size="sm" /> {p.slug}
                  </span>
                ))}
              </p>
            </div>
          </div>
        ) : (
          <Skeleton className="mt-2 h-7 w-64" />
        )}
      </header>
      {r ? (
        <div className="space-y-6 px-4 pt-5 sm:px-6">
          <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
            <StatCard label="Crash-free sessions">
              <CrashFree value={r.crash_free_sessions} />
            </StatCard>
            <StatCard label="Crash-free users">
              <CrashFree value={r.crash_free_users} />
            </StatCard>
            <StatCard label="Sessions">
              <span className="tabular">{compact(r.sessions)}</span>
            </StatCard>
            <StatCard label="Users">
              <span className="tabular">{compact(r.users)}</span>
            </StatCard>
            <StatCard label="Adoption">
              <Adoption value={r.adoption} />
            </StatCard>
            <StatCard label="New issues">
              <span className="tabular">{r.new_issues}</span>
            </StatCard>
          </div>

          <section>
            <h2 className="mb-2 text-[13px] font-semibold">Sessions</h2>
            <div className="h-56 rounded-lg border bg-card p-3" role="img" aria-label="Sessions over time: healthy, errored and crashed">
              {chart.length ? (
                <ResponsiveContainer width="100%" height="100%">
                  <AreaChart data={chart} margin={{ top: 4, right: 8, bottom: 0, left: 0 }}>
                    <CartesianGrid vertical={false} stroke="var(--border)" />
                    <XAxis
                      dataKey="ts"
                      type="number"
                      domain={['dataMin', 'dataMax']}
                      tickFormatter={(v: number) => absolute(v, { date: true }).split(',')[0]}
                      tick={{ fontSize: 11, fill: 'var(--muted-foreground)' }}
                      axisLine={false}
                      tickLine={false}
                      minTickGap={40}
                    />
                    <YAxis
                      tickFormatter={(v: number) => compact(v)}
                      tick={{ fontSize: 11, fill: 'var(--muted-foreground)' }}
                      axisLine={false}
                      tickLine={false}
                      width={40}
                    />
                    <Tooltip
                      isAnimationActive={false}
                      content={({ active, payload, label }) =>
                        active && payload?.length ? (
                          <div className="rounded-md border bg-popover px-2.5 py-1.5 text-xs shadow-md">
                            <div className="mb-1 text-muted-foreground">{absolute(label as number)}</div>
                            {payload.map((p) => (
                              <div key={String(p.dataKey)} className="flex items-center gap-2">
                                <span className="size-2 rounded-sm" style={{ background: p.color }} />
                                <span className="capitalize">{String(p.dataKey)}</span>
                                <span className="ml-auto pl-3 font-medium tabular">{thousands(p.value as number)}</span>
                              </div>
                            ))}
                          </div>
                        ) : null
                      }
                    />
                    <Area
                      type="monotone"
                      dataKey="healthy"
                      stackId="1"
                      stroke="var(--chart-3)"
                      fill="var(--chart-3)"
                      fillOpacity={0.25}
                      isAnimationActive={false}
                    />
                    <Area
                      type="monotone"
                      dataKey="errored"
                      stackId="1"
                      stroke="var(--chart-4)"
                      fill="var(--chart-4)"
                      fillOpacity={0.4}
                      isAnimationActive={false}
                    />
                    <Area
                      type="monotone"
                      dataKey="crashed"
                      stackId="1"
                      stroke="var(--destructive)"
                      fill="var(--destructive)"
                      fillOpacity={0.5}
                      isAnimationActive={false}
                    />
                  </AreaChart>
                </ResponsiveContainer>
              ) : (
                <EmptyState compact title="No session data" description="Release health needs SDKs that send sessions." />
              )}
            </div>
          </section>

          <div className="grid gap-6 lg:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
            <section>
              <h2 className="mb-2 text-[13px] font-semibold">New issues in this release</h2>
              <div className="overflow-hidden rounded-lg border bg-card">
                {issues.isPending ? (
                  <ListSkeleton rows={4} />
                ) : issues.data?.data.length ? (
                  <ul className="divide-y">
                    {issues.data.data.map((i) => (
                      <li key={i.id}>
                        <Link to="/$org/issues/$issueId" params={{ org, issueId: i.id }} className="flex items-center gap-3 px-3 py-2 hover:bg-row-hover">
                          <LevelBar level={i.level} className="h-8" />
                          <div className="min-w-0 flex-1">
                            <div className="truncate text-[13px] font-medium">{i.title}</div>
                            <div className="font-mono text-[11px] text-muted-foreground">{i.short_id}</div>
                          </div>
                          <span className="text-[13px] tabular">{compact(i.count)}</span>
                        </Link>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <EmptyState compact title="No new issues" description="Nothing first appeared in this release." />
                )}
              </div>
            </section>
            <section>
              <h2 className="mb-2 text-[13px] font-semibold">Deploys</h2>
              <ol className="space-y-2">
                {r.deploys.length === 0 && <li className="text-[13px] text-muted-foreground">No deploys recorded.</li>}
                {r.deploys.map((d) => (
                  <li key={d.id} className="flex items-center gap-3 rounded-lg border bg-card px-3 py-2">
                    <Rocket className="size-4 text-muted-foreground" />
                    <div className="min-w-0 flex-1">
                      <Badge variant={d.environment === 'production' ? 'brand' : 'outline'}>{d.environment}</Badge>
                      {d.name && <span className="ml-2 text-[13px]">{d.name}</span>}
                    </div>
                    <span className="text-xs text-muted-foreground">
                      <RelativeTime date={d.date_finished} suffix />
                    </span>
                  </li>
                ))}
              </ol>
            </section>
          </div>
        </div>
      ) : (
        <div className="space-y-4 p-6">
          <Skeleton className="h-20" />
          <Skeleton className="h-56" />
        </div>
      )}
    </div>
  )
}
