import { createFileRoute } from '@tanstack/react-router'
import { TeamsPage } from '@/features/settings/org-pages'

export const Route = createFileRoute('/_auth/$org/settings/teams')({
  component: function Page() {
    const params = Route.useParams()
    return <TeamsPage org={params.org} />
  },
})
