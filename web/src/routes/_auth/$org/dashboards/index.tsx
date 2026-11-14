import { createFileRoute } from '@tanstack/react-router'
import { DashboardsPage } from '@/features/metrics/dashboards'

export const Route = createFileRoute('/_auth/$org/dashboards/')({
  head: () => ({ meta: [{ title: 'Dashboards · rupta' }] }),
  component: DashboardsPage,
})
