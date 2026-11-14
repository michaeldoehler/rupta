import { useQuery } from '@tanstack/react-query'
import { MessageSquareText } from 'lucide-react'
import { api } from '@/api/endpoints'
import { qk, useProjects } from '@/api/queries'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { FeedbackItem } from '@/features/feedback/feedback-item'

export function FeedbackTab({ org, issueId }: { org: string; issueId: string }) {
  const projects = useProjects(org)
  const q = useQuery({ queryKey: qk.issues.feedback(issueId), queryFn: () => api.issues.feedback(issueId, projects.data ?? []), enabled: projects.isSuccess })
  return (
    <div className="mx-auto max-w-3xl px-4 py-5 sm:px-6">
      {q.isPending ? (
        <ListSkeleton rows={3} />
      ) : q.isError ? (
        <ErrorState error={q.error} retry={() => q.refetch()} />
      ) : q.data.length === 0 ? (
        <EmptyState
          icon={<MessageSquareText />}
          title="No user feedback for this issue"
          description="When an SDK's feedback widget or crash-report dialog is enabled, what users write about this error shows up here."
        />
      ) : (
        <ul className="divide-y rounded-lg border bg-card">
          {q.data.map((f) => (
            <FeedbackItem key={f.id} f={f} org={org} showIssue={false} />
          ))}
        </ul>
      )}
    </div>
  )
}
