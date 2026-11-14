import { createFileRoute } from '@tanstack/react-router'
import { FeedbackTab } from '@/features/issue-detail/feedback-tab'

export const Route = createFileRoute('/_auth/$org/issues/$issueId/feedback')({
  component: function IssueFeedback() {
    const { org, issueId } = Route.useParams()
    return <FeedbackTab org={org} issueId={issueId} />
  },
})
