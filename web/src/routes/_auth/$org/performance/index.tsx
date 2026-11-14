import { createFileRoute, useNavigate } from '@tanstack/react-router'
import * as z from 'zod/mini'
import { TransactionsPage } from '@/features/performance/transactions-page'

const searchSchema = z.object({
  sort: z.catch(z.optional(z.enum(['tpm', 'p50', 'p95', 'p99', 'failure_rate', 'count'])), undefined),
})

export const Route = createFileRoute('/_auth/$org/performance/')({
  validateSearch: searchSchema,
  head: () => ({ meta: [{ title: 'Performance · rupta' }] }),
  component: function Performance() {
    const s = Route.useSearch()
    const navigate = useNavigate()
    return (
      <TransactionsPage
        sort={s.sort ?? 'tpm'}
        onSort={(sort) => void navigate({ to: '.', search: (prev: Record<string, unknown>) => ({ ...prev, sort: sort === 'tpm' ? undefined : sort }), replace: true })}
      />
    )
  },
})
