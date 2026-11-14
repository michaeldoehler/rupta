import { createFileRoute, useNavigate } from '@tanstack/react-router'
import * as z from 'zod/mini'
import { LogsPage } from '@/features/logs/logs-page'

const searchSchema = z.object({
  query: z.catch(z.optional(z.string()), undefined),
  live: z.catch(z.optional(z.boolean()), undefined),
  start: z.catch(z.optional(z.string()), undefined),
  end: z.catch(z.optional(z.string()), undefined),
})

export const Route = createFileRoute('/_auth/$org/logs/')({
  validateSearch: searchSchema,
  head: () => ({ meta: [{ title: 'Logs · rupta' }] }),
  component: function Logs() {
    const s = Route.useSearch()
    const navigate = useNavigate()
    return (
      <LogsPage
        query={s.query ?? ''}
        live={Boolean(s.live)}
        start={s.start}
        end={s.end}
        onChange={(p) =>
          void navigate({
            to: '.',
            search: (prev) => ({ ...prev, ...p, query: 'query' in p ? p.query || undefined : prev.query, live: 'live' in p ? p.live || undefined : prev.live }),
            replace: true,
          })
        }
      />
    )
  },
})
