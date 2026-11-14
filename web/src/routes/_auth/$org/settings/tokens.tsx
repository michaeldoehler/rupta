import { createFileRoute } from '@tanstack/react-router'
import { OrgTokensPage } from '@/features/settings/org-more-pages'

export const Route = createFileRoute('/_auth/$org/settings/tokens')({
  component: function Page() {
    const params = Route.useParams()
    return <OrgTokensPage org={params.org} />
  },
})
