import { createFileRoute } from '@tanstack/react-router'
import { RuleEditor } from '@/features/alerts/rule-editor'

export const Route = createFileRoute('/_auth/$org/alerts/$project/$ruleId')({
  head: () => ({ meta: [{ title: 'Alert rule · rupta' }] }),
  component: function EditRule() {
    const { org, project, ruleId } = Route.useParams()
    return <RuleEditor org={org} project={project} ruleId={ruleId} />
  },
})
