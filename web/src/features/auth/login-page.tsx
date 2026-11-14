import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Loader2 } from 'lucide-react'
import { useState } from 'react'
import { ApiError, errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { orgAuthConfigQuery, qk, sessionQuery } from '@/api/queries'
import { SsoProviderIcon } from '@/components/brand/sso-icon'
import { Field, FormError } from '@/components/common/field'
import { ErrorState } from '@/components/common/states'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Separator } from '@/components/ui/separator'
import { Skeleton } from '@/components/ui/skeleton'
import { AuthLayout } from './auth-layout'

const SSO_ERRORS: Record<string, string> = {
  sso_failed: 'Single sign-on failed. Try again or contact your administrator.',
  sso_denied: 'The identity provider did not allow the sign-in.',
  not_invited: 'Your account is not a member of this organization yet. Ask an administrator for an invitation.',
  disabled: 'Your account is disabled.',
}

export function LoginPage({ next, ssoError, org }: { next?: string; ssoError?: string; org?: string }) {
  const config = useQuery(orgAuthConfigQuery(org ?? ''))
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(ssoError ? (SSO_ERRORS[ssoError] ?? `Single sign-on failed (${ssoError}).`) : null)
  const [busy, setBusy] = useState(false)
  const safeNext = next && next.startsWith('/') && !next.startsWith('//') ? next : '/'

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      const session = await api.auth.login({ email, password })
      if (session?.user) qc.setQueryData(qk.session, session)
      else await qc.fetchQuery({ ...sessionQuery, staleTime: 0 })
      await navigate({ to: safeNext })
    } catch (err) {
      if (err instanceof ApiError && err.code === 'sso_required' && typeof err.details.provider === 'string') {
        // The organization requires single sign-on: continue there.
        window.location.assign(api.auth.ssoStartUrl(err.details.provider, safeNext))
        return
      }
      setError(err instanceof ApiError && err.status === 401 ? 'Email or password is incorrect.' : errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <AuthLayout title="Sign in" subtitle="Welcome back. Sign in to see what broke." footer="Self-hosted error tracking">
      {config.isPending ? (
        <div className="space-y-3">
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-9 w-full" />
        </div>
      ) : config.isError ? (
        <ErrorState error={config.error} retry={() => config.refetch()} />
      ) : (
        <div className="grid gap-4">
          {config.data.sso.length > 0 && (
            <div className="grid gap-2">
              {config.data.sso.map((p) => (
                <Button key={p.id} variant="outline" className="h-9 w-full" asChild>
                  <a href={api.auth.ssoStartUrl(p.id, safeNext)}>
                    <SsoProviderIcon name={p.name} size="sm" decorative /> Continue with {p.name}
                  </a>
                </Button>
              ))}
            </div>
          )}
          {config.data.sso.length > 0 && config.data.local_login && (
            <div className="flex items-center gap-3 text-xs text-muted-foreground">
              <Separator className="flex-1" /> or <Separator className="flex-1" />
            </div>
          )}
          <FormError message={error} />
          {config.data.local_login ? (
            <form onSubmit={submit} className="grid gap-3.5" noValidate>
              <Field label="Email">
                {(p) => <Input {...p} type="email" autoComplete="username" autoFocus required value={email} onChange={(e) => setEmail(e.target.value)} />}
              </Field>
              <Field label="Password">
                {(p) => (
                  <Input {...p} type="password" autoComplete="current-password" required value={password} onChange={(e) => setPassword(e.target.value)} />
                )}
              </Field>
              <Button type="submit" variant="brand" className="mt-1 h-9 w-full" disabled={busy || !email || !password}>
                {busy && <Loader2 className="animate-spin" />} Sign in
              </Button>
            </form>
          ) : (
            <p className="text-center text-[13px] text-muted-foreground">Password sign-in is disabled on this instance.</p>
          )}
        </div>
      )}
    </AuthLayout>
  )
}
