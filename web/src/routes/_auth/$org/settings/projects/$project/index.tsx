import { createFileRoute } from '@tanstack/react-router'
import { ProjectGeneralPage } from '@/features/settings/project-pages'

export const Route = createFileRoute('/_auth/$org/settings/projects/$project/')({
  component: function Page() {
    const params = Route.useParams()
    return <ProjectGeneralPage org={params.org} project={params.project} />
  },
})
