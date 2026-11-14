import { createFileRoute } from '@tanstack/react-router'
import { ProfilesPage } from '@/features/profiles/profiles-page'

export const Route = createFileRoute('/_auth/$org/profiles/')({
  head: () => ({ meta: [{ title: 'Profiles · rupta' }] }),
  component: ProfilesPage,
})
