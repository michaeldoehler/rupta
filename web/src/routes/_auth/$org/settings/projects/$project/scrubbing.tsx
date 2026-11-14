import { createFileRoute } from '@tanstack/react-router'
import { ProjectScrubbingPage } from '@/features/settings/project-pages'

export const Route = createFileRoute('/_auth/$org/settings/projects/$project/scrubbing')({
  component: function Page() {
    const params = Route.useParams()
    return <ProjectScrubbingPage org={params.org} project={params.project} />
  },
})
