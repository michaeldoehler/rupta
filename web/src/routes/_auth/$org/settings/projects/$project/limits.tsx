import { createFileRoute } from '@tanstack/react-router'
import { ProjectLimitsPage } from '@/features/settings/project-pages'

export const Route = createFileRoute('/_auth/$org/settings/projects/$project/limits')({
  component: function Page() {
    const params = Route.useParams()
    return <ProjectLimitsPage org={params.org} project={params.project} />
  },
})
