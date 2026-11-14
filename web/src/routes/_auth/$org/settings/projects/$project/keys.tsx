import { createFileRoute } from '@tanstack/react-router'
import { ProjectKeysPage } from '@/features/settings/project-pages'

export const Route = createFileRoute('/_auth/$org/settings/projects/$project/keys')({
  component: function Page() {
    const params = Route.useParams()
    return <ProjectKeysPage org={params.org} project={params.project} />
  },
})
