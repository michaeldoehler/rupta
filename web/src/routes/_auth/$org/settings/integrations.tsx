import { createFileRoute } from '@tanstack/react-router'
import { IntegrationsPage } from '@/features/integrations/integrations-page'

export const Route = createFileRoute('/_auth/$org/settings/integrations')({
  head: () => ({ meta: [{ title: 'Integrations · rupta' }] }),
  component: function Page() {
    const { org } = Route.useParams()
    return <IntegrationsPage org={org} />
  },
})
