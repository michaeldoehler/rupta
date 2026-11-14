import { createFileRoute, Outlet } from '@tanstack/react-router'
import { SettingsLayout } from '@/features/settings/settings-layout'

export const Route = createFileRoute('/_auth/$org/settings')({
  head: () => ({ meta: [{ title: 'Settings · rupta' }] }),
  component: function Settings() {
    const { org } = Route.useParams()
    return (
      <SettingsLayout org={org}>
        <Outlet />
      </SettingsLayout>
    )
  },
})
