import { createFileRoute } from '@tanstack/react-router'
import { FeedbackPage } from '@/features/feedback/feedback-page'

export const Route = createFileRoute('/_auth/$org/feedback')({
  head: () => ({ meta: [{ title: 'User feedback · rupta' }] }),
  component: FeedbackPage,
})
