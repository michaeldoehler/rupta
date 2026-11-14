import { createFileRoute } from '@tanstack/react-router'
import { TagsTab } from '@/features/issue-detail/tags-tab'

export const Route = createFileRoute('/_auth/$org/issues/$issueId/tags/')({
  component: function Tags() {
    const { org, issueId } = Route.useParams()
    return <TagsTab org={org} issueId={issueId} />
  },
})
