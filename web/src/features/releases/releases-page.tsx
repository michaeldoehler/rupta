import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ArrowDownUp, Package, Search } from 'lucide-react'
import { useDeferredValue, useState } from 'react'
import { api } from '@/api/endpoints'
import { qk } from '@/api/queries'
import { PlatformIcon } from '@/components/common/platform-icon'
import { Sparkline } from '@/components/common/sparkline'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { compact, shortVersion } from '@/lib/format'
import { FilterBar } from '@/features/shell/filter-bar'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { Adoption, CrashFree } from './health'

const SORTS = [
  ['date', 'Date created'],
  ['sessions', 'Sessions'],
  ['crash_free', 'Crash-free (lowest)'],
  ['adoption', 'Adoption'],
] as const

export function ReleasesPage() {
  const { org, projects, environments } = useGlobalFilters()
  const [search, setSearch] = useState('')
  const [sort, setSort] = useState<string>('date')
  const query = useDeferredValue(search)
  const params = { project: projects, environment: environments, query, sort }
  const q = useQuery({ queryKey: qk.releases(org, params), queryFn: () => api.releases.list(org, params), placeholderData: (p) => p })

  return (
    <div>
      <div className="space-y-3 border-b px-3 pt-3 pb-3 sm:px-5">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h1 className="text-lg font-semibold tracking-tight">Releases</h1>
          <FilterBar />
        </div>
        <div className="flex flex-wrap gap-2">
          <div className="relative max-w-md min-w-48 flex-1">
            <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search versions" className="pl-8" aria-label="Search releases" />
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" className="font-normal">
                <ArrowDownUp className="opacity-70" /> {SORTS.find(([k]) => k === sort)?.[1]}
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              <DropdownMenuRadioGroup value={sort} onValueChange={setSort}>
                {SORTS.map(([k, label]) => (
                  <DropdownMenuRadioItem key={k} value={k}>
                    {label}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      {q.isPending ? (
        <ListSkeleton rows={10} />
      ) : q.isError ? (
        <ErrorState error={q.error} retry={() => q.refetch()} />
      ) : q.data.length === 0 ? (
        <EmptyState
          icon={<Package />}
          title={search ? 'No releases match' : 'No releases yet'}
          description={
            search
              ? 'Try another version.'
              : 'Set the release option in your SDK, or create releases with the upload CLI. Release health appears once SDKs send sessions.'
          }
        />
      ) : (
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="pl-4 sm:pl-6">Release</TableHead>
              <TableHead className="hidden md:table-cell">Project</TableHead>
              <TableHead className="text-right">New issues</TableHead>
              <TableHead>Crash-free sessions</TableHead>
              <TableHead className="hidden lg:table-cell">Crash-free users</TableHead>
              <TableHead className="hidden md:table-cell">Adoption</TableHead>
              <TableHead className="hidden xl:table-cell">Sessions</TableHead>
              <TableHead className="hidden lg:table-cell">Last deploy</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {q.data.map((r) => (
              <TableRow key={r.version}>
                <TableCell className="pl-4 sm:pl-6">
                  <Link to="/$org/releases/$version" params={{ org, version: r.version }} className="group block">
                    <span className="font-mono text-[13px] font-medium group-hover:text-brand group-hover:underline">{shortVersion(r.version)}</span>
                    <span className="block text-xs text-muted-foreground">
                      <RelativeTime date={r.date_created} suffix />
                    </span>
                  </Link>
                </TableCell>
                <TableCell className="hidden md:table-cell">
                  <div className="flex flex-col gap-1">
                    {r.projects.map((p) => (
                      <span key={p.id} className="inline-flex items-center gap-1.5 text-[13px]">
                        <PlatformIcon platform={p.platform} size="sm" /> {p.slug}
                      </span>
                    ))}
                  </div>
                </TableCell>
                <TableCell className="text-right tabular">
                  {r.new_issues > 0 ? (
                    <Link to="/$org/issues" params={{ org }} search={{ query: `first_release:${r.version}` }} className="font-medium hover:underline">
                      {r.new_issues}
                    </Link>
                  ) : (
                    <span className="text-muted-foreground">0</span>
                  )}
                </TableCell>
                <TableCell>
                  <CrashFree value={r.crash_free_sessions} />
                </TableCell>
                <TableCell className="hidden lg:table-cell">
                  <CrashFree value={r.crash_free_users} />
                </TableCell>
                <TableCell className="hidden md:table-cell">
                  <Adoption value={r.adoption} />
                </TableCell>
                <TableCell className="hidden xl:table-cell">
                  <div className="flex items-center gap-2">
                    {r.stats && <Sparkline data={r.stats} width={80} height={20} label={`${r.sessions} sessions`} />}
                    <span className="text-xs text-muted-foreground tabular">{compact(r.sessions)}</span>
                  </div>
                </TableCell>
                <TableCell className="hidden lg:table-cell">
                  {r.last_deploy ? (
                    <span className="flex items-center gap-1.5">
                      <Badge variant={r.last_deploy.environment === 'production' ? 'brand' : 'outline'}>{r.last_deploy.environment}</Badge>
                      <span className="text-xs text-muted-foreground">
                        <RelativeTime date={r.last_deploy.date_finished} suffix />
                      </span>
                    </span>
                  ) : (
                    <span className="text-xs text-muted-foreground">not deployed</span>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  )
}
