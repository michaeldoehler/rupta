import { createFileRoute } from '@tanstack/react-router'
import { SsoPage } from '@/features/settings/sso-page'

export const Route = createFileRoute('/_auth/$org/settings/sso')({
  component: function Page() {
    const params = Route.useParams()
    return <SsoPage org={params.org} />
  },
})
