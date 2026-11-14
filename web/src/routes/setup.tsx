import { createFileRoute, redirect } from '@tanstack/react-router'
import { authConfigQuery } from '@/api/queries'
import { SetupPage } from '@/features/auth/setup-page'

export const Route = createFileRoute('/setup')({
  beforeLoad: async ({ context }) => {
    const config = await context.queryClient.fetchQuery(authConfigQuery)
    if (!config.setup_required) throw redirect({ to: '/login' })
  },
  head: () => ({ meta: [{ title: 'Set up · rupta' }] }),
  component: SetupPage,
})
