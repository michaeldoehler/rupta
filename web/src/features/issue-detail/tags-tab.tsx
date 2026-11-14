import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ChevronRight, Tags } from 'lucide-react'
import { api } from '@/api/endpoints'
import { qk } from '@/api/queries'
import type { TagDistribution } from '@/api/types'
import { CardsSkeleton, EmptyState, ErrorState } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { compact, percent, thousands } from '@/lib/format'
import { formatFilter } from '@/lib/search/parse'

const BAR_COLORS = ['bg-chart-1', 'bg-chart-2', 'bg-chart-3', 'bg-chart-4', 'bg-chart-5']

export function DistributionBar({ tag }: { tag: TagDistribution }) {
  const total = Math.max(1, tag.total_values)
  const top = tag.top_values.slice(0, 5)
  const other = Math.max(0, total - top.reduce((s, v) => s + v.count, 0))
  return (
    <div className="flex h-2 w-full overflow-hidden rounded-full bg-muted" aria-hidden>
      {top.map((v, i) => (
        <span key={v.value} className={BAR_COLORS[i]} style={{ width: `${(v.count / total) * 100}%` }} />
      ))}
      {other > 0 && <span className="bg-muted-foreground/25" style={{ width: `${(other / total) * 100}%` }} />}
    </div>
  )
}

function TagCard({ tag, org, issueId }: { tag: TagDistribution; org: string; issueId: string }) {
  const total = Math.max(1, tag.total_values)
  return (
    <div className="rounded-lg border bg-card p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <Link
          to="/$org/issues/$issueId/tags/$tagKey"
          params={{ org, issueId, tagKey: tag.key }}
          className="group flex items-center gap-1 font-mono text-[13px] font-semibold hover:text-brand-text"
        >
          {tag.key}
          <ChevronRight className="size-3.5 opacity-0 transition-opacity group-hover:opacity-100" />
        </Link>
        <span className="text-xs text-muted-foreground tabular">{compact(tag.unique_values)} values</span>
      </div>
      <DistributionBar tag={tag} />
      <ul className="mt-2.5 space-y-1">
        {tag.top_values.slice(0, 5).map((v, i) => (
          <li key={v.value} className="flex items-center gap-2 text-[12.5px]">
            <span className={`size-2 shrink-0 rounded-sm ${BAR_COLORS[i]}`} />
            <Link
              to="/$org/issues"
              params={{ org }}
              search={{ query: `is:unresolved ${formatFilter(tag.key, v.value)}` }}
              className="min-w-0 flex-1 truncate hover:underline"
              title={v.value}
            >
              {v.name ?? v.value}
            </Link>
            <span className="text-muted-foreground tabular">{percent((v.count / total) * 100, 0)}</span>
          </li>
        ))}
      </ul>
    </div>
  )
}

export function TagsTab({ org, issueId }: { org: string; issueId: string }) {
  const q = useQuery({ queryKey: qk.issues.tags(issueId), queryFn: () => api.issues.tags(issueId) })
  return (
    <div className="px-4 py-4 sm:px-6">
      {q.isPending ? (
        <CardsSkeleton count={6} className="sm:grid-cols-2 xl:grid-cols-3" />
      ) : q.isError ? (
        <ErrorState error={q.error} retry={() => q.refetch()} />
      ) : q.data.length === 0 ? (
        <EmptyState icon={<Tags />} title="No tags" description="Events of this issue carry no tags." />
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {q.data.map((t) => (
            <TagCard key={t.key} tag={t} org={org} issueId={issueId} />
          ))}
        </div>
      )}
    </div>
  )
}

export function TagDetail({ org, issueId, tagKey }: { org: string; issueId: string; tagKey: string }) {
  const q = useQuery({ queryKey: qk.issues.tagValues(issueId, tagKey), queryFn: () => api.issues.tagValues(issueId, tagKey) })
  const summary = useQuery({ queryKey: qk.issues.tags(issueId), queryFn: () => api.issues.tags(issueId) })
  const totalValues = summary.data?.find((t) => t.key === tagKey)?.total_values ?? q.data?.data.reduce((s, v) => s + v.count, 0) ?? 0
  const total = Math.max(1, totalValues)
  return (
    <div className="space-y-3 px-4 py-4 sm:px-6">
      <div className="flex items-center gap-2">
        <Button asChild variant="ghost" size="xs">
          <Link to="/$org/issues/$issueId/tags" params={{ org, issueId }}>
            All tags
          </Link>
        </Button>
        <ChevronRight className="size-3.5 text-muted-foreground" />
        <h2 className="font-mono text-sm font-semibold">{tagKey}</h2>
        {q.data && <span className="text-xs text-muted-foreground">{thousands(totalValues)} events</span>}
      </div>
      <div className="overflow-hidden rounded-lg border bg-card">
        {q.isPending ? (
          <CardsSkeleton count={1} />
        ) : q.isError ? (
          <ErrorState error={q.error} retry={() => q.refetch()} />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Value</TableHead>
                <TableHead className="w-48">Share</TableHead>
                <TableHead className="text-right">Events</TableHead>
                <TableHead className="hidden md:table-cell">First seen</TableHead>
                <TableHead className="hidden md:table-cell">Last seen</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {q.data.data.map((v) => (
                <TableRow key={v.value}>
                  <TableCell className="max-w-0 min-w-48">
                    <Link
                      to="/$org/issues"
                      params={{ org }}
                      search={{ query: `is:unresolved ${formatFilter(tagKey, v.value)}` }}
                      className="block truncate font-mono text-[12.5px] hover:underline"
                    >
                      {v.value}
                    </Link>
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                        <div className="h-full bg-brand" style={{ width: `${(v.count / total) * 100}%` }} />
                      </div>
                      <span className="w-10 text-right text-xs text-muted-foreground tabular">{percent((v.count / total) * 100, 1)}</span>
                    </div>
                  </TableCell>
                  <TableCell className="text-right tabular">{thousands(v.count)}</TableCell>
                  <TableCell className="hidden text-muted-foreground md:table-cell">
                    {v.first_seen ? <RelativeTime date={v.first_seen} suffix /> : '—'}
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground md:table-cell">{v.last_seen ? <RelativeTime date={v.last_seen} suffix /> : '—'}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </div>
    </div>
  )
}
