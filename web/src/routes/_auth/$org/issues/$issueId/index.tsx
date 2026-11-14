import { createFileRoute } from '@tanstack/react-router'
import { z } from 'zod'
import { eventQuery } from '@/api/queries'
import { DetailsTab } from '@/features/issue-detail/details-tab'

export const Route = createFileRoute('/_auth/$org/issues/$issueId/')({
  validateSearch: z.object({ event: z.string().optional().catch(undefined) }),
  loaderDeps: ({ search }) => ({ event: search.event ?? 'recommended' }),
  loader: ({ context, params, deps }) => {
    void context.queryClient.prefetchQuery(eventQuery(params.issueId, deps.event))
  },
  component: function Details() {
    const { org, issueId } = Route.useParams()
    const { event } = Route.useSearch()
    return <DetailsTab org={org} issueId={issueId} event={event ?? 'recommended'} />
  },
})
