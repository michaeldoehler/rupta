import { createFileRoute } from '@tanstack/react-router'
import { ChannelsPage } from '@/features/settings/org-more-pages'

export const Route = createFileRoute('/_auth/$org/settings/channels')({
  component: function Page() {
    const params = Route.useParams()
    return <ChannelsPage org={params.org} />
  },
})
