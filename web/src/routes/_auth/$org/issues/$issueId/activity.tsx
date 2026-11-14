import { createFileRoute } from '@tanstack/react-router'
import { ActivityTab } from '@/features/issue-detail/activity-tab'

export const Route = createFileRoute('/_auth/$org/issues/$issueId/activity')({
  component: function Activity() {
    const { issueId } = Route.useParams()
    return <ActivityTab issueId={issueId} />
  },
})
