import { createFileRoute } from '@tanstack/react-router'
import { ProjectFiltersPage } from '@/features/settings/project-pages'

export const Route = createFileRoute('/_auth/$org/settings/projects/$project/filters')({
  component: function Page() {
    const params = Route.useParams()
    return <ProjectFiltersPage org={params.org} project={params.project} />
  },
})
