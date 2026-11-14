import { createFileRoute } from '@tanstack/react-router'
import { IssueReplays } from '@/features/replays/replays-page'

export const Route = createFileRoute('/_auth/$org/issues/$issueId/replays')({
  head: () => ({ meta: [{ title: 'Replays · Issue · rupta' }] }),
  component: function Replays() {
    const { org, issueId } = Route.useParams()
    return <IssueReplays org={org} issueId={issueId} />
  },
})
