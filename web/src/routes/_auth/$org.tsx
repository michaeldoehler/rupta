import { createFileRoute, notFound, Outlet, retainSearchParams } from '@tanstack/react-router'
import { projectsQuery } from '@/api/queries'
import { AppShell } from '@/features/shell/app-shell'
import { globalSearchSchema } from '@/lib/search-params'

export const Route = createFileRoute('/_auth/$org')({
  validateSearch: globalSearchSchema,
  search: { middlewares: [retainSearchParams(['project', 'environment', 'period'])] },
  beforeLoad: ({ context, params }) => {
    const membership = context.session.orgs.find((o) => o.slug === params.org)
    if (!membership) throw notFound()
    return { membership }
  },
  loader: ({ context, params }) => {
    // Warm the project list; most pages need it.
    void context.queryClient.prefetchQuery(projectsQuery(params.org))
  },
  component: function OrgLayout() {
    const { org } = Route.useParams()
    return (
      <AppShell org={org}>
        <Outlet />
      </AppShell>
    )
  },
})
