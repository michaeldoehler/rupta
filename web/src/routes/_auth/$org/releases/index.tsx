import { createFileRoute } from '@tanstack/react-router'
import { ReleasesPage } from '@/features/releases/releases-page'

export const Route = createFileRoute('/_auth/$org/releases/')({
  head: () => ({ meta: [{ title: 'Releases · rupta' }] }),
  component: ReleasesPage,
})
