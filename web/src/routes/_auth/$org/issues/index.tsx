import { createFileRoute } from '@tanstack/react-router'
import * as z from 'zod/mini'
import { DEFAULT_QUERY, IssueStreamPage } from '@/features/issues/issue-stream-page'

const searchSchema = z.object({
  query: z.catch(z.optional(z.string()), undefined),
  sort: z.catch(z.optional(z.enum(['last_seen', 'first_seen', 'freq', 'users', 'priority'])), undefined),
})

export const Route = createFileRoute('/_auth/$org/issues/')({
  validateSearch: searchSchema,
  head: () => ({ meta: [{ title: 'Issues · rupta' }] }),
  component: function Issues() {
    const s = Route.useSearch()
    return <IssueStreamPage query={s.query ?? DEFAULT_QUERY} sort={s.sort ?? 'last_seen'} />
  },
})
