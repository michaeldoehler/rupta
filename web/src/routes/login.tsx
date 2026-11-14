import { createFileRoute, redirect } from '@tanstack/react-router'
import * as z from 'zod/mini'
import { authConfigQuery, sessionQuery } from '@/api/queries'
import { LoginPage } from '@/features/auth/login-page'

export const Route = createFileRoute('/login')({
  validateSearch: z.object({
    next: z.catch(z.optional(z.string()), undefined),
    error: z.catch(z.optional(z.string()), undefined),
    org: z.catch(z.optional(z.string()), undefined),
  }),
  beforeLoad: async ({ context }) => {
    const [session, config] = await Promise.all([
      context.queryClient.ensureQueryData(sessionQuery),
      context.queryClient.ensureQueryData(authConfigQuery).catch(() => null),
    ])
    if (session) throw redirect({ to: '/' })
    if (config?.setup_required) throw redirect({ to: '/setup' })
  },
  head: () => ({ meta: [{ title: 'Sign in · rupta' }] }),
  component: function Login() {
    const { next, error, org } = Route.useSearch()
    return <LoginPage next={next} ssoError={error} org={org} />
  },
})
