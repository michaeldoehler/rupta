import { createFileRoute } from '@tanstack/react-router'
import { ProjectTelemetryPage } from '@/features/settings/telemetry-settings'

export const Route = createFileRoute('/_auth/$org/settings/projects/$project/telemetry')({
  component: function Page() {
    const params = Route.useParams()
    return <ProjectTelemetryPage org={params.org} project={params.project} />
  },
})
