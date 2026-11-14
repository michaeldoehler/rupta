import { createFileRoute } from '@tanstack/react-router'
import { CronsPage } from '@/features/crons/crons-page'

export const Route = createFileRoute('/_auth/$org/crons/')({
  head: () => ({ meta: [{ title: 'Crons · rupta' }] }),
  component: CronsPage,
})
