import { createFileRoute } from '@tanstack/react-router'
import { ProjectIntegrationsPage } from '@/features/integrations/project-integrations'

export const Route = createFileRoute('/_auth/$org/settings/projects/$project/integrations')({
  component: function Page() {
    const params = Route.useParams()
    return <ProjectIntegrationsPage org={params.org} project={params.project} />
  },
})
