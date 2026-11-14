import { createFileRoute } from '@tanstack/react-router'
import * as z from 'zod/mini'
import { AggregateProfilePage } from '@/features/profiles/profiles-page'

const searchSchema = z.object({
  transaction: z.catch(z.string(), ''),
  tx_project: z.catch(z.optional(z.string()), undefined),
})

export const Route = createFileRoute('/_auth/$org/profiles/aggregate')({
  validateSearch: searchSchema,
  head: () => ({ meta: [{ title: 'Aggregated flamegraph · rupta' }] }),
  component: function Aggregate() {
    const s = Route.useSearch()
    return <AggregateProfilePage key={`${s.transaction}-${s.tx_project}`} transaction={s.transaction} projectId={s.tx_project} />
  },
})
