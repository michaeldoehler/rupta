import { useQuery, useQueryClient } from '@tanstack/react-query'
import { ChevronDown, CircleCheck, CircleX, ExternalLink, Loader2, Plus, ShieldCheck, Trash2, TriangleAlert } from 'lucide-react'
import { type ReactNode, useEffect, useState } from 'react'
import { toast } from 'sonner'
import { ApiError, errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { qk } from '@/api/queries'
import type { InstanceSso, Role, SsoConfig, SsoDefaultRole, SsoTestResult } from '@/api/types'
import { BrandPicker } from '@/components/brand/brand-picker'
import { SsoProviderIcon } from '@/components/brand/sso-icon'
import { CopyField } from '@/components/common/copy-button'
import { Field, FormError } from '@/components/common/field'
import { CardsSkeleton, ErrorState } from '@/components/common/states'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { cn } from '@/lib/utils'
import { useRole } from './org-pages'
import { SettingsCard, SettingsPage, ToggleRow } from './settings-layout'

type Form = {
  provider_name: string
  issuer: string
  client_id: string
  client_secret: string
  scopes: string
  role_claim: string
  mapping: [string, Role][]
  default_role: SsoDefaultRole
  jit_provisioning: boolean
  enforce: boolean
  owner_local_login: boolean
}

const PRESETS: Record<string, Partial<Form> & { label: string; issuerHint: string }> = {
  zitadel: {
    label: 'Zitadel',
    provider_name: 'Zitadel',
    // The reserved scope that asks for the roles of the requested project; the
    // roles then arrive in the claim of the same name (RUPTA_OIDC_SCOPES default).
    scopes: 'openid email profile urn:zitadel:iam:org:project:roles',
    role_claim: 'urn:zitadel:iam:org:project:roles',
    mapping: [
      ['rupta-admin', 'admin'],
      ['rupta-member', 'member'],
      ['rupta-viewer', 'viewer'],
    ],
    issuerHint: 'https://your-instance.zitadel.cloud',
  },
  keycloak: {
    label: 'Keycloak',
    provider_name: 'Keycloak',
    scopes: 'openid email profile roles',
    role_claim: 'realm_access.roles',
    mapping: [],
    issuerHint: 'https://keycloak.example.com/realms/acme',
  },
  entra: {
    label: 'Microsoft Entra ID',
    provider_name: 'Microsoft',
    scopes: 'openid email profile',
    role_claim: 'roles',
    mapping: [],
    issuerHint: 'https://login.microsoftonline.com/<tenant-id>/v2.0',
  },
  generic: {
    label: 'Other OIDC provider',
    provider_name: 'SSO',
    scopes: 'openid email profile',
    role_claim: '',
    mapping: [],
    issuerHint: 'https://idp.example.com',
  },
}

const ROLES: Role[] = ['owner', 'admin', 'member', 'viewer']

const DEFAULT_ROLES: { value: SsoDefaultRole; label: string }[] = [
  { value: 'admin', label: 'Admin' },
  { value: 'member', label: 'Member' },
  { value: 'viewer', label: 'Viewer' },
  { value: 'none', label: 'No access (refuse sign-in)' },
]

const ROLE_LABELS: Record<string, string> = {
  owner: 'Owner',
  admin: 'Admin',
  member: 'Member',
  viewer: 'Viewer',
  none: 'No access (refuse sign-in)',
}

export const SSO_DOCS = 'https://github.com/michaeldoehler/rupta/blob/main/docs/sso.md'

function toForm(c: SsoConfig | null): Form {
  return {
    provider_name: c?.provider_name ?? '',
    issuer: c?.issuer ?? '',
    client_id: c?.client_id ?? '',
    client_secret: '',
    scopes: c?.scopes ?? 'openid email profile',
    role_claim: c?.role_claim ?? '',
    mapping: Object.entries(c?.role_mapping ?? {}),
    default_role: c?.default_role ?? 'member',
    jit_provisioning: c?.jit_provisioning ?? true,
    enforce: c?.enforce ?? false,
    owner_local_login: c?.owner_local_login ?? true,
  }
}

export function SsoPage({ org }: { org: string }) {
  const q = useQuery({ queryKey: qk.sso(org), queryFn: () => api.orgs.sso(org), retry: (n, e) => !(e instanceof ApiError && e.status === 403) && n < 2 })
  const iq = useQuery({ queryKey: qk.instanceSso(org), queryFn: () => api.orgs.instanceSso(org), retry: false })
  const qc = useQueryClient()
  const owner = useRole('owner').can
  const [orgOpen, setOrgOpen] = useState<boolean | null>(null)
  const [form, setForm] = useState<Form>(() => toForm(null))
  const [preset, setPreset] = useState<string>('zitadel')
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [testing, setTesting] = useState(false)
  const [test, setTest] = useState<SsoTestResult | null>(null)

  useEffect(() => {
    if (q.isSuccess) {
      if (q.data) setForm(toForm(q.data))
      else applyPreset('zitadel')
    }
  }, [q.isSuccess, q.data])

  function applyPreset(key: string) {
    setPreset(key)
    const p = PRESETS[key]
    setForm((f) => ({
      ...f,
      provider_name: p.provider_name ?? f.provider_name,
      scopes: p.scopes ?? f.scopes,
      role_claim: p.role_claim ?? '',
      mapping: p.mapping ?? [],
    }))
  }

  const set = <K extends keyof Form>(k: K, v: Form[K]) => setForm((f) => ({ ...f, [k]: v }))
  const body = (): Partial<SsoConfig> => ({
    provider_name: form.provider_name.trim() || 'SSO',
    issuer: form.issuer.trim().replace(/\/$/, ''),
    client_id: form.client_id.trim(),
    ...(form.client_secret ? { client_secret: form.client_secret } : {}),
    scopes: form.scopes.trim(),
    role_claim: form.role_claim.trim() || null,
    role_mapping: Object.fromEntries(form.mapping.filter(([k]) => k.trim()).map(([k, v]) => [k.trim(), v])),
    default_role: form.default_role,
    jit_provisioning: form.jit_provisioning,
    enforce: form.enforce,
    owner_local_login: form.owner_local_login,
  })

  function validate() {
    const e: Record<string, string> = {}
    if (!/^https:\/\/.+/.test(form.issuer.trim())) e.issuer = 'Enter the https:// URL of the issuer.'
    if (!form.client_id.trim()) e.client_id = 'Required.'
    if (!form.client_secret && !q.data?.has_client_secret) e.client_secret = 'Required.'
    if (!form.scopes.split(/\s+/).includes('openid')) e.scopes = 'Must include “openid”.'
    if (form.default_role === 'none') {
      if (!form.role_claim.trim()) e.role_claim = 'Required when users without a matching role are refused.'
      if (!form.mapping.some(([k]) => k.trim())) e.mapping = 'Add at least one mapping when users without a matching role are refused.'
    }
    setErrors(e)
    return Object.keys(e).length === 0
  }

  async function runTest() {
    setTesting(true)
    setTest(null)
    try {
      setTest(await api.orgs.testSso(org, body()))
    } catch (e) {
      // Servers without a test endpoint: check the discovery document from the browser.
      if (e instanceof ApiError && (e.status === 404 || e.status === 405)) setTest(await browserDiscoveryCheck(form.issuer.trim().replace(/\/$/, '')))
      else setTest({ ok: false, message: errorMessage(e) })
    } finally {
      setTesting(false)
    }
  }

  async function remove() {
    try {
      await api.orgs.deleteSso(org)
      qc.setQueryData(qk.sso(org), null)
      setForm(toForm(null))
      applyPreset('zitadel')
      toast.success('Single sign-on removed')
    } catch (err) {
      toast.error(errorMessage(err))
    }
  }

  async function save() {
    if (!validate()) return
    setSaving(true)
    setError(null)
    try {
      const saved = await api.orgs.saveSso(org, body())
      qc.setQueryData(qk.sso(org), saved)
      setForm(toForm(saved))
      toast.success('Single sign-on saved')
    } catch (err) {
      if (err instanceof ApiError) setErrors(err.fieldErrors)
      setError(errorMessage(err))
    } finally {
      setSaving(false)
    }
  }

  if (q.isPending || iq.isPending)
    return (
      <SettingsPage title="Single sign-on">
        <CardsSkeleton count={3} />
      </SettingsPage>
    )
  // Admins may see the instance provider but not the organization's settings.
  const orgForbidden = q.isError && q.error instanceof ApiError && q.error.status === 403
  if (q.isError && !orgForbidden) return <ErrorState error={q.error} />
  const instance = iq.data?.provider ?? null
  const localLogin = iq.data?.local_login ?? true
  const redirect = q.data?.redirect_uri ?? `${location.origin}/auth/sso/org-${org}/callback`
  const configured = Boolean(q.data)
  const open = orgOpen ?? (configured || !instance)

  const orgSection = orgForbidden ? (
    <p className="rounded-md border bg-muted/50 px-3 py-2 text-[13px] text-muted-foreground">
      Only owners can see and change this organization's own provider.
    </p>
  ) : (
    <>
      {!owner && <p className="rounded-md border bg-muted/50 px-3 py-2 text-[13px] text-muted-foreground">Only owners can change single sign-on.</p>}
      <FormError message={error} />
      <SettingsCard title="Provider" description="Choose a preset to fill in typical values.">
        <BrandPicker
          label="Provider preset"
          layout="chips"
          className="mb-4"
          value={preset}
          onChange={applyPreset}
          disabled={!owner}
          options={Object.entries(PRESETS).map(([k, p]) => ({ value: k, label: p.label, icon: <SsoProviderIcon name={k} size="sm" decorative /> }))}
        />
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Button label" hint={`Shown on the sign-in page as “Continue with ${form.provider_name || 'SSO'}”.`}>
            {(p) => <Input {...p} value={form.provider_name} disabled={!owner} onChange={(e) => set('provider_name', e.target.value)} />}
          </Field>
          <Field label="Issuer URL" error={errors.issuer} hint="rupta reads /.well-known/openid-configuration from it.">
            {(p) => (
              <Input
                {...p}
                value={form.issuer}
                disabled={!owner}
                placeholder={PRESETS[preset]?.issuerHint}
                onChange={(e) => set('issuer', e.target.value)}
                className="font-mono text-[13px]"
              />
            )}
          </Field>
          <Field label="Client ID" error={errors.client_id}>
            {(p) => (
              <Input
                {...p}
                value={form.client_id}
                disabled={!owner}
                onChange={(e) => set('client_id', e.target.value)}
                className="font-mono text-[13px]"
                autoComplete="off"
              />
            )}
          </Field>
          <Field
            label="Client secret"
            error={errors.client_secret}
            hint={q.data?.has_client_secret ? 'A secret is stored. Leave empty to keep it.' : 'Stored encrypted.'}
          >
            {(p) => (
              <Input
                {...p}
                type="password"
                value={form.client_secret}
                disabled={!owner}
                placeholder={q.data?.has_client_secret ? '••••••••••••' : ''}
                onChange={(e) => set('client_secret', e.target.value)}
                autoComplete="new-password"
              />
            )}
          </Field>
          <Field label="Scopes" error={errors.scopes} className="sm:col-span-2">
            {(p) => <Input {...p} value={form.scopes} disabled={!owner} onChange={(e) => set('scopes', e.target.value)} className="font-mono text-[13px]" />}
          </Field>
          <div className="sm:col-span-2">
            <p className="mb-1.5 text-[13px] font-medium">Redirect URI</p>
            <CopyField value={redirect} label="Copy redirect URI" />
            <p className="mt-1 text-xs text-muted-foreground">Register this URI in the application at your identity provider.</p>
          </div>
        </div>
      </SettingsCard>

      <SettingsCard title="Roles" description="Map role or group names from the ID token to rupta roles. The highest matching role wins.">
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Role claim" optional error={errors.role_claim} hint="Claim with roles or groups. Nested claims use dots.">
            {(p) => (
              <Input
                {...p}
                value={form.role_claim}
                disabled={!owner}
                placeholder="groups"
                onChange={(e) => set('role_claim', e.target.value)}
                className="font-mono text-[13px]"
              />
            )}
          </Field>
          <Field
            label="Default role"
            hint={
              form.default_role === 'none'
                ? 'Users without a matching role cannot sign in, and no account is created for them. Owners can always sign in.'
                : 'For new members without a matching role; existing members keep their role.'
            }
          >
            {(p) => (
              <Select value={form.default_role} onValueChange={(v) => set('default_role', v as SsoDefaultRole)} disabled={!owner}>
                <SelectTrigger id={p.id} className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {DEFAULT_ROLES.map((r) => (
                    <SelectItem key={r.value} value={r.value}>
                      {r.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </Field>
        </div>
        <div className="mt-4 space-y-2">
          <div className="grid grid-cols-[1fr_auto_8rem_auto] items-center gap-2 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
            <span>Value in the token</span>
            <span />
            <span>rupta role</span>
            <span className="w-6" />
          </div>
          {form.mapping.map(([k, v], i) => (
            <div key={i} className="grid grid-cols-[1fr_auto_8rem_auto] items-center gap-2">
              <Input
                value={k}
                disabled={!owner}
                onChange={(e) =>
                  set(
                    'mapping',
                    form.mapping.map((m, j) => (j === i ? [e.target.value, m[1]] : m)),
                  )
                }
                className="h-8 font-mono text-[13px]"
                aria-label={`Token value ${i + 1}`}
              />
              <span className="text-muted-foreground">→</span>
              <Select
                value={v}
                disabled={!owner}
                onValueChange={(r) =>
                  set(
                    'mapping',
                    form.mapping.map((m, j) => (j === i ? [m[0], r as Role] : m)),
                  )
                }
              >
                <SelectTrigger size="sm" className="w-full capitalize" aria-label={`Role for mapping ${i + 1}`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {ROLES.map((r) => (
                    <SelectItem key={r} value={r} className="capitalize">
                      {r}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button
                variant="ghost"
                size="icon-xs"
                disabled={!owner}
                onClick={() =>
                  set(
                    'mapping',
                    form.mapping.filter((_, j) => j !== i),
                  )
                }
                aria-label={`Remove mapping ${i + 1}`}
              >
                <Trash2 />
              </Button>
            </div>
          ))}
          <Button variant="ghost" size="sm" disabled={!owner} onClick={() => set('mapping', [...form.mapping, ['', 'member']])}>
            <Plus /> Add mapping
          </Button>
          {errors.mapping && <p className="text-xs text-destructive">{errors.mapping}</p>}
        </div>
      </SettingsCard>

      <SettingsCard title="Behaviour">
        <div className="divide-y">
          <ToggleRow
            title="Create accounts on first sign-in"
            description="New users who authenticate at the provider become members with their mapped role. When off, they must be invited first."
          >
            <Switch
              checked={form.jit_provisioning}
              disabled={!owner}
              onCheckedChange={(v) => set('jit_provisioning', v)}
              aria-label="Create accounts on first sign-in"
            />
          </ToggleRow>
          <ToggleRow
            title="Require single sign-on"
            description="Members must sign in through the provider; their passwords stop working for this organization."
          >
            <Switch checked={form.enforce} disabled={!owner} onCheckedChange={(v) => set('enforce', v)} aria-label="Require single sign-on" />
          </ToggleRow>
          <ToggleRow title="Owners may still use their password" description="A fallback in case the identity provider is down or misconfigured.">
            <Switch
              checked={form.owner_local_login}
              disabled={!owner}
              onCheckedChange={(v) => set('owner_local_login', v)}
              aria-label="Owners may still use their password"
            />
          </ToggleRow>
        </div>
        {form.enforce && !configured && (
          <p className="mt-3 flex items-center gap-2 text-xs text-warning">
            <TriangleAlert className="size-3.5" /> Test the connection before requiring single sign-on.
          </p>
        )}
      </SettingsCard>

      {test && (
        <div
          role="status"
          className={cn(
            'flex gap-2.5 rounded-lg border px-3 py-2.5 text-[13px]',
            test.ok ? 'border-success/40 bg-success-soft' : 'border-destructive/40 bg-destructive/8',
          )}
        >
          {test.ok ? <CircleCheck className="mt-0.5 size-4 shrink-0 text-success" /> : <CircleX className="mt-0.5 size-4 shrink-0 text-destructive" />}
          <div>
            <p className="font-medium">{test.ok ? 'Connection works' : 'Connection failed'}</p>
            <p className="text-muted-foreground">{test.message}</p>
            {test.authorization_endpoint && <p className="mt-1 font-mono text-xs text-muted-foreground">{test.authorization_endpoint}</p>}
          </div>
        </div>
      )}

      <div className="flex flex-wrap justify-end gap-2">
        {configured && (
          <Button variant="ghost" className="mr-auto text-destructive" disabled={!owner} onClick={() => void remove()}>
            Remove single sign-on
          </Button>
        )}
        <Button variant="outline" disabled={!owner || testing || !form.issuer} onClick={() => void runTest()}>
          {testing && <Loader2 className="animate-spin" />} Test connection
        </Button>
        <Button variant="brand" disabled={!owner || saving} onClick={() => void save()}>
          {saving && <Loader2 className="animate-spin" />} Save
        </Button>
      </div>
    </>
  )

  return (
    <SettingsPage
      title="Single sign-on"
      description="Let members sign in with an OpenID Connect identity provider. Roles can be taken from a claim of the ID token."
      actions={
        configured || instance ? (
          <Badge variant="success">
            <ShieldCheck /> Configured
          </Badge>
        ) : (
          <Badge variant="outline">Not configured</Badge>
        )
      }
    >
      {instance && <InstanceSsoCard instance={instance} localLogin={localLogin} org={org} />}
      {instance ? (
        <Collapsible open={open} onOpenChange={setOrgOpen} className="space-y-6">
          <section className="rounded-lg border bg-card">
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="max-w-2xl">
                <h2 className="text-sm font-semibold">Use a different identity provider for this organization (optional)</h2>
                <div className="mt-1 space-y-1 text-[13px] text-muted-foreground">
                  <p>
                    The sign-in page always shows the instance provider. This organization's provider appears next to it when the sign-in page is opened with{' '}
                    <code className="font-mono text-xs">?org={org}</code>.
                  </p>
                  <p>
                    Each provider sets roles only in its own organization: the instance provider in{' '}
                    <span className="font-mono text-xs">{instance.default_org}</span>, this one in this organization. If both apply to the same organization,
                    the provider a member signed in with last sets their role.
                  </p>
                  <p>“Require single sign-on” below refuses passwords for this organization's members; signing in with the instance provider still works.</p>
                </div>
              </div>
              <CollapsibleTrigger asChild>
                <Button variant="outline" size="sm" aria-expanded={open}>
                  <ChevronDown className={cn('transition-transform', open && 'rotate-180')} />
                  {open ? 'Hide' : configured ? 'Show organization provider' : 'Set up organization provider'}
                </Button>
              </CollapsibleTrigger>
            </div>
          </section>
          <CollapsibleContent className="space-y-6">{orgSection}</CollapsibleContent>
        </Collapsible>
      ) : (
        orgSection
      )}
    </SettingsPage>
  )
}

/** The provider from the deployment's RUPTA_OIDC_* variables, read-only. */
function InstanceSsoCard({ instance, localLogin, org }: { instance: InstanceSso; localLogin: boolean; org: string }) {
  const mapping = Object.entries(instance.role_mapping)
  const rows: [string, ReactNode][] = [
    ['Button label', instance.provider_name],
    ['Issuer URL', <span className="font-mono text-[13px] break-all">{instance.issuer}</span>],
    ['Client ID', <span className="font-mono text-[13px] break-all">{instance.client_id}</span>],
    ['Client secret', instance.has_client_secret ? 'Set' : 'Not set (public client)'],
    ['Scopes', <span className="font-mono text-[13px] break-all">{instance.scopes}</span>],
    ['Role claim', instance.role_claim ? <span className="font-mono text-[13px] break-all">{instance.role_claim}</span> : 'None (no role mapping)'],
    [
      'Role mapping',
      mapping.length ? (
        <ul className="space-y-0.5">
          {mapping.map(([k, v]) => (
            <li key={k}>
              <span className="font-mono text-[13px]">{k}</span> → {ROLE_LABELS[v] ?? v}
            </li>
          ))}
        </ul>
      ) : (
        'None'
      ),
    ],
    ['Default role', ROLE_LABELS[instance.default_role] ?? instance.default_role],
    ['Create accounts on first sign-in', instance.jit_provisioning ? 'On' : 'Off'],
    [
      'Additional audiences',
      instance.allowed_audiences.length ? (
        <span className="font-mono text-[13px]">{instance.allowed_audiences.join(', ')}</span>
      ) : (
        'Any (the client ID must be one of them)'
      ),
    ],
    ['Members join', instance.default_org === org ? 'This organization' : <span className="font-mono text-[13px]">{instance.default_org}</span>],
    ['Password login', localLogin ? 'Enabled' : 'Disabled for this instance'],
  ]
  return (
    <section className="rounded-lg border bg-card" aria-label="Single sign-on for this instance">
      <div className="flex flex-wrap items-start justify-between gap-3 border-b px-4 py-3">
        <div>
          <h2 className="text-sm font-semibold">Single sign-on for this instance</h2>
          <p className="mt-0.5 text-[13px] text-muted-foreground">Configured by environment variables of the deployment.</p>
        </div>
        <Badge variant="success">
          <ShieldCheck /> Configured
        </Badge>
      </div>
      <div className="p-4">
        <dl className="grid gap-x-6 gap-y-2 text-[13px] sm:grid-cols-[12rem_1fr]">
          {rows.map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-muted-foreground">{k}</dt>
              <dd>{v}</dd>
            </div>
          ))}
        </dl>
        <div className="mt-4">
          <p className="mb-1.5 text-[13px] font-medium">Redirect URI</p>
          <CopyField value={instance.redirect_uri} label="Copy instance redirect URI" />
        </div>
        <p className="mt-4 text-xs text-muted-foreground">
          These settings are read-only here. Change them in the deployment's <code className="font-mono">RUPTA_OIDC_*</code> environment variables and restart
          rupta.{' '}
          <a href={SSO_DOCS} target="_blank" rel="noreferrer" className="inline-flex items-center gap-0.5 underline underline-offset-2">
            Single sign-on documentation <ExternalLink className="size-3" />
          </a>
        </p>
      </div>
    </section>
  )
}

/** Fetches the issuer's discovery document directly; needs CORS on the identity provider. */
async function browserDiscoveryCheck(issuer: string): Promise<SsoTestResult> {
  try {
    const res = await fetch(`${issuer}/.well-known/openid-configuration`, { credentials: 'omit' })
    if (!res.ok) return { ok: false, message: `${issuer}/.well-known/openid-configuration answered HTTP ${res.status}.` }
    const doc = (await res.json()) as { issuer?: string; authorization_endpoint?: string }
    if (doc.issuer && doc.issuer.replace(/\/$/, '') !== issuer) return { ok: false, message: `The discovery document names a different issuer: ${doc.issuer}` }
    return {
      ok: true,
      message: 'Discovery document loaded (checked from this browser; the server checks again at sign-in).',
      issuer: doc.issuer,
      authorization_endpoint: doc.authorization_endpoint,
    }
  } catch {
    return {
      ok: false,
      message:
        'Could not load the discovery document from this browser (network error or the provider does not allow cross-origin requests). Save and try a sign-in to check from the server.',
    }
  }
}
