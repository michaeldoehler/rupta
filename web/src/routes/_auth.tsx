import { createFileRoute, Outlet, redirect } from '@tanstack/react-router'
import { setCsrfToken } from '@/api/client'
import { sessionQuery } from '@/api/queries'

// Pathless layout: everything below needs a signed-in user.
export const Route = createFileRoute('/_auth')({
  beforeLoad: async ({ context, location }) => {
    const session = await context.queryClient.ensureQueryData(sessionQuery)
    if (!session) throw redirect({ to: '/login', search: { next: location.href } })
    setCsrfToken(session.csrf_token)
    return { session }
  },
  component: Outlet,
})
