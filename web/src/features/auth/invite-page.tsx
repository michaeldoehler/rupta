import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Loader2, MailX } from 'lucide-react'
import { useState } from 'react'
import { ApiError, errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { qk } from '@/api/queries'
import { Field, FormError } from '@/components/common/field'
import { EmptyState } from '@/components/common/states'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { AuthLayout } from './auth-layout'

export function InvitePage({ token }: { token: string }) {
  const inv = useQuery({ queryKey: qk.invitation(token), queryFn: () => api.invitations.get(token), retry: false })
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [name, setName] = useState('')
  const [password, setPassword] = useState('')
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  if (inv.isPending)
    return (
      <AuthLayout title="Join organization">
        <div className="space-y-3">
          <Skeleton className="h-5 w-2/3" />
          <Skeleton className="h-9" />
          <Skeleton className="h-9" />
        </div>
      </AuthLayout>
    )
  if (inv.isError)
    return (
      <AuthLayout title="Invitation">
        <EmptyState compact icon={<MailX />} title="This invitation cannot be used" description={errorMessage(inv.error)} />
      </AuthLayout>
    )

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    if (password.length < 10) {
      setFieldErrors({ password: 'Use at least 10 characters.' })
      return
    }
    setBusy(true)
    setError(null)
    try {
      const session = await api.invitations.accept(token, { name, password })
      qc.setQueryData(qk.session, session)
      await navigate({ to: '/$org/issues', params: { org: inv.data!.org.slug } })
    } catch (err) {
      if (err instanceof ApiError && Object.keys(err.fieldErrors).length) setFieldErrors(err.fieldErrors)
      else setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  const d = inv.data
  return (
    <AuthLayout
      title={`Join ${d.org.name}`}
      subtitle={
        <>
          {d.invited_by ? `${d.invited_by} invited you` : 'You were invited'} as{' '}
          <Badge variant="outline" className="capitalize">
            {d.role}
          </Badge>
        </>
      }
    >
      <form onSubmit={submit} className="grid gap-3.5" noValidate>
        <FormError message={error} />
        <Field label="Email">{(p) => <Input {...p} value={d.email} disabled readOnly />}</Field>
        <Field label="Name" error={fieldErrors.name}>
          {(p) => <Input {...p} autoFocus autoComplete="name" value={name} onChange={(e) => setName(e.target.value)} />}
        </Field>
        <Field label="Choose a password" error={fieldErrors.password} hint="At least 10 characters.">
          {(p) => <Input {...p} type="password" autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} />}
        </Field>
        <Button type="submit" variant="brand" className="mt-1 h-9" disabled={busy}>
          {busy && <Loader2 className="animate-spin" />} Accept and join
        </Button>
      </form>
    </AuthLayout>
  )
}
