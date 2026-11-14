import { createFileRoute, redirect } from '@tanstack/react-router'
import { authConfigQuery, sessionQuery } from '@/api/queries'

export const Route = createFileRoute('/')({
  beforeLoad: async ({ context }) => {
    const session = await context.queryClient.ensureQueryData(sessionQuery)
    if (session?.orgs.length) throw redirect({ to: '/$org/issues', params: { org: lastOrg(session.orgs.map((o) => o.slug)) } })
    if (session) throw redirect({ to: '/onboarding' })
    const config = await context.queryClient.ensureQueryData(authConfigQuery)
    throw redirect({ to: config.setup_required ? '/setup' : '/login' })
  },
})

function lastOrg(slugs: string[]): string {
  try {
    const last = localStorage.getItem('rupta-last-org')
    if (last && slugs.includes(last)) return last
  } catch {
    /* ignore */
  }
  return slugs[0]
}
