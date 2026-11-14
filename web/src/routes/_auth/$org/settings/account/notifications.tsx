import { createFileRoute } from '@tanstack/react-router'
import { NotificationsPage } from '@/features/settings/account-pages'

export const Route = createFileRoute('/_auth/$org/settings/account/notifications')({
  component: NotificationsPage,
})
