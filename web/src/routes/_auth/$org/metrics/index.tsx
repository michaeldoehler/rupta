import { createFileRoute, useNavigate } from '@tanstack/react-router'
import * as z from 'zod/mini'
import { MetricsPage } from '@/features/metrics/metrics-page'

const searchSchema = z.object({
  name: z.catch(z.optional(z.string()), undefined),
  op: z.catch(z.optional(z.enum(['sum', 'avg', 'min', 'max', 'count', 'count_unique', 'p50', 'p75', 'p95', 'p99'])), undefined),
  group_by: z.catch(z.optional(z.string()), undefined),
  chart: z.catch(z.optional(z.enum(['line', 'area', 'bar'])), undefined),
})

export const Route = createFileRoute('/_auth/$org/metrics/')({
  validateSearch: searchSchema,
  head: () => ({ meta: [{ title: 'Metrics · rupta' }] }),
  component: function Metrics() {
    const s = Route.useSearch()
    const navigate = useNavigate()
    return <MetricsPage state={s} onChange={(next) => void navigate({ to: '.', search: (prev: Record<string, unknown>) => ({ ...prev, ...next }), replace: true })} />
  },
})
