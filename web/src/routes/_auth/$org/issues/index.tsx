import { createFileRoute } from '@tanstack/react-router'
import { z } from 'zod'
import { DEFAULT_QUERY, IssueStreamPage } from '@/features/issues/issue-stream-page'

const searchSchema = z.object({
  query: z.string().optional().catch(undefined),
  sort: z.enum(['last_seen', 'first_seen', 'freq', 'users', 'priority']).optional().catch(undefined),
  stats: z.enum(['24h', '14d']).optional().catch(undefined),
})

export const Route = createFileRoute('/_auth/$org/issues/')({
  validateSearch: searchSchema,
  head: () => ({ meta: [{ title: 'Issues · rupta' }] }),
  component: function Issues() {
    const s = Route.useSearch()
    return <IssueStreamPage query={s.query ?? DEFAULT_QUERY} sort={s.sort ?? 'last_seen'} stats={s.stats ?? '24h'} />
  },
})
