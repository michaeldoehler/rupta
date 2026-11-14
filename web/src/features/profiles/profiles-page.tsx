import { useQuery } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { ChevronRight, Download, Flame, Route as RouteIcon, Search } from 'lucide-react'
import { lazy, Suspense, useDeferredValue, useMemo, useState } from 'react'
import { useProjects } from '@/api/queries'
import { telemetryApi, tk, useInfiniteList } from '@/api/telemetry'
import type { Flamegraph, ProfileRow } from '@/api/telemetry-types'
import { PlatformIcon } from '@/components/common/platform-icon'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { absolute, compact } from '@/lib/format'
import { cn } from '@/lib/utils'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { ms } from '@/features/telemetry/format'
import { ExploreHeader, InfiniteVirtualList, SectionTabs, StatTile } from '@/features/telemetry/ui'

const FlamegraphView = lazy(() => import('./flamegraph-view'))

const PROFILE_TABS = (org: string) => [
  { to: '/$org/profiles' as const, params: { org }, label: 'Profiles', exact: true },
  { to: '/$org/profiles/transactions' as const, params: { org }, label: 'By transaction' },
]

const COLS = 'grid grid-cols-[minmax(0,1fr)_80px_90px] items-center gap-3 px-4 md:grid-cols-[minmax(0,1fr)_120px_90px_110px_120px_100px] sm:px-6'

function ProfileListRow({ p, org, project, focused }: { p: ProfileRow; org: string; project?: { slug: string; platform: string | null }; focused: boolean }) {
  return (
    <Link
      to="/$org/profiles/$project/$profileId"
      params={{ org, project: p.project_id, profileId: p.profile_id }}
      className={cn(COLS, 'h-full border-b text-[13px] hover:bg-row-hover focus-visible:bg-row-hover focus-visible:outline-none', focused && 'bg-row-selected')}
    >
      <span className="flex min-w-0 items-center gap-2" role="gridcell">
        <PlatformIcon platform={p.platform ?? project?.platform} size="sm" />
        <span className="truncate font-mono font-medium">{p.transaction_name ?? 'Continuous profile'}</span>
        {p.kind === 'continuous' && <Badge variant="outline">continuous</Badge>}
      </span>
      <span role="gridcell" className="hidden truncate text-muted-foreground md:block">
        {project?.slug}
      </span>
      <span role="gridcell" className="text-right font-medium tabular">
        {ms(p.duration_ms)}
      </span>
      <span role="gridcell" className="hidden truncate font-mono text-xs text-muted-foreground md:block">
        {p.release ?? '—'}
      </span>
      <span role="gridcell" className="hidden text-muted-foreground md:block">
        {p.environment ?? '—'}
      </span>
      <span role="gridcell" className="text-right text-muted-foreground">
        <RelativeTime date={p.start} suffix plain />
      </span>
    </Link>
  )
}

export function ProfilesPage() {
  const { org, projects, environments, period } = useGlobalFilters()
  const navigate = useNavigate()
  const { data: projectList } = useProjects(org)
  const [text, setText] = useState('')
  const query = useDeferredValue(text.trim())
  const params = { project: projects, environment: environments, period, query }
  const list = useInfiniteList(tk.profiles(org, params), (cursor, signal) => telemetryApi.profiles.list(org, { ...params, cursor, limit: 100 }, signal))
  const rows = useMemo(() => list.data?.pages.flatMap((p) => p.data) ?? [], [list.data])
  const [focused, setFocused] = useState(-1)
  useHotkeys(
    {
      j: () => setFocused((f) => Math.min(rows.length - 1, f + 1)),
      k: () => setFocused((f) => Math.max(0, f - 1)),
      Enter: () =>
        rows[focused] &&
        void navigate({ to: '/$org/profiles/$project/$profileId', params: { org, project: rows[focused].project_id, profileId: rows[focused].profile_id } }),
    },
    [rows, focused],
  )
  return (
    <div className="flex h-full flex-col">
      <ExploreHeader title="Profiles" tabs={<SectionTabs label="Profiling sections" items={PROFILE_TABS(org)} />}>
        <div className="relative max-w-md">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="Filter by transaction"
            className="pl-8"
            aria-label="Filter profiles by transaction"
          />
        </div>
      </ExploreHeader>
      {list.isPending ? (
        <ListSkeleton rows={12} />
      ) : list.isError ? (
        <ErrorState error={list.error} retry={() => list.refetch()} />
      ) : rows.length === 0 ? (
        <EmptyState
          icon={<Flame />}
          title={text ? 'No profiles match' : 'No profiles yet'}
          description={
            text
              ? 'Try another transaction or a longer period.'
              : 'Enable profiling in your SDK (for example profilesSampleRate or profiles_sample_rate together with tracing).'
          }
        />
      ) : (
        <InfiniteVirtualList
          label="Profiles"
          rows={rows}
          rowHeight={40}
          getKey={(p) => p.profile_id}
          hasNextPage={list.hasNextPage}
          fetchNextPage={() => void list.fetchNextPage()}
          isFetchingNextPage={list.isFetchingNextPage}
          focusedIndex={focused}
          header={
            <div
              className={cn(
                COLS,
                'sticky top-0 z-10 h-8 border-b bg-background/95 text-[11px] font-medium tracking-wide text-muted-foreground uppercase backdrop-blur',
              )}
              role="row"
            >
              <span role="columnheader">Transaction</span>
              <span role="columnheader" className="hidden md:block">
                Project
              </span>
              <span role="columnheader" className="text-right">
                Duration
              </span>
              <span role="columnheader" className="hidden md:block">
                Release
              </span>
              <span role="columnheader" className="hidden md:block">
                Environment
              </span>
              <span role="columnheader" className="text-right">
                When
              </span>
            </div>
          }
          render={(p, i) => <ProfileListRow p={p} org={org} project={projectList?.find((x) => x.id === p.project_id)} focused={i === focused} />}
        />
      )}
    </div>
  )
}

/** Profiles grouped by transaction, linking to the aggregated flamegraph. */
export function ProfileTransactionsPage() {
  const { org, projects, environments, period } = useGlobalFilters()
  const { data: projectList } = useProjects(org)
  const params = { project: projects, environment: environments, period }
  const q = useQuery({
    queryKey: tk.profiles(org, { ...params, all: true }),
    queryFn: ({ signal }) => telemetryApi.profiles.list(org, { ...params, limit: 1000 }, signal),
    staleTime: 60_000,
  })
  const groups = useMemo(() => {
    const m = new Map<string, { name: string; project_id: string; count: number; durations: number[]; last: string; platform: string | null }>()
    for (const p of q.data?.data ?? []) {
      const key = `${p.project_id}:${p.transaction_name}`
      const g = m.get(key) ?? { name: p.transaction_name ?? '(none)', project_id: p.project_id, count: 0, durations: [], last: p.start, platform: p.platform }
      g.count += 1
      g.durations.push(p.duration_ms)
      if (p.start > g.last) g.last = p.start
      m.set(key, g)
    }
    return [...m.values()].sort((a, b) => b.count - a.count)
  }, [q.data])
  return (
    <div>
      <ExploreHeader title="Profiles" tabs={<SectionTabs label="Profiling sections" items={PROFILE_TABS(org)} />} />
      {q.isPending ? (
        <ListSkeleton rows={8} />
      ) : q.isError ? (
        <ErrorState error={q.error} retry={() => q.refetch()} />
      ) : groups.length === 0 ? (
        <EmptyState icon={<Flame />} title="No profiles yet" description="Profiles are grouped by the transaction they were taken in." />
      ) : (
        <ul className="divide-y">
          {groups.map((g) => {
            const sorted = [...g.durations].sort((a, b) => a - b)
            const p50 = sorted[Math.floor(sorted.length / 2)]
            return (
              <li key={`${g.project_id}:${g.name}`}>
                <Link
                  to="/$org/profiles/aggregate"
                  params={{ org }}
                  search={(prev: Record<string, unknown>) => ({ ...prev, transaction: g.name, tx_project: g.project_id })}
                  className="flex items-center gap-3 px-4 py-2.5 text-[13px] hover:bg-row-hover sm:px-6"
                >
                  <PlatformIcon platform={g.platform} size="sm" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-mono font-medium">{g.name}</span>
                    <span className="text-xs text-muted-foreground">
                      {projectList?.find((p) => p.id === g.project_id)?.slug} · last <RelativeTime date={g.last} suffix plain />
                    </span>
                  </span>
                  <span className="hidden w-28 text-right text-muted-foreground tabular sm:block">p50 {ms(p50)}</span>
                  <span className="w-24 text-right tabular">{compact(g.count)} profiles</span>
                  <ChevronRight className="size-4 text-muted-foreground" />
                </Link>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}

function ViewerFallback() {
  return (
    <div className="space-y-3" aria-busy="true" aria-label="Loading flamegraph">
      <Skeleton className="h-7 w-2/3" />
      <Skeleton className="h-72 w-full" />
    </div>
  )
}

function FlamegraphSection({
  q,
  durationMs,
}: {
  q: { data?: Flamegraph; isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown }
  durationMs?: number | null
}) {
  if (q.isError) return <ErrorState error={q.error} retry={() => q.refetch()} />
  if (q.isPending || !q.data) return <ViewerFallback />
  if (!q.data.root.total_weight) return <EmptyState icon={<Flame />} title="No samples" description="The profile has no samples on the profiled thread." />
  return (
    <Suspense fallback={<ViewerFallback />}>
      <FlamegraphView flamegraph={q.data} durationMs={durationMs} />
    </Suspense>
  )
}

export function ProfilePage({ project, profileId }: { project: string; profileId: string }) {
  const { org, projects, period } = useGlobalFilters()
  const { data: projectList } = useProjects(org)
  const q = useQuery({
    queryKey: tk.flamegraph(org, project, profileId),
    queryFn: () => telemetryApi.profiles.flamegraph(org, project, profileId),
    staleTime: Infinity,
  })
  // The list row has the metadata; the flamegraph endpoint has the tree.
  const meta = useQuery({
    queryKey: tk.profiles(org, { find: profileId, projects, period }),
    queryFn: async () =>
      (await telemetryApi.profiles.list(org, { project: [project].filter((p) => /^\d+$/.test(p)), period: '90d', limit: 100 })).data.find(
        (p) => p.profile_id === profileId,
      ) ?? null,
    staleTime: 60_000,
  })
  const m = meta.data
  const proj = projectList?.find((p) => p.id === project || p.slug === project)
  return (
    <div>
      <ExploreHeader
        title={<span className="font-mono">{m?.transaction_name ?? 'Profile'}</span>}
        description={
          <span className="flex flex-wrap items-center gap-1.5">
            <Link to="/$org/profiles" params={{ org }} className="hover:text-foreground">
              Profiles
            </Link>
            <ChevronRight className="size-3" />
            <span className="font-mono">{profileId.slice(0, 12)}</span>
            {proj && (
              <>
                <ChevronRight className="size-3" />
                <PlatformIcon platform={proj.platform} size="sm" /> {proj.slug}
              </>
            )}
          </span>
        }
        actions={
          <>
            {m?.trace_id && (
              <Button asChild variant="outline" size="sm">
                <Link to="/$org/traces/$traceId" params={{ org, traceId: m.trace_id }}>
                  <RouteIcon /> Trace
                </Link>
              </Button>
            )}
            {m?.transaction_name && (
              <Button asChild variant="outline" size="sm">
                <Link
                  to="/$org/profiles/aggregate"
                  params={{ org }}
                  search={(prev: Record<string, unknown>) => ({ ...prev, transaction: m.transaction_name!, tx_project: m.project_id })}
                >
                  <Flame /> Aggregated
                </Link>
              </Button>
            )}
            <Button asChild variant="outline" size="sm">
              <a href={telemetryApi.profiles.rawUrl(org, project, profileId)} download={`${profileId}.json`}>
                <Download /> Raw
              </a>
            </Button>
          </>
        }
      />
      <div className="space-y-3 p-3 sm:p-5">
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
          <StatTile label="Duration" value={m ? ms(m.duration_ms) : '—'} />
          <StatTile
            label="Samples"
            value={q.data ? q.data.samples.toLocaleString('en-US') : '—'}
            hint={q.data && m ? `≈ ${ms(m.duration_ms / Math.max(1, q.data.samples))} per sample` : undefined}
          />
          <StatTile label="Release" value={<span className="font-mono text-sm">{m?.release ?? '—'}</span>} hint={m?.environment ?? undefined} />
          <StatTile label="Recorded" value={m ? <RelativeTime date={m.start} suffix /> : '—'} hint={m ? absolute(m.start) : undefined} />
        </div>
        <FlamegraphSection q={q} durationMs={m?.duration_ms} />
      </div>
    </div>
  )
}

export function AggregateProfilePage({ transaction, projectId }: { transaction: string; projectId?: string }) {
  const { org, projects, environments, period } = useGlobalFilters()
  const scope = projectId ? [projectId] : projects
  const params = { transaction, project: scope, environment: environments, period }
  const q = useQuery({
    queryKey: tk.aggregate(org, params),
    queryFn: () => telemetryApi.profiles.aggregate(org, params),
    enabled: Boolean(transaction),
    staleTime: 60_000,
  })
  return (
    <div>
      <ExploreHeader
        title={<span className="font-mono">{transaction || 'Aggregated flamegraph'}</span>}
        description={
          <span className="flex items-center gap-1.5">
            <Link to="/$org/profiles/transactions" params={{ org }} className="hover:text-foreground">
              Profiles
            </Link>
            <ChevronRight className="size-3" />
            Aggregated flamegraph{q.data ? ` · ${q.data.profiles} profiles · ${q.data.samples.toLocaleString('en-US')} samples` : ''}
          </span>
        }
        actions={
          <Button asChild variant="outline" size="sm">
            <Link
              to="/$org/performance/summary"
              params={{ org }}
              search={(prev: Record<string, unknown>) => ({ ...prev, name: transaction, tx_project: projectId })}
            >
              Transaction summary
            </Link>
          </Button>
        }
      />
      <div className="p-3 sm:p-5">
        <p className="mb-3 text-[13px] text-muted-foreground">
          Merged from the latest profiles (at most 50) of this transaction in the period. Widths are shares of all samples; time order is the order calls were
          first seen across profiles.
        </p>
        <FlamegraphSection q={q} />
      </div>
    </div>
  )
}
