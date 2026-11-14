import { createFileRoute } from '@tanstack/react-router'
import { ProfilePage } from '@/features/profiles/profiles-page'

export const Route = createFileRoute('/_auth/$org/profiles/$project/$profileId')({
  head: () => ({ meta: [{ title: 'Profile · rupta' }] }),
  component: function Profile() {
    const { project, profileId } = Route.useParams()
    return <ProfilePage key={profileId} project={project} profileId={profileId} />
  },
})
