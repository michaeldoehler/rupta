import { createFileRoute } from '@tanstack/react-router'
import { RuleEditor } from '@/features/alerts/rule-editor'

export const Route = createFileRoute('/_auth/$org/alerts/new')({
  head: () => ({ meta: [{ title: 'New alert rule · rupta' }] }),
  component: function NewRule() {
    const { org } = Route.useParams()
    return <RuleEditor org={org} />
  },
})
