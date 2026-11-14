import { createFileRoute, redirect } from '@tanstack/react-router'

// Notifications and webhooks link to /organizations/{org}/issues/{id}.
export const Route = createFileRoute('/organizations/$org/issues/$issueId')({
  beforeLoad: ({ params }) => {
    throw redirect({ to: '/$org/issues/$issueId', params })
  },
})
