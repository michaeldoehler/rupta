import { createFileRoute } from '@tanstack/react-router'
import { ReplayPage } from '@/features/replays/replays-page'

export const Route = createFileRoute('/_auth/$org/replays/$project/$replayId')({
  head: () => ({ meta: [{ title: 'Session replay · rupta' }] }),
  component: function Replay() {
    const { project, replayId } = Route.useParams()
    return <ReplayPage key={replayId} project={project} replayId={replayId} />
  },
})
