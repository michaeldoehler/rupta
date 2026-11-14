import { createFileRoute } from '@tanstack/react-router'
import { OrgGeneralPage } from '@/features/settings/org-pages'

export const Route = createFileRoute('/_auth/$org/settings/')({
  component: function Page() {
    const params = Route.useParams()
    return <OrgGeneralPage org={params.org} />
  },
})
