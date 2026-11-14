import { createFileRoute } from '@tanstack/react-router'
import { issueQuery } from '@/api/queries'
import { IssueLayout } from '@/features/issue-detail/issue-layout'

export const Route = createFileRoute('/_auth/$org/issues/$issueId')({
  loader: ({ context, params }) => {
    void context.queryClient.prefetchQuery(issueQuery(params.issueId))
  },
  head: () => ({ meta: [{ title: 'Issue · rupta' }] }),
  component: function Issue() {
    const { org, issueId } = Route.useParams()
    return <IssueLayout org={org} issueId={issueId} />
  },
})
