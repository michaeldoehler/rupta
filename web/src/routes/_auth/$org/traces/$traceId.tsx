import { createFileRoute } from '@tanstack/react-router'
import * as z from 'zod/mini'
import { TracePage } from '@/features/performance/trace-page'

const searchSchema = z.object({
  span: z.catch(z.optional(z.string()), undefined),
})

export const Route = createFileRoute('/_auth/$org/traces/$traceId')({
  validateSearch: searchSchema,
  head: () => ({ meta: [{ title: 'Trace · rupta' }] }),
  component: function Trace() {
    const { org, traceId } = Route.useParams()
    const { span } = Route.useSearch()
    return <TracePage key={traceId} org={org} traceId={traceId} initialSpan={span} />
  },
})
