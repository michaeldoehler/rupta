import { createFileRoute } from '@tanstack/react-router'
import { InvitePage } from '@/features/auth/invite-page'

export const Route = createFileRoute('/invite/$token')({
  head: () => ({ meta: [{ title: 'Invitation · rupta' }] }),
  component: function Invite() {
    const { token } = Route.useParams()
    return <InvitePage token={token} />
  },
})
