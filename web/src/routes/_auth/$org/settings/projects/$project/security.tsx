import { createFileRoute } from '@tanstack/react-router'
import { ProjectSecurityPage } from '@/features/settings/project-pages'

export const Route = createFileRoute('/_auth/$org/settings/projects/$project/security')({
  component: function Page() {
    const params = Route.useParams()
    return <ProjectSecurityPage org={params.org} project={params.project} />
  },
})
