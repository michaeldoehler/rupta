import { createFileRoute, Outlet } from '@tanstack/react-router'
import { ProjectLayout } from '@/features/settings/project-pages'

export const Route = createFileRoute('/_auth/$org/settings/projects/$project')({
  component: function Project() {
    const { org, project } = Route.useParams()
    return (
      <ProjectLayout org={org} project={project}>
        <Outlet />
      </ProjectLayout>
    )
  },
})
