import { createFileRoute } from '@tanstack/react-router'
import { AuditLogPage } from '@/features/settings/org-more-pages'

export const Route = createFileRoute('/_auth/$org/settings/audit-log')({
  component: function Page() {
    const params = Route.useParams()
    return <AuditLogPage org={params.org} />
  },
})
