import { createFileRoute } from '@tanstack/react-router'
import { ProfileTransactionsPage } from '@/features/profiles/profiles-page'

export const Route = createFileRoute('/_auth/$org/profiles/transactions')({
  head: () => ({ meta: [{ title: 'Profiles by transaction · rupta' }] }),
  component: ProfileTransactionsPage,
})
