import { useQueryClient } from '@tanstack/react-query'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useState } from 'react'
import { errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { qk } from '@/api/queries'
import { Field, FormError } from '@/components/common/field'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { AuthLayout } from '@/features/auth/auth-layout'

export const Route = createFileRoute('/_auth/onboarding')({
  head: () => ({ meta: [{ title: 'New organization · rupta' }] }),
  component: Onboarding,
})

function Onboarding() {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [name, setName] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  return (
    <AuthLayout title="Create an organization" subtitle="Organizations hold projects, members and settings.">
      <form
        className="grid gap-4"
        onSubmit={async (e) => {
          e.preventDefault()
          if (!name.trim()) return setError('Enter a name.')
          setBusy(true)
          try {
            const org = await api.orgs.create({ name: name.trim() })
            await qc.invalidateQueries({ queryKey: qk.session })
            await navigate({ to: '/$org/settings/projects', params: { org: org.slug } })
          } catch (err) {
            setError(errorMessage(err))
          } finally {
            setBusy(false)
          }
        }}
      >
        <FormError message={error} />
        <Field label="Name">{(p) => <Input {...p} autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Acme" />}</Field>
        <Button type="submit" variant="brand" disabled={busy}>
          Create organization
        </Button>
      </form>
    </AuthLayout>
  )
}
