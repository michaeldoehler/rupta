import { createFileRoute } from '@tanstack/react-router'
import { MonitorPage } from '@/features/crons/monitor-page'

export const Route = createFileRoute('/_auth/$org/crons/$project/$slug')({
  head: ({ params }) => ({ meta: [{ title: `${params.slug} · Crons · rupta` }] }),
  component: function Monitor() {
    const { org, project, slug } = Route.useParams()
    return <MonitorPage org={org} project={project} slug={slug} />
  },
})
