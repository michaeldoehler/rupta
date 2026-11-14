import { createFileRoute } from '@tanstack/react-router'
import { MembersPage } from '@/features/settings/org-pages'

export const Route = createFileRoute('/_auth/$org/settings/members')({
  component: function Page() {
    const params = Route.useParams()
    return <MembersPage org={params.org} />
  },
})
