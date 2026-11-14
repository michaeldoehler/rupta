import { createFileRoute } from '@tanstack/react-router'
import { ProfilePage } from '@/features/settings/account-pages'

export const Route = createFileRoute('/_auth/$org/settings/account/')({
  component: ProfilePage,
})
