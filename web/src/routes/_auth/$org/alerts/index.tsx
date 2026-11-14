import { createFileRoute } from '@tanstack/react-router'
import { AlertsPage } from '@/features/alerts/alerts-page'

export const Route = createFileRoute('/_auth/$org/alerts/')({
  head: () => ({ meta: [{ title: 'Alerts · rupta' }] }),
  component: AlertsPage,
})
