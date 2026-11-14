import { createFileRoute, useNavigate } from '@tanstack/react-router'
import * as z from 'zod/mini'
import { SpansPage } from '@/features/performance/spans-page'

const searchSchema = z.object({
  group_by: z.catch(z.optional(z.enum(['op', 'description'])), undefined),
  q: z.catch(z.optional(z.string()), undefined),
})

export const Route = createFileRoute('/_auth/$org/traces/')({
  validateSearch: searchSchema,
  head: () => ({ meta: [{ title: 'Traces · rupta' }] }),
  component: function Traces() {
    const s = Route.useSearch()
    const navigate = useNavigate()
    return (
      <SpansPage
        key={s.q ?? ''}
        groupBy={s.group_by ?? 'op'}
        query={s.q ?? ''}
        onChange={(p) => void navigate({ to: '.', search: (prev: Record<string, unknown>) => ({ ...prev, ...p }), replace: true })}
      />
    )
  },
})
