import { createFileRoute } from '@tanstack/react-router'
import { ProjectsPage } from '@/features/settings/project-pages'

export const Route = createFileRoute('/_auth/$org/settings/projects/')({
  component: function Page() {
    const params = Route.useParams()
    return <ProjectsPage org={params.org} />
  },
})
