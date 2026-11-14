import { createFileRoute } from '@tanstack/react-router'
import { ProjectSourceMapsPage } from '@/features/settings/project-pages'

export const Route = createFileRoute('/_auth/$org/settings/projects/$project/source-maps')({
  component: function Page() {
    const params = Route.useParams()
    return <ProjectSourceMapsPage org={params.org} project={params.project} />
  },
})
