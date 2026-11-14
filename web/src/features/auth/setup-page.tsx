import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Check, Loader2 } from 'lucide-react'
import { useState } from 'react'
import { ApiError, errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { qk } from '@/api/queries'
import { Field, FormError } from '@/components/common/field'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { AuthLayout } from './auth-layout'

const STEPS = ['Your account', 'Organization'] as const

export function SetupPage() {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [step, setStep] = useState(0)
  const [form, setForm] = useState({ name: '', email: '', password: '', org_name: '' })
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const set = (k: keyof typeof form) => (e: React.ChangeEvent<HTMLInputElement>) => setForm((f) => ({ ...f, [k]: e.target.value }))

  function validateAccount() {
    const errs: Record<string, string> = {}
    if (!form.name.trim()) errs.name = 'Enter your name.'
    if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(form.email)) errs.email = 'Enter a valid email address.'
    if (form.password.length < 10) errs.password = 'Use at least 10 characters.'
    setFieldErrors(errs)
    return Object.keys(errs).length === 0
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    if (step === 0) {
      if (validateAccount()) setStep(1)
      return
    }
    if (!form.org_name.trim()) {
      setFieldErrors({ org_name: 'Enter a name for your organization.' })
      return
    }
    setBusy(true)
    setError(null)
    try {
      const session = await api.auth.setup(form)
      qc.setQueryData(qk.session, session)
      qc.setQueryData(qk.authConfig, (c: object | undefined) => (c ? { ...c, setup_required: false } : c))
      await navigate({ to: '/$org/issues', params: { org: session.orgs[0]?.slug ?? 'default' } })
    } catch (err) {
      if (err instanceof ApiError && Object.keys(err.fieldErrors).length) {
        setFieldErrors(err.fieldErrors)
        if (err.fieldErrors.email || err.fieldErrors.password || err.fieldErrors.name) setStep(0)
      } else setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <AuthLayout
      title="Set up rupta"
      subtitle="Create the first administrator account and your organization. You can invite your team afterwards."
      footer="This page is only available until the first account exists."
    >
      <ol className="mb-5 flex items-center gap-2 text-xs" aria-label="Setup steps">
        {STEPS.map((s, i) => (
          <li key={s} className="flex flex-1 items-center gap-2" aria-current={i === step ? 'step' : undefined}>
            <span
              className={cn(
                'flex size-5 shrink-0 items-center justify-center rounded-full border text-[11px] font-semibold',
                i < step && 'border-brand bg-brand text-brand-foreground',
                i === step && 'border-brand text-brand-text',
              )}
            >
              {i < step ? <Check className="size-3" /> : i + 1}
            </span>
            <span className={cn(i === step ? 'font-medium' : 'text-muted-foreground')}>{s}</span>
            {i < STEPS.length - 1 && <span className="h-px flex-1 bg-border" />}
          </li>
        ))}
      </ol>
      <form onSubmit={submit} className="grid gap-3.5" noValidate>
        <FormError message={error} />
        {step === 0 ? (
          <>
            <Field label="Name" error={fieldErrors.name}>
              {(p) => <Input {...p} autoComplete="name" autoFocus value={form.name} onChange={set('name')} />}
            </Field>
            <Field label="Email" error={fieldErrors.email}>
              {(p) => <Input {...p} type="email" autoComplete="email" value={form.email} onChange={set('email')} />}
            </Field>
            <Field label="Password" error={fieldErrors.password} hint="At least 10 characters.">
              {(p) => <Input {...p} type="password" autoComplete="new-password" value={form.password} onChange={set('password')} />}
            </Field>
            <Button type="submit" variant="brand" className="mt-1 h-9">
              Continue
            </Button>
          </>
        ) : (
          <>
            <Field label="Organization name" error={fieldErrors.org_name} hint="Usually your company or team. Projects and members belong to it.">
              {(p) => <Input {...p} autoFocus placeholder="Acme" value={form.org_name} onChange={set('org_name')} />}
            </Field>
            <div className="mt-1 flex gap-2">
              <Button variant="outline" className="h-9" onClick={() => setStep(0)}>
                Back
              </Button>
              <Button type="submit" variant="brand" className="h-9 flex-1" disabled={busy}>
                {busy && <Loader2 className="animate-spin" />} Create account
              </Button>
            </div>
          </>
        )}
      </form>
    </AuthLayout>
  )
}
