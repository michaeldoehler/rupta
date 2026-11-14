import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { BookOpen, CircleAlert, CircleCheck, KeyRound, Pencil, Plug, Plus, RefreshCw, Send, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { ApiError, errorMessage } from '@/api/client'
import {
  ik,
  integrationsApi,
  type Integration,
  type IntegrationConfig,
  type IntegrationKind,
  type IntegrationWithSecret,
  type TestResult,
} from '@/api/integrations'
import { CopyField } from '@/components/common/copy-button'
import { Field, FormError } from '@/components/common/field'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Tip } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { useRole } from '@/features/settings/org-pages'
import { SettingsCard, SettingsPage } from '@/features/settings/settings-layout'
import { BrandPicker } from '@/components/brand/brand-picker'
import { KINDS, kindMeta, TrackerIcon } from './kinds'

/** The webhook URL and (once) the secret, with the tracker's setup steps. */
function WebhookDialog({ integration, open, onOpenChange }: { integration: IntegrationWithSecret; open: boolean; onOpenChange: (o: boolean) => void }) {
  const m = kindMeta(integration.kind)
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader className="flex-row items-center gap-3">
          <TrackerIcon kind={integration.kind} size="xl" />
          <div className="grid gap-1.5 text-left">
            <DialogTitle>Set up the webhook in {m.label}</DialogTitle>
            <DialogDescription>With a webhook, status changes in {m.label} reach rupta right away.</DialogDescription>
          </div>
        </DialogHeader>
        <div className="space-y-3">
          <Field label="Webhook URL">{() => <CopyField value={integration.webhook_url} label="Copy webhook URL" />}</Field>
          {integration.webhook_secret ? (
            <Field label="Webhook secret" hint="Shown only now. Copy it into the tracker's webhook form.">
              {() => <CopyField value={integration.webhook_secret!} label="Copy webhook secret" />}
            </Field>
          ) : m.trackerGeneratesSecret ? (
            <p className="rounded-md border border-warning/40 bg-warning-soft px-3 py-2 text-[13px]">
              {m.label} creates the signing secret. Save it in this integration (Edit → Webhook signing secret); until then rupta refuses its webhooks.
            </p>
          ) : null}
          <p className="text-[13px] text-muted-foreground">{m.webhookSteps}</p>
          <a href={m.docs} target="_blank" rel="noreferrer noopener" className="inline-flex items-center gap-1.5 text-[13px] text-palm-text hover:text-brand">
            <BookOpen className="size-3.5" /> Setup guide for {m.label}
          </a>
        </div>
        <DialogFooter>
          <Button onClick={() => onOpenChange(false)}>Done</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function IntegrationDialog({
  org,
  integration,
  open,
  onOpenChange,
  onCreated,
}: {
  org: string
  integration: Integration | null
  open: boolean
  onOpenChange: (o: boolean) => void
  onCreated: (i: IntegrationWithSecret) => void
}) {
  const qc = useQueryClient()
  const [kind, setKind] = useState<IntegrationKind>(integration?.kind ?? 'jira_cloud')
  const [name, setName] = useState(integration?.name ?? KINDS[0].label)
  const [config, setConfig] = useState<IntegrationConfig>(integration?.config ?? {})
  const [token, setToken] = useState('')
  const [secret, setSecret] = useState('')
  const [validate, setValidate] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})
  const [busy, setBusy] = useState(false)
  const m = kindMeta(kind)
  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    setFieldErrors({})
    if (!name.trim()) return setFieldErrors({ name: 'Enter a name.' })
    const missing = m.fields.find((f) => f.required && !config[f.key]?.trim())
    if (missing) return setFieldErrors({ [missing.key]: `Enter the ${missing.label.toLowerCase()}.` })
    if (!integration && !token.trim()) return setFieldErrors({ token: `Enter the ${m.tokenLabel.toLowerCase()}.` })
    setBusy(true)
    try {
      const cleaned = Object.fromEntries(m.fields.map((f) => [f.key, config[f.key]?.trim() || undefined]).filter(([, v]) => v))
      const body = {
        name: name.trim(),
        config: cleaned,
        validate,
        ...(token.trim() ? { token: token.trim() } : {}),
        ...(secret.trim() ? { webhook_secret: secret.trim() } : {}),
      }
      if (integration) {
        await integrationsApi.update(org, integration.id, body)
        toast.success('Integration saved')
      } else {
        const created = await integrationsApi.create(org, { ...body, kind })
        onCreated(created)
        toast.success(`${m.label} connected${created.account ? ` as ${created.account.name}` : ''}`)
      }
      await qc.invalidateQueries({ queryKey: ik.list(org) })
      onOpenChange(false)
    } catch (err) {
      // A field error shows under its field; anything else above the form.
      const shown = new Set(['name', 'token', 'webhook_secret', ...m.fields.flatMap((f) => [f.key, `config.${f.key}`])])
      const fe = err instanceof ApiError ? err.fieldErrors : {}
      setFieldErrors(fe)
      if (!Object.keys(fe).some((k) => shown.has(k))) setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <form onSubmit={submit} className="grid gap-4" noValidate>
          <DialogHeader className="flex-row items-center gap-3">
            <TrackerIcon kind={kind} size="xl" />
            <div className="grid gap-1.5 text-left">
              <DialogTitle>{integration ? `Edit ${integration.name}` : 'Connect an issue tracker'}</DialogTitle>
              <DialogDescription>{integration ? `${m.label}: ` : ''}Create and link tickets from issues, and keep statuses in sync.</DialogDescription>
            </div>
          </DialogHeader>
          <FormError message={error} />
          {!integration && (
            <BrandPicker
              label="Tracker"
              value={kind}
              onChange={(k) => {
                setKind(k)
                setConfig({})
                if (!name || KINDS.some((x) => x.label === name)) setName(kindMeta(k).label)
              }}
              options={KINDS.map((k) => ({
                value: k.kind,
                label: k.label,
                description: k.description,
                icon: <TrackerIcon kind={k.kind} size="lg" decorative />,
              }))}
            />
          )}
          <Field label="Name" error={fieldErrors.name}>
            {(p) => <Input {...p} value={name} onChange={(e) => setName(e.target.value)} placeholder={m.label} />}
          </Field>
          <div className="grid gap-4 sm:grid-cols-2">
            {m.fields.map((f) => (
              <Field
                key={f.key}
                label={f.label}
                hint={f.hint}
                error={fieldErrors[f.key] ?? fieldErrors[`config.${f.key}`]}
                optional={!f.required}
                className={f.key === 'base_url' ? 'sm:col-span-2' : undefined}
              >
                {(p) => (
                  <Input
                    {...p}
                    value={config[f.key] ?? ''}
                    placeholder={f.placeholder}
                    onChange={(e) => setConfig((c) => ({ ...c, [f.key]: e.target.value }))}
                    className={f.key === 'base_url' || f.key === 'email' ? 'font-mono text-[13px]' : undefined}
                    autoComplete="off"
                  />
                )}
              </Field>
            ))}
          </div>
          <Field
            label={m.tokenLabel}
            error={fieldErrors.token}
            hint={integration?.token_hint ? `Stored: ${integration.token_hint}. Leave empty to keep it. ${m.tokenHint}` : m.tokenHint}
          >
            {(p) => (
              <Input
                {...p}
                type="password"
                value={token}
                onChange={(e) => setToken(e.target.value)}
                className="font-mono text-[13px]"
                autoComplete="new-password"
                placeholder={integration ? '••••••••' : ''}
              />
            )}
          </Field>
          {(m.trackerGeneratesSecret || integration) && (
            <Field
              label="Webhook signing secret"
              optional={!m.trackerGeneratesSecret}
              error={fieldErrors.webhook_secret}
              hint={
                m.trackerGeneratesSecret
                  ? `Paste the signing secret ${m.label} shows for the webhook.${integration?.has_webhook_secret ? ' One is stored; leave empty to keep it.' : ''}`
                  : 'Leave empty to keep the current secret; use “New secret” in the list to generate one.'
              }
            >
              {(p) => (
                <Input {...p} type="password" value={secret} onChange={(e) => setSecret(e.target.value)} className="font-mono text-[13px]" autoComplete="off" />
              )}
            </Field>
          )}
          <div className="flex items-start gap-2">
            <Checkbox id="validate-token" checked={validate} onCheckedChange={(v) => setValidate(v === true)} />
            <Label htmlFor="validate-token" className="leading-snug font-normal">
              Check the token with {m.label} before saving
            </Label>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy}>
              {busy ? 'Checking…' : integration ? 'Save' : 'Connect'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function IntegrationCard({
  org,
  i,
  can,
  onEdit,
  onShowWebhook,
}: {
  org: string
  i: Integration
  can: boolean
  onEdit: () => void
  onShowWebhook: (i: IntegrationWithSecret) => void
}) {
  const qc = useQueryClient()
  const m = kindMeta(i.kind)
  const [test, setTest] = useState<TestResult | null>(null)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const invalidate = () => qc.invalidateQueries({ queryKey: ik.list(org) })
  const toggle = useMutation({
    mutationFn: (enabled: boolean) => integrationsApi.update(org, i.id, { enabled, validate: false }),
    onSettled: invalidate,
    onError: (e) => toast.error(errorMessage(e)),
  })
  const testM = useMutation({
    mutationFn: () => integrationsApi.test(org, i.id),
    onSuccess: (r) => {
      setTest(r)
      void invalidate()
    },
    onError: (e) => setTest({ ok: false, account: null, error: errorMessage(e), reason: null }),
  })
  const rotate = useMutation({
    mutationFn: () => integrationsApi.update(org, i.id, { rotate_webhook_secret: true, validate: false }),
    onSuccess: (r) => {
      onShowWebhook(r)
      void invalidate()
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const del = useMutation({
    mutationFn: () => integrationsApi.remove(org, i.id),
    onSuccess: () => toast.success(`${i.name} removed`),
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: invalidate,
  })
  return (
    <li className="px-4 py-3" aria-label={i.name}>
      <div className="flex flex-wrap items-center gap-3">
        <TrackerIcon kind={i.kind} size="lg" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2 font-medium">
            {i.name}
            <Badge variant="outline">{m.label}</Badge>
            {!i.enabled && <Badge variant="secondary">disabled</Badge>}
            {i.last_error && (
              <Tip content={i.last_error_at ? `Last failure ${new Date(i.last_error_at).toLocaleString()}` : 'Last failure'}>
                <Badge variant="destructive">
                  <CircleAlert /> error
                </Badge>
              </Tip>
            )}
          </div>
          <div className="mt-0.5 flex flex-wrap gap-x-3 text-xs text-muted-foreground">
            {i.config.base_url && <span className="font-mono">{i.config.base_url}</span>}
            {i.account && <span>as {i.account.name}</span>}
            {i.token_hint && <span className="font-mono">token {i.token_hint}</span>}
            <span>
              updated <RelativeTime date={i.updated_at} suffix plain />
            </span>
          </div>
        </div>
        <div className="flex items-center gap-1">
          {can && (
            <Switch
              checked={i.enabled}
              onCheckedChange={(v) => toggle.mutate(v)}
              aria-label={`${i.enabled ? 'Disable' : 'Enable'} ${i.name}`}
              className="mr-2"
            />
          )}
          <Button variant="outline" size="xs" onClick={() => testM.mutate()} disabled={testM.isPending || !can}>
            <Send /> {testM.isPending ? 'Testing…' : 'Test'}
          </Button>
          {can && (
            <>
              <Button variant="ghost" size="icon-xs" aria-label={`Edit ${i.name}`} onClick={onEdit}>
                <Pencil />
              </Button>
              <Button variant="ghost" size="icon-xs" aria-label={`Delete ${i.name}`} onClick={() => setConfirmDelete(true)}>
                <Trash2 />
              </Button>
            </>
          )}
        </div>
      </div>
      {i.last_error && <p className="mt-2 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-1.5 text-xs text-destructive">{i.last_error}</p>}
      {test && (
        <p
          className={cn(
            'mt-2 flex items-center gap-2 rounded-md border px-3 py-1.5 text-xs',
            test.ok ? 'border-success/40 bg-success-soft text-success' : 'border-destructive/30 bg-destructive/5 text-destructive',
          )}
          role="status"
        >
          {test.ok ? <CircleCheck className="size-3.5" /> : <CircleAlert className="size-3.5" />}
          {test.ok
            ? `Connection works${test.account ? `: signed in as ${test.account.name}` : ''}.`
            : `${test.error ?? 'The test failed.'}${test.reason ? ` (${test.reason})` : ''}`}
        </p>
      )}
      <div className="mt-2.5 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
        <CopyField value={i.webhook_url} label={`Copy webhook URL of ${i.name}`} />
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <KeyRound className="size-3.5" />
          {i.has_webhook_secret ? 'Webhook secret stored' : 'No webhook secret yet'}
          {can && !kindMeta(i.kind).trackerGeneratesSecret && (
            <Button variant="ghost" size="xs" onClick={() => rotate.mutate()} disabled={rotate.isPending}>
              <RefreshCw /> New secret
            </Button>
          )}
          <a href={m.docs} target="_blank" rel="noreferrer noopener" className="inline-flex items-center gap-1 text-palm-text hover:text-brand">
            <BookOpen className="size-3.5" /> Setup
          </a>
        </div>
      </div>
      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove {i.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              Links to its tickets are removed from issues (the tickets stay in {m.label}), and alert rules that create tickets with it stop doing so.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction destructive onClick={() => del.mutate()}>
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </li>
  )
}

export function IntegrationsPage({ org }: { org: string }) {
  const q = useQuery({ queryKey: ik.list(org), queryFn: () => integrationsApi.list(org) })
  const { can } = useRole('admin')
  const [editing, setEditing] = useState<{ open: boolean; integration: Integration | null }>({ open: false, integration: null })
  const [webhook, setWebhook] = useState<IntegrationWithSecret | null>(null)
  return (
    <SettingsPage
      title="Integrations"
      description="Connect Jira, Linear, GitHub or GitLab to create tickets from issues and keep their status in sync. Tokens and secrets are stored encrypted and never shown again."
      actions={
        can && (
          <Button size="sm" onClick={() => setEditing({ open: true, integration: null })}>
            <Plus /> Connect tracker
          </Button>
        )
      }
    >
      {q.isPending ? (
        <ListSkeleton rows={3} />
      ) : q.isError ? (
        <ErrorState error={q.error} retry={() => q.refetch()} />
      ) : q.data.length === 0 ? (
        <SettingsCard>
          <EmptyState
            compact
            icon={<Plug />}
            title="No issue trackers connected"
            description="Connect a tracker to create tickets from issues, link existing ones and resolve issues when their ticket is done."
            action={
              can ? (
                <Button size="sm" onClick={() => setEditing({ open: true, integration: null })}>
                  <Plus /> Connect tracker
                </Button>
              ) : undefined
            }
          />
        </SettingsCard>
      ) : (
        <ul className="divide-y rounded-lg border bg-card" aria-label="Integrations">
          {q.data.map((i) => (
            <IntegrationCard key={i.id} org={org} i={i} can={can} onEdit={() => setEditing({ open: true, integration: i })} onShowWebhook={setWebhook} />
          ))}
        </ul>
      )}
      <p className="text-xs text-muted-foreground">Per project, choose where new tickets go and how statuses sync under project settings → Integrations.</p>
      {editing.open && (
        <IntegrationDialog
          key={editing.integration?.id ?? 'new'}
          org={org}
          integration={editing.integration}
          open={editing.open}
          onOpenChange={(o) => setEditing((e) => ({ ...e, open: o }))}
          onCreated={setWebhook}
        />
      )}
      {webhook && <WebhookDialog integration={webhook} open onOpenChange={(o) => !o && setWebhook(null)} />}
    </SettingsPage>
  )
}
