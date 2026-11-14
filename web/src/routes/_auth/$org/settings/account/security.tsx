import { createFileRoute } from '@tanstack/react-router'
import { SecurityPage } from '@/features/settings/account-pages'

export const Route = createFileRoute('/_auth/$org/settings/account/security')({
  component: SecurityPage,
})
