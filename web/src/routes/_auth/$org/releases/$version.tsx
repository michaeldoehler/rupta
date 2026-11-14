import { createFileRoute } from '@tanstack/react-router'
import { ReleaseDetailPage } from '@/features/releases/release-detail-page'

export const Route = createFileRoute('/_auth/$org/releases/$version')({
  head: ({ params }) => ({ meta: [{ title: `${params.version} · Releases · rupta` }] }),
  component: function Release() {
    const { org, version } = Route.useParams()
    return <ReleaseDetailPage org={org} version={version} />
  },
})
