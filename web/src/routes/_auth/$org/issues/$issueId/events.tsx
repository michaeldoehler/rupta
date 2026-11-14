import { createFileRoute } from '@tanstack/react-router'
import { EventsTab } from '@/features/issue-detail/events-tab'

export const Route = createFileRoute('/_auth/$org/issues/$issueId/events')({
  component: function Events() {
    const { org, issueId } = Route.useParams()
    return <EventsTab org={org} issueId={issueId} />
  },
})
