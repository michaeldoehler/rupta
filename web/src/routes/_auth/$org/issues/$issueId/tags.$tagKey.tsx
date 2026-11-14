import { createFileRoute } from '@tanstack/react-router'
import { TagDetail } from '@/features/issue-detail/tags-tab'

export const Route = createFileRoute('/_auth/$org/issues/$issueId/tags/$tagKey')({
  component: function Tag() {
    const { org, issueId, tagKey } = Route.useParams()
    return <TagDetail org={org} issueId={issueId} tagKey={tagKey} />
  },
})
