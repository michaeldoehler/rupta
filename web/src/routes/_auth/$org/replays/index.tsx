import { createFileRoute, useNavigate } from '@tanstack/react-router'
import * as z from 'zod/mini'
import { ReplaysPage } from '@/features/replays/replays-page'

const searchSchema = z.object({
  query: z.catch(z.optional(z.string()), undefined),
  sort: z.catch(z.optional(z.enum(['-started', '-duration', '-count_errors', '-count_clicks'])), undefined),
})

export const Route = createFileRoute('/_auth/$org/replays/')({
  validateSearch: searchSchema,
  head: () => ({ meta: [{ title: 'Session replays · rupta' }] }),
  component: function Replays() {
    const s = Route.useSearch()
    const navigate = useNavigate()
    return <ReplaysPage query={s.query ?? ''} sort={s.sort ?? '-started'} onChange={(p) => void navigate({ to: '.', search: (prev: Record<string, unknown>) => ({ ...prev, ...p }) as typeof s, replace: true })} />
  },
})
