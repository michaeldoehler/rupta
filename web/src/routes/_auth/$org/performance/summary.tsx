import { createFileRoute } from '@tanstack/react-router'
import * as z from 'zod/mini'
import { TransactionSummaryPage } from '@/features/performance/transaction-summary-page'

const searchSchema = z.object({
  name: z.catch(z.string(), ''),
  tx_project: z.catch(z.optional(z.string()), undefined),
})

export const Route = createFileRoute('/_auth/$org/performance/summary')({
  validateSearch: searchSchema,
  head: () => ({ meta: [{ title: 'Transaction summary · rupta' }] }),
  component: function Summary() {
    const s = Route.useSearch()
    return <TransactionSummaryPage key={`${s.name}-${s.tx_project}`} name={s.name} projectId={s.tx_project} />
  },
})
