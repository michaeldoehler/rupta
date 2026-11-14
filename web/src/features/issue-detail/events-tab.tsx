import { useInfiniteQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { Search } from 'lucide-react'
import { useState } from 'react'
import { api } from '@/api/endpoints'
import { qk } from '@/api/queries'
import { LevelDot } from '@/components/common/level'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { shortVersion } from '@/lib/format'

export function EventsTab({ org, issueId }: { org: string; issueId: string }) {
  const [draft, setDraft] = useState('')
  const [query, setQuery] = useState('')
  const q = useInfiniteQuery({
    queryKey: qk.issues.events(issueId, query),
    queryFn: ({ pageParam }) => api.issues.events(issueId, { query, cursor: pageParam ?? undefined, limit: 50 }),
    initialPageParam: null as string | null,
    getNextPageParam: (l) => l.next_cursor,
  })
  const rows = q.data?.pages.flatMap((p) => p.data) ?? []
  return (
    <div className="space-y-3 px-4 py-4 sm:px-6">
      <form
        className="relative max-w-xl"
        onSubmit={(e) => {
          e.preventDefault()
          setQuery(draft.trim())
        }}
      >
        <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Filter events: environment:production browser:Safari user.email:…"
          className="pl-8 font-mono text-[13px]"
          aria-label="Filter events"
        />
      </form>
      <div className="overflow-hidden rounded-lg border bg-card">
        {q.isPending ? (
          <ListSkeleton rows={8} />
        ) : q.isError ? (
          <ErrorState error={q.error} retry={() => q.refetch()} />
        ) : rows.length === 0 ? (
          <EmptyState compact icon={<Search />} title="No events match" description="Events may have been removed by the retention policy." />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Event</TableHead>
                <TableHead>Time</TableHead>
                <TableHead className="hidden md:table-cell">Environment</TableHead>
                <TableHead className="hidden lg:table-cell">Release</TableHead>
                <TableHead className="hidden md:table-cell">User</TableHead>
                <TableHead className="hidden xl:table-cell">Browser / OS</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((e) => (
                <TableRow key={e.event_id}>
                  <TableCell className="max-w-0 min-w-56">
                    <Link to="/$org/issues/$issueId" params={{ org, issueId }} search={{ event: e.event_id }} className="group flex min-w-0 items-center gap-2">
                      <LevelDot level={e.level} />
                      <span className="font-mono text-xs text-brand-text group-hover:underline">{e.event_id.slice(0, 8)}</span>
                      <span className="truncate text-muted-foreground">{e.message ?? e.transaction}</span>
                    </Link>
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-muted-foreground">
                    <RelativeTime date={e.timestamp} suffix />
                  </TableCell>
                  <TableCell className="hidden md:table-cell">{e.environment}</TableCell>
                  <TableCell className="hidden font-mono text-xs lg:table-cell">{e.release ? shortVersion(e.release) : '—'}</TableCell>
                  <TableCell className="hidden max-w-48 truncate md:table-cell">
                    {e.user?.email ?? e.user?.id ?? e.user?.ip_address ?? <span className="text-muted-foreground">—</span>}
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground xl:table-cell">{[e.browser, e.os].filter(Boolean).join(' · ') || '—'}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </div>
      {q.hasNextPage && (
        <Button variant="outline" size="sm" onClick={() => q.fetchNextPage()} disabled={q.isFetchingNextPage}>
          Load more
        </Button>
      )}
    </div>
  )
}
