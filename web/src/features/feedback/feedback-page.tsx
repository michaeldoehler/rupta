import { useQueries } from '@tanstack/react-query'
import { MessageSquareText, Search } from 'lucide-react'
import { useDeferredValue, useState } from 'react'
import { api } from '@/api/endpoints'
import { qk, useProjects } from '@/api/queries'
import type { Feedback } from '@/api/types'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { Input } from '@/components/ui/input'
import { FilterBar } from '@/features/shell/filter-bar'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { FeedbackItem } from './feedback-item'

export function FeedbackPage() {
  const { org, projects: selected } = useGlobalFilters()
  const [search, setSearch] = useState('')
  const query = useDeferredValue(search)
  const projects = useProjects(org)
  const targets = (projects.data ?? []).filter((p) => !selected.length || selected.includes(p.id))
  const results = useQueries({
    queries: targets.map((p) => ({
      queryKey: qk.feedback(org, p.slug, query),
      queryFn: () =>
        api.projects.feedback(org, p).then((pg) => ({
          ...pg,
          data: pg.data.filter((f) => !query || `${f.message} ${f.email ?? ''} ${f.name ?? ''}`.toLowerCase().includes(query.toLowerCase())),
        })),
    })),
  })
  const pending = projects.isPending || results.some((r) => r.isPending)
  const error = projects.error ?? results.find((r) => r.error)?.error
  const items: Feedback[] = results.flatMap((r) => r.data?.data ?? []).sort((a, b) => b.created_at.localeCompare(a.created_at))

  return (
    <div>
      <div className="space-y-3 border-b px-3 py-3 sm:px-5">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h1 className="text-lg font-semibold tracking-tight">User feedback</h1>
          <FilterBar showEnvironment={false} showPeriod={false} />
        </div>
        <div className="relative max-w-md">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search messages or emails"
            className="pl-8"
            aria-label="Search feedback"
          />
        </div>
      </div>
      {pending ? (
        <ListSkeleton rows={8} />
      ) : error ? (
        <ErrorState error={error} />
      ) : items.length === 0 ? (
        <EmptyState
          icon={<MessageSquareText />}
          title={query ? 'No feedback matches' : 'No user feedback yet'}
          description="Enable the feedback widget or the crash-report dialog of the SDK to let users tell you what happened."
        />
      ) : (
        <ul className="mx-auto max-w-4xl divide-y">
          {items.map((f) => (
            <FeedbackItem key={f.id} f={f} org={org} />
          ))}
        </ul>
      )}
    </div>
  )
}
