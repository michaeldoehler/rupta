import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Monitor, Moon, Plus, Sun } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { ApiError, errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { qk, useApiMutation } from '@/api/queries'
import type { UserPreferences } from '@/api/types'
import { UserAvatar } from '@/components/common/avatar'
import { Field, FormError } from '@/components/common/field'
import { CardsSkeleton } from '@/components/common/states'
import { useTheme, type Theme } from '@/components/theme-provider'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { absolute, setTimePreferences } from '@/lib/format'
import { cn } from '@/lib/utils'
import { SettingsCard, SettingsPage, ToggleRow } from './settings-layout'
import { CreateTokenDialog, TokenTable } from './tokens'

function useUser() {
  return useQuery({ queryKey: qk.user, queryFn: api.user.get })
}

function timezones(): string[] {
  try {
    return (Intl as unknown as { supportedValuesOf: (k: string) => string[] }).supportedValuesOf('timeZone')
  } catch {
    return ['UTC', 'Europe/Berlin', 'Europe/London', 'America/New_York', 'America/Los_Angeles', 'Asia/Tokyo']
  }
}

export function ProfilePage() {
  const user = useUser()
  const qc = useQueryClient()
  const { theme, setTheme } = useTheme()
  const [name, setName] = useState('')
  useEffect(() => {
    if (user.data) setName(user.data.name)
  }, [user.data])
  const zones = useMemo(timezones, [])
  const update = useApiMutation((body: { name?: string; preferences?: UserPreferences }) => api.user.update(body), {
    invalidate: [qk.user, qk.session],
    success: 'Saved',
    onSuccess: (u) => {
      qc.setQueryData(qk.user, u)
      setTimePreferences(u.preferences.timezone, u.preferences.clock_24h)
    },
  })
  if (user.isPending)
    return (
      <SettingsPage title="Profile & preferences">
        <CardsSkeleton count={3} />
      </SettingsPage>
    )
  if (!user.data) return null
  const prefs = user.data.preferences
  const themes: { v: Theme; label: string; icon: React.ReactNode }[] = [
    { v: 'light', label: 'Light', icon: <Sun className="size-4" /> },
    { v: 'dark', label: 'Dark', icon: <Moon className="size-4" /> },
    { v: 'system', label: 'System', icon: <Monitor className="size-4" /> },
  ]
  return (
    <SettingsPage title="Profile & preferences" description="How you appear to others and how rupta looks for you.">
      <SettingsCard
        title="Profile"
        footer={
          <Button size="sm" onClick={() => update.mutate({ name })} disabled={update.isPending || name === user.data.name || !name.trim()}>
            Save profile
          </Button>
        }
      >
        <div className="flex flex-col gap-4 sm:flex-row sm:items-start">
          <UserAvatar name={user.data.name || user.data.email} src={user.data.avatar_url} className="size-14 text-base" />
          <div className="grid flex-1 gap-4 sm:grid-cols-2">
            <Field label="Name">{(p) => <Input {...p} value={name} onChange={(e) => setName(e.target.value)} autoComplete="name" />}</Field>
            <Field
              label="Email"
              hint={user.data.identities.length ? `Linked with ${user.data.identities.map((i) => i.provider).join(', ')}` : 'Contact an admin to change it.'}
            >
              {(p) => <Input {...p} value={user.data.email} disabled readOnly />}
            </Field>
          </div>
        </div>
      </SettingsCard>

      <SettingsCard title="Theme" description="Stored in this browser.">
        <RadioGroup value={theme} onValueChange={(v) => setTheme(v as Theme)} className="grid grid-cols-3 gap-3" aria-label="Theme">
          {themes.map((t) => (
            <label
              key={t.v}
              className={cn(
                'flex cursor-pointer flex-col items-center gap-2 rounded-lg border p-3 text-[13px] hover:bg-accent',
                theme === t.v && 'border-brand ring-1 ring-brand',
              )}
            >
              <ThemePreview mode={t.v} />
              <span className="flex items-center gap-2">
                <RadioGroupItem value={t.v} aria-label={t.label} /> {t.icon} {t.label}
              </span>
            </label>
          ))}
        </RadioGroup>
      </SettingsCard>

      <SettingsCard title="Time" description={`Saved in this browser. Times are shown like this: ${absolute(new Date(), { seconds: true })}`}>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Timezone">
            {(p) => (
              <Select value={prefs.timezone ?? 'browser'} onValueChange={(v) => update.mutate({ preferences: { ...prefs, timezone: v } })}>
                <SelectTrigger id={p.id} className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent className="max-h-72">
                  <SelectItem value="browser">Browser default ({Intl.DateTimeFormat().resolvedOptions().timeZone})</SelectItem>
                  {zones.map((z) => (
                    <SelectItem key={z} value={z}>
                      {z}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </Field>
          <div className="flex items-end pb-1.5">
            <label className="flex items-center gap-2 text-[13px]">
              <Switch checked={prefs.clock_24h ?? true} onCheckedChange={(v) => update.mutate({ preferences: { ...prefs, clock_24h: v } })} /> 24-hour clock
            </label>
          </div>
        </div>
      </SettingsCard>
    </SettingsPage>
  )
}

function ThemePreview({ mode }: { mode: Theme }) {
  const half = mode === 'system'
  const pane = (dark: boolean) => (
    <div className={cn('flex h-full flex-1 gap-1 p-1.5', dark ? 'bg-[oklch(0.17_0.004_60)]' : 'bg-[oklch(0.985_0.002_75)]')}>
      <div className={cn('w-3 rounded-sm', dark ? 'bg-white/10' : 'bg-black/8')} />
      <div className="flex flex-1 flex-col gap-1">
        <div className={cn('h-1.5 w-3/4 rounded-sm', dark ? 'bg-white/25' : 'bg-black/20')} />
        <div className={cn('h-1.5 w-1/2 rounded-sm', dark ? 'bg-white/15' : 'bg-black/10')} />
        <div className="h-1.5 w-1/3 rounded-sm bg-brand" />
      </div>
    </div>
  )
  return <div className="flex h-14 w-full overflow-hidden rounded-md border">{half ? [pane(false), pane(true)] : pane(mode === 'dark')}</div>
}

export function SecurityPage() {
  const user = useUser()
  const qc = useQueryClient()
  const [form, setForm] = useState({ current: '', next: '', confirm: '' })
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [create, setCreate] = useState(false)
  const tokens = useQuery({ queryKey: qk.userTokens, queryFn: api.user.tokens })
  const del = useApiMutation((id: string) => api.user.deleteToken(id), { invalidate: [qk.userTokens], success: 'Token revoked' })

  async function changePassword(e: React.FormEvent) {
    e.preventDefault()
    const errs: Record<string, string> = {}
    if (form.next.length < 10) errs.new_password = 'Use at least 10 characters.'
    if (form.next !== form.confirm) errs.confirm = 'The passwords do not match.'
    setErrors(errs)
    if (Object.keys(errs).length) return
    setBusy(true)
    setError(null)
    try {
      await api.user.changePassword({ current_password: form.current, new_password: form.next })
      toast.success('Password changed')
      setForm({ current: '', next: '', confirm: '' })
    } catch (err) {
      if (err instanceof ApiError) setErrors(err.fieldErrors)
      setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <SettingsPage title="Password & tokens">
      {user.data?.has_password !== false && (
        <SettingsCard title="Change password" description="Other sessions stay signed in until they expire.">
          <form onSubmit={changePassword} className="grid max-w-md gap-4" noValidate>
            <FormError message={error} />
            <Field label="Current password" error={errors.current_password}>
              {(p) => (
                <Input
                  {...p}
                  type="password"
                  autoComplete="current-password"
                  value={form.current}
                  onChange={(e) => setForm((f) => ({ ...f, current: e.target.value }))}
                />
              )}
            </Field>
            <Field label="New password" error={errors.new_password} hint="At least 10 characters.">
              {(p) => (
                <Input
                  {...p}
                  type="password"
                  autoComplete="new-password"
                  value={form.next}
                  onChange={(e) => setForm((f) => ({ ...f, next: e.target.value }))}
                />
              )}
            </Field>
            <Field label="Repeat new password" error={errors.confirm}>
              {(p) => (
                <Input
                  {...p}
                  type="password"
                  autoComplete="new-password"
                  value={form.confirm}
                  onChange={(e) => setForm((f) => ({ ...f, confirm: e.target.value }))}
                />
              )}
            </Field>
            <div>
              <Button type="submit" size="sm" disabled={busy || !form.current || !form.next}>
                Change password
              </Button>
            </div>
          </form>
        </SettingsCard>
      )}
      <SettingsCard
        title="Personal API tokens"
        description="Act as you, limited to the scopes you choose. For automation shared by a team, prefer organization tokens."
        footer={
          <Button size="sm" variant="outline" onClick={() => setCreate(true)}>
            <Plus /> Create token
          </Button>
        }
      >
        <div className="-m-4">
          <TokenTable tokens={tokens.data} loading={tokens.isPending} onDelete={(t) => del.mutate(t.id)} onCreate={() => setCreate(true)} />
        </div>
      </SettingsCard>
      <CreateTokenDialog
        open={create}
        onOpenChange={setCreate}
        defaultScopes={['project:read']}
        onCreate={async (body) => {
          const t = await api.user.createToken(body)
          void qc.invalidateQueries({ queryKey: qk.userTokens })
          return t
        }}
      />
    </SettingsPage>
  )
}

export function NotificationsPage() {
  const user = useUser()
  const update = useApiMutation((preferences: UserPreferences) => api.user.update({ preferences }), {
    invalidate: [qk.user],
    success: 'Notification settings saved',
  })
  if (user.isPending)
    return (
      <SettingsPage title="Notifications">
        <CardsSkeleton count={2} />
      </SettingsPage>
    )
  if (!user.data) return null
  const prefs = user.data.preferences
  const n = prefs.notifications ?? {}
  const setN = (patch: Partial<NonNullable<UserPreferences['notifications']>>) => update.mutate({ ...prefs, notifications: { ...n, ...patch } })
  return (
    <SettingsPage
      title="Notifications"
      description={`Emails go to ${user.data.email}. Channel notifications (Slack, Teams, webhooks) are configured in alert rules.`}
    >
      <SettingsCard>
        <div className="divide-y">
          <ToggleRow title="Issue alerts" description="Emails from alert rules that target you, your teams or all members.">
            <Switch checked={n.issue_alerts ?? true} onCheckedChange={(v) => setN({ issue_alerts: v })} aria-label="Issue alerts" />
          </ToggleRow>
          <ToggleRow title="Workflow" description="Assignments, comments, resolutions and regressions on issues.">
            <Select value={n.workflow ?? 'participating'} onValueChange={(v) => setN({ workflow: v as 'all' })}>
              <SelectTrigger size="sm" className="w-48" aria-label="Workflow notifications">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All issues</SelectItem>
                <SelectItem value="participating">Issues I participate in</SelectItem>
                <SelectItem value="none">Off</SelectItem>
              </SelectContent>
            </Select>
          </ToggleRow>
          <ToggleRow title="Deploys" description="When a release you contributed to is deployed.">
            <Switch checked={n.deploys ?? false} onCheckedChange={(v) => setN({ deploys: v })} aria-label="Deploy notifications" />
          </ToggleRow>
          <ToggleRow title="Weekly report" description="A summary of new, resolved and busiest issues every Monday.">
            <Switch checked={n.weekly_report ?? true} onCheckedChange={(v) => setN({ weekly_report: v })} aria-label="Weekly report" />
          </ToggleRow>
        </div>
      </SettingsCard>
      <p className="text-xs text-muted-foreground">
        These choices are saved in this browser only: the server does not store per-user notification preferences yet. Today, who gets emailed is decided by the
        recipients of each alert rule. Email delivery needs SMTP on the server (RUPTA_SMTP_URL).
      </p>
    </SettingsPage>
  )
}
