import { createFileRoute } from '@tanstack/react-router'
import { DashboardPage } from '@/features/metrics/dashboards'

export const Route = createFileRoute('/_auth/$org/dashboards/$dashboardId')({
  head: () => ({ meta: [{ title: 'Dashboard · rupta' }] }),
  component: function Dashboard() {
    const { dashboardId } = Route.useParams()
    return <DashboardPage key={dashboardId} dashboardId={dashboardId} />
  },
})
