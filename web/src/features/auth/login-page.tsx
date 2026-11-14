import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { KeyRound, Loader2 } from 'lucide-react'
import { useState } from 'react'
import { ApiError, errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { authConfigQuery, qk, sessionQuery } from '@/api/queries'
import { Field, FormError } from '@/components/common/field'
import { ErrorState } from '@/components/common/states'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Separator } from '@/components/ui/separator'
import { Skeleton } from '@/components/ui/skeleton'
import { AuthLayout } from './auth-layout'

export function LoginPage({ next, ssoError }: { next?: string; ssoError?: string }) {
  const config = useQuery(authConfigQuery)
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(ssoError ? `Single sign-on failed: ${ssoError}` : null)
  const [busy, setBusy] = useState(false)
  const safeNext = next && next.startsWith('/') && !next.startsWith('//') ? next : '/'

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await api.auth.login({ email, password })
      await qc.invalidateQueries({ queryKey: qk.session })
      await qc.fetchQuery(sessionQuery)
      await navigate({ to: safeNext })
    } catch (err) {
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
                    <KeyRound /> Continue with {p.name}
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
