import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { FileArchive, FolderPlus, Plus, Trash2 } from 'lucide-react'
import { useEffect, useState, type ReactNode } from 'react'
import { toast } from 'sonner'
import { ApiError, errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { qk, useApiMutation, useProjects, useTeams } from '@/api/queries'
import type { Project, ProjectKey, ProjectSettings } from '@/api/types'
import { CodeBlock } from '@/components/common/code-block'
import { CopyField } from '@/components/common/copy-button'
import { Field, FormError } from '@/components/common/field'
import { PlatformIcon, platformLabel } from '@/components/common/platform-icon'
import { Sparkline } from '@/components/common/sparkline'
import { CardsSkeleton, EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
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
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { bytes } from '@/lib/format'
import { cn } from '@/lib/utils'
import { LinesInput } from './lines-input'
import { useRole } from './org-pages'
import { guideFor, SDK_GUIDES } from './sdk-snippets'
import { SettingsCard, SettingsPage, ToggleRow } from './settings-layout'

export const PLATFORMS = [
  'javascript',
  'javascript-react',
  'javascript-nextjs',
  'node',
  'python',
  'python-django',
  'python-flask',
  'python-fastapi',
  'php',
  'php-laravel',
  'php-symfony',
  'go',
  'java',
  'dotnet',
  'ruby',
  'rust',
]

// ---- project list ------------------------------------------------------------

export function ProjectsPage({ org }: { org: string }) {
  const projects = useProjects(org)
  const teams = useTeams(org)
  const { can } = useRole('admin')
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState({ name: '', platform: 'javascript-react', team: '__none' })
  const [error, setError] = useState<string | null>(null)

  async function create(e: React.FormEvent) {
    e.preventDefault()
    if (!form.name.trim()) return setError('Enter a name.')
    try {
      const p = await api.projects.create(org, { name: form.name.trim(), platform: form.platform, team: form.team === '__none' ? null : form.team })
      await qc.invalidateQueries({ queryKey: qk.projects(org) })
      toast.success(`Project ${p.slug} created`)
      setOpen(false)
      await navigate({ to: '/$org/settings/projects/$project/keys', params: { org, project: p.slug } })
    } catch (err) {
      setError(err instanceof ApiError ? (err.fieldErrors.name ?? err.message) : errorMessage(err))
    }
  }

  return (
    <SettingsPage
      title="Projects"
      description="A project receives events from one application or service and has its own DSN, settings and alert rules."
      actions={
        can && (
          <Button size="sm" variant="brand" onClick={() => setOpen(true)}>
            <FolderPlus /> New project
          </Button>
        )
      }
    >
      {projects.isPending ? (
        <ListSkeleton rows={5} />
      ) : projects.isError ? (
        <ErrorState error={projects.error} />
      ) : projects.data.length === 0 ? (
        <EmptyState icon={<FolderPlus />} title="No projects yet" description="Create a project to get a DSN for your SDK." />
      ) : (
        <ul className="divide-y rounded-lg border bg-card">
          {projects.data.map((p) => (
            <li key={p.id}>
              <Link to="/$org/settings/projects/$project" params={{ org, project: p.slug }} className="flex items-center gap-3 px-4 py-3 hover:bg-row-hover">
                <PlatformIcon platform={p.platform} size="lg" />
                <div className="min-w-0 flex-1">
                  <div className="font-medium">{p.name}</div>
                  <div className="text-xs text-muted-foreground">
                    <span className="font-mono">{p.slug}</span> · {platformLabel(p.platform)} {p.team && <>· #{p.team.slug}</>}
                  </div>
                </div>
                {p.stats && (
                  <div className="hidden items-center gap-2 sm:flex">
                    <Sparkline data={p.stats} width={120} height={24} label="Events in the last 24 hours" />
                    <span className="w-16 text-right text-xs text-muted-foreground">24h</span>
                  </div>
                )}
              </Link>
            </li>
          ))}
        </ul>
      )}
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <form onSubmit={create} className="grid gap-4" noValidate>
            <DialogHeader>
              <DialogTitle>New project</DialogTitle>
              <DialogDescription>You get a DSN and setup instructions for the platform right after.</DialogDescription>
            </DialogHeader>
            <FormError message={error} />
            <Field label="Name">
              {(p) => (
                <Input {...p} autoFocus value={form.name} onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))} placeholder="Checkout service" />
              )}
            </Field>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="Platform">
                {(p) => (
                  <Select value={form.platform} onValueChange={(v) => setForm((f) => ({ ...f, platform: v }))}>
                    <SelectTrigger id={p.id} className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {PLATFORMS.map((pl) => (
                        <SelectItem key={pl} value={pl}>
                          <PlatformIcon platform={pl} size="sm" /> {platformLabel(pl)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              </Field>
              <Field label="Team" optional>
                {(p) => (
                  <Select value={form.team} onValueChange={(v) => setForm((f) => ({ ...f, team: v }))}>
                    <SelectTrigger id={p.id} className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__none">No team</SelectItem>
                      {teams.data?.map((t) => (
                        <SelectItem key={t.id} value={t.id}>
                          #{t.slug}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              </Field>
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setOpen(false)}>
                Cancel
              </Button>
              <Button type="submit">Create project</Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </SettingsPage>
  )
}

// ---- project layout ----------------------------------------------------------

const PROJECT_TABS = [
  ['/$org/settings/projects/$project', 'General'],
  ['/$org/settings/projects/$project/keys', 'Client keys & SDK'],
  ['/$org/settings/projects/$project/filters', 'Inbound filters'],
  ['/$org/settings/projects/$project/scrubbing', 'Data scrubbing'],
  ['/$org/settings/projects/$project/security', 'Security'],
  ['/$org/settings/projects/$project/limits', 'Rate limits & retention'],
  ['/$org/settings/projects/$project/source-maps', 'Source maps'],
] as const

export function useProject(org: string, project: string) {
  return useQuery({ queryKey: qk.project(org, project), queryFn: () => api.projects.get(org, project) })
}

export function ProjectLayout({ org, project, children }: { org: string; project: string; children: ReactNode }) {
  const q = useProject(org, project)
  return (
    <div>
      <div className="border-b px-4 pt-5 sm:px-8">
        <div className="mx-auto max-w-4xl">
          <div className="flex items-center gap-3">
            <PlatformIcon platform={q.data?.platform} size="lg" />
            <div>
              <h1 className="text-lg leading-tight font-semibold tracking-tight">{q.data?.name ?? project}</h1>
              <p className="font-mono text-xs text-muted-foreground">{project}</p>
            </div>
          </div>
          <nav aria-label="Project settings" className="mt-4 -mb-px flex gap-1 overflow-x-auto">
            {PROJECT_TABS.map(([to, label]) => (
              <Link
                key={to}
                to={to}
                params={{ org, project }}
                activeOptions={{ exact: true, includeSearch: false }}
                className="shrink-0 border-b-2 border-transparent px-2.5 pb-2 text-[13px] font-medium text-muted-foreground hover:text-foreground"
                activeProps={{ className: 'border-brand! text-foreground', 'aria-current': 'page' }}
              >
                {label}
              </Link>
            ))}
          </nav>
        </div>
      </div>
      {q.isError ? <ErrorState error={q.error} /> : children}
    </div>
  )
}

function useSaveSettings(org: string, project: string) {
  const qc = useQueryClient()
  return useApiMutation((settings: Partial<ProjectSettings>) => api.projects.update(org, project, { settings }), {
    success: 'Settings saved',
    onSuccess: (p) => {
      qc.setQueryData(qk.project(org, project), p)
      void qc.invalidateQueries({ queryKey: qk.projects(org) })
    },
  })
}

function Loading({ title }: { title: string }) {
  return (
    <SettingsPage title={title}>
      <CardsSkeleton count={2} />
    </SettingsPage>
  )
}

// ---- general -----------------------------------------------------------------

export function ProjectGeneralPage({ org, project }: { org: string; project: string }) {
  const q = useProject(org, project)
  const teams = useTeams(org)
  const { can } = useRole('admin')
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [form, setForm] = useState({ name: '', slug: '', platform: '', team: '__none' })
  const [confirm, setConfirm] = useState('')
  const [deleting, setDeleting] = useState(false)
  useEffect(() => {
    if (q.data) setForm({ name: q.data.name, slug: q.data.slug, platform: q.data.platform ?? '', team: q.data.team?.id ?? '__none' })
  }, [q.data])
  const save = useApiMutation(
    () =>
      api.projects.update(org, project, {
        name: form.name,
        slug: form.slug,
        platform: form.platform || null,
        team: form.team === '__none' ? null : form.team,
      }),
    {
      success: 'Project saved',
      invalidate: [qk.projects(org)],
      onSuccess: (p: Project) => {
        qc.setQueryData(qk.project(org, p.slug), p)
        if (p.slug !== project) void navigate({ to: '/$org/settings/projects/$project', params: { org, project: p.slug } })
      },
    },
  )
  if (!q.data) return <Loading title="General" />
  return (
    <SettingsPage title="General">
      <SettingsCard
        footer={
          <Button size="sm" disabled={!can || save.isPending} onClick={() => save.mutate(undefined)}>
            Save
          </Button>
        }
      >
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Name">
            {(p) => <Input {...p} value={form.name} disabled={!can} onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))} />}
          </Field>
          <Field label="Slug" hint="Used in URLs, the upload CLI and API paths.">
            {(p) => (
              <Input
                {...p}
                value={form.slug}
                disabled={!can}
                onChange={(e) => setForm((f) => ({ ...f, slug: e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, '-') }))}
                className="font-mono"
              />
            )}
          </Field>
          <Field label="Platform" hint="Chooses the default SDK instructions and syntax highlighting.">
            {(p) => (
              <Select value={form.platform} onValueChange={(v) => setForm((f) => ({ ...f, platform: v }))} disabled={!can}>
                <SelectTrigger id={p.id} className="w-full">
                  <SelectValue placeholder="Other" />
                </SelectTrigger>
                <SelectContent>
                  {PLATFORMS.map((pl) => (
                    <SelectItem key={pl} value={pl}>
                      <PlatformIcon platform={pl} size="sm" /> {platformLabel(pl)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </Field>
          <Field label="Team" hint="Owners of the project; used by alert rules that email “the project's team”.">
            {(p) => (
              <Select value={form.team} onValueChange={(v) => setForm((f) => ({ ...f, team: v }))} disabled={!can}>
                <SelectTrigger id={p.id} className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none">No team</SelectItem>
                  {teams.data?.map((t) => (
                    <SelectItem key={t.id} value={t.id}>
                      #{t.slug}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </Field>
        </div>
      </SettingsCard>
      <SettingsCard
        title="Delete project"
        danger
        description="Deletes the project with all its issues, events, keys, alert rules and source maps. SDKs using its DSN get errors."
      >
        <Button variant="destructive" size="sm" disabled={!can} onClick={() => setDeleting(true)}>
          Delete project…
        </Button>
      </SettingsCard>
      <AlertDialog open={deleting} onOpenChange={setDeleting}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {project}?</AlertDialogTitle>
            <AlertDialogDescription>
              Type <span className="font-mono font-semibold text-foreground">{project}</span> to confirm. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <Input value={confirm} onChange={(e) => setConfirm(e.target.value)} aria-label="Project slug" className="font-mono" />
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              destructive
              disabled={confirm !== project}
              onClick={async () => {
                try {
                  await api.projects.remove(org, project)
                  await qc.invalidateQueries({ queryKey: qk.projects(org) })
                  toast.success('Project deleted')
                  await navigate({ to: '/$org/settings/projects', params: { org } })
                } catch (e) {
                  toast.error(errorMessage(e))
                }
              }}
            >
              Delete forever
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SettingsPage>
  )
}

// ---- keys & SDK --------------------------------------------------------------

function KeyCard({ k, org, project, canManage }: { k: ProjectKey; org: string; project: string; canManage: boolean }) {
  const [showSecret, setShowSecret] = useState(false)
  const update = useApiMutation((body: Partial<ProjectKey>) => api.projects.updateKey(org, project, k.id, body), {
    invalidate: [qk.keys(org, project)],
    success: 'Key updated',
  })
  const del = useApiMutation(() => api.projects.deleteKey(org, project, k.id), { invalidate: [qk.keys(org, project)], success: 'Key deleted' })
  const [confirm, setConfirm] = useState(false)
  return (
    <section className={cn('rounded-lg border bg-card', !k.is_active && 'opacity-70')}>
      <div className="flex flex-wrap items-center gap-2 border-b px-4 py-2.5">
        <h3 className="text-[13px] font-semibold">{k.label}</h3>
        {!k.is_active && <Badge variant="outline">disabled</Badge>}
        {k.rate_limit_per_min && <Badge variant="secondary">{k.rate_limit_per_min.toLocaleString()} events/min</Badge>}
        <span className="text-xs text-muted-foreground">
          created <RelativeTime date={k.created_at} suffix />
        </span>
        {canManage && (
          <div className="ml-auto flex items-center gap-2">
            <label className="flex items-center gap-2 text-xs">
              <Switch
                checked={k.is_active}
                onCheckedChange={(v) => update.mutate({ is_active: v })}
                aria-label={`${k.is_active ? 'Disable' : 'Enable'} key ${k.label}`}
              />{' '}
              Active
            </label>
            <Button variant="ghost" size="icon-xs" aria-label={`Delete key ${k.label}`} onClick={() => setConfirm(true)}>
              <Trash2 />
            </Button>
          </div>
        )}
      </div>
      <div className="grid gap-3 p-4">
        <div>
          <p className="mb-1.5 text-xs font-medium text-muted-foreground">DSN</p>
          <CopyField value={k.dsn} label="Copy DSN" />
        </div>
        <div className="grid gap-3 sm:grid-cols-2">
          <div>
            <p className="mb-1.5 text-xs font-medium text-muted-foreground">Public key</p>
            <CopyField value={k.public_key} label="Copy public key" />
          </div>
          <div>
            <p className="mb-1.5 flex items-center justify-between text-xs font-medium text-muted-foreground">
              Secret key
              <button type="button" className="text-[11px] hover:text-foreground" onClick={() => setShowSecret((s) => !s)}>
                {showSecret ? 'hide' : 'show'}
              </button>
            </p>
            <CopyField value={k.secret_key} label="Copy secret key" mask={!showSecret} />
          </div>
        </div>
        <p className="text-xs text-muted-foreground">The DSN is not a secret: browser apps embed it. Keys only allow sending events to this project.</p>
      </div>
      <AlertDialog open={confirm} onOpenChange={setConfirm}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete key “{k.label}”?</AlertDialogTitle>
            <AlertDialogDescription>
              SDKs using this DSN will get 401 responses and their events are lost. Disable the key instead if you may need it again.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction destructive onClick={() => del.mutate(undefined)}>
              Delete key
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  )
}

export function ProjectKeysPage({ org, project }: { org: string; project: string }) {
  const q = useProject(org, project)
  const keys = useQuery({ queryKey: qk.keys(org, project), queryFn: () => api.projects.keys(org, project) })
  const { can } = useRole('admin')
  const [guide, setGuide] = useState<string | null>(null)
  const create = useApiMutation(() => api.projects.createKey(org, project, { label: `Key ${(keys.data?.length ?? 0) + 1}` }), {
    invalidate: [qk.keys(org, project)],
    success: 'Key created',
  })
  const active = keys.data?.find((k) => k.is_active) ?? keys.data?.[0]
  const current = guide ? SDK_GUIDES.find((g) => g.id === guide)! : guideFor(q.data?.platform)
  const dsn = active?.dsn ?? 'https://<public-key>@<your-rupta-host>/<project-id>'
  return (
    <SettingsPage
      title="Client keys & SDK setup"
      description="SDKs send events with a DSN. Use one key per application if you want to rotate or rate-limit them separately."
      actions={
        can && (
          <Button size="sm" variant="outline" onClick={() => create.mutate(undefined)} disabled={create.isPending}>
            <Plus /> New key
          </Button>
        )
      }
    >
      {keys.isPending ? (
        <CardsSkeleton count={1} />
      ) : keys.isError ? (
        <ErrorState error={keys.error} />
      ) : (
        keys.data.map((k) => <KeyCard key={k.id} k={k} org={org} project={project} canManage={can} />)
      )}
      <SettingsCard title="Set up the SDK" description="rupta works with the official open-source SDKs. Pick your platform:">
        <div className="mb-4 flex flex-wrap gap-1.5" role="tablist" aria-label="Platform">
          {SDK_GUIDES.map((g) => (
            <button
              key={g.id}
              type="button"
              role="tab"
              aria-selected={current.id === g.id}
              onClick={() => setGuide(g.id)}
              className={cn(
                'inline-flex items-center gap-1.5 rounded-md border px-2 py-1 text-xs font-medium hover:bg-accent',
                current.id === g.id && 'border-brand bg-brand-soft text-brand-text',
              )}
            >
              <PlatformIcon platform={g.platforms[0] ?? g.id} size="sm" />
              {g.label}
            </button>
          ))}
        </div>
        <ol className="space-y-4" role="tabpanel" aria-label={`${current.label} setup`}>
          {current.steps(dsn, `${project}@1.0.0`).map((s, i) => (
            <li key={i} className="grid gap-2">
              <p className="flex items-center gap-2 text-[13px] font-medium">
                <span className="flex size-5 items-center justify-center rounded-full bg-muted text-[11px] font-semibold">{i + 1}</span>
                {s.title}
              </p>
              <CodeBlock code={s.code} lang={s.lang} title={s.file} />
            </li>
          ))}
        </ol>
        {current.note && <p className="mt-4 text-xs text-muted-foreground">{current.note}</p>}
        <p className="mt-4 text-xs text-muted-foreground">
          Then trigger a test error. It appears in{' '}
          <Link to="/$org/issues" params={{ org }} className="text-brand-text hover:underline">
            Issues
          </Link>{' '}
          within seconds.
        </p>
      </SettingsCard>
    </SettingsPage>
  )
}

// ---- inbound filters ---------------------------------------------------------

export function ProjectFiltersPage({ org, project }: { org: string; project: string }) {
  const q = useProject(org, project)
  const save = useSaveSettings(org, project)
  const { can } = useRole('admin')
  const [messages, setMessages] = useState<string[]>([])
  const [releases, setReleases] = useState<string[]>([])
  useEffect(() => {
    if (q.data) {
      setMessages(q.data.settings.filters.error_messages)
      setReleases(q.data.settings.filters.releases)
    }
  }, [q.data])
  if (!q.data) return <Loading title="Inbound filters" />
  const f = q.data.settings.filters
  const toggle = (k: 'browser_extensions' | 'localhost' | 'legacy_browsers' | 'web_crawlers', v: boolean) => save.mutate({ filters: { ...f, [k]: v } })
  return (
    <SettingsPage
      title="Inbound filters"
      description="Events dropped here are not stored and do not count against rate limits. Dropped events are counted in the project statistics."
    >
      <SettingsCard>
        <div className="divide-y">
          <ToggleRow title="Browser extensions" description="Errors caused by browser extensions and injected scripts.">
            <Switch
              checked={f.browser_extensions}
              disabled={!can}
              onCheckedChange={(v) => toggle('browser_extensions', v)}
              aria-label="Filter browser extensions"
            />
          </ToggleRow>
          <ToggleRow title="Localhost" description="Events from localhost, 127.0.0.1 and ::1.">
            <Switch checked={f.localhost} disabled={!can} onCheckedChange={(v) => toggle('localhost', v)} aria-label="Filter localhost" />
          </ToggleRow>
          <ToggleRow title="Legacy browsers" description="Very old browser versions (for example Internet Explorer).">
            <Switch checked={f.legacy_browsers} disabled={!can} onCheckedChange={(v) => toggle('legacy_browsers', v)} aria-label="Filter legacy browsers" />
          </ToggleRow>
          <ToggleRow title="Web crawlers" description="Requests from known bots and crawlers, by user agent.">
            <Switch checked={f.web_crawlers} disabled={!can} onCheckedChange={(v) => toggle('web_crawlers', v)} aria-label="Filter web crawlers" />
          </ToggleRow>
        </div>
      </SettingsCard>
      <SettingsCard
        title="Custom filters"
        description="Glob patterns, one per line. * matches any characters. Matching is case-insensitive."
        footer={
          <Button
            size="sm"
            disabled={!can || save.isPending}
            onClick={() =>
              save.mutate({
                filters: { ...f, error_messages: messages.map((s) => s.trim()).filter(Boolean), releases: releases.map((s) => s.trim()).filter(Boolean) },
              })
            }
          >
            Save filters
          </Button>
        }
      >
        <div className="grid gap-4 md:grid-cols-2">
          <Field label="Error messages">
            {(p) => <LinesInput {...p} value={messages} onChange={setMessages} placeholder={'ResizeObserver loop *\n*Script error.*'} />}
          </Field>
          <Field label="Releases">{(p) => <LinesInput {...p} value={releases} onChange={setReleases} placeholder={'*-dev\nweb-app@0.*'} />}</Field>
        </div>
      </SettingsCard>
    </SettingsPage>
  )
}

// ---- data scrubbing ----------------------------------------------------------

export function ProjectScrubbingPage({ org, project }: { org: string; project: string }) {
  const q = useProject(org, project)
  const save = useSaveSettings(org, project)
  const { can } = useRole('admin')
  const [extra, setExtra] = useState<string[]>([])
  const [safe, setSafe] = useState<string[]>([])
  useEffect(() => {
    if (q.data) {
      setExtra(q.data.settings.scrubbing.extra_fields)
      setSafe(q.data.settings.scrubbing.safe_fields)
    }
  }, [q.data])
  if (!q.data) return <Loading title="Data scrubbing" />
  const s = q.data.settings.scrubbing
  return (
    <SettingsPage
      title="Data scrubbing"
      description="The server removes sensitive values before events are stored. SDK-side options like sendDefaultPii decide what is sent in the first place."
    >
      <SettingsCard>
        <div className="divide-y">
          <ToggleRow
            title="Scrub sensitive data"
            description="Replace values of keys like password, secret, token, authorization, cookie, api_key and credit card numbers with [Filtered]."
          >
            <Switch
              checked={s.enabled}
              disabled={!can}
              onCheckedChange={(v) => save.mutate({ scrubbing: { ...s, enabled: v } })}
              aria-label="Scrub sensitive data"
            />
          </ToggleRow>
          <ToggleRow title="Do not store IP addresses" description="Remove the user's IP address and the X-Forwarded-For header from events.">
            <Switch
              checked={s.scrub_ip_addresses}
              disabled={!can}
              onCheckedChange={(v) => save.mutate({ scrubbing: { ...s, scrub_ip_addresses: v } })}
              aria-label="Do not store IP addresses"
            />
          </ToggleRow>
        </div>
      </SettingsCard>
      <SettingsCard
        title="Fields"
        description="Key names, one per line. A key matches if it contains the text (case-insensitive)."
        footer={
          <Button
            size="sm"
            disabled={!can || save.isPending}
            onClick={() =>
              save.mutate({
                scrubbing: { ...s, extra_fields: extra.map((x) => x.trim()).filter(Boolean), safe_fields: safe.map((x) => x.trim()).filter(Boolean) },
              })
            }
          >
            Save fields
          </Button>
        }
      >
        <div className="grid gap-4 md:grid-cols-2">
          <Field label="Additional sensitive fields" hint="Also scrubbed.">
            {(p) => <LinesInput {...p} value={extra} onChange={setExtra} placeholder={'iban\ntax_id\ndate_of_birth'} />}
          </Field>
          <Field label="Safe fields" hint="Never scrubbed, even when they match.">
            {(p) => <LinesInput {...p} value={safe} onChange={setSafe} placeholder={'token_type\npassword_strength'} />}
          </Field>
        </div>
      </SettingsCard>
    </SettingsPage>
  )
}

// ---- security ----------------------------------------------------------------

export function ProjectSecurityPage({ org, project }: { org: string; project: string }) {
  const q = useProject(org, project)
  const keys = useQuery({ queryKey: qk.keys(org, project), queryFn: () => api.projects.keys(org, project) })
  const save = useSaveSettings(org, project)
  const { can } = useRole('admin')
  const [origins, setOrigins] = useState<string[]>([])
  useEffect(() => {
    if (q.data) setOrigins(q.data.settings.allowed_origins)
  }, [q.data])
  if (!q.data) return <Loading title="Security" />
  const key = keys.data?.find((k) => k.is_active)
  const base = key ? new URL(key.dsn) : null
  const reportUri = key && base ? `${base.protocol}//${base.host}/api/${q.data.id}/security/?sentry_key=${key.public_key}` : '…'
  return (
    <SettingsPage title="Security" description="Which browsers may send events, and where to send Content Security Policy reports.">
      <SettingsCard
        title="Allowed origins"
        description="Browser SDK requests must come from one of these origins (Origin or Referer header). * allows any origin; *.example.com allows subdomains. Server SDKs are not affected."
        footer={
          <Button size="sm" disabled={!can || save.isPending} onClick={() => save.mutate({ allowed_origins: origins.map((o) => o.trim()).filter(Boolean) })}>
            Save origins
          </Button>
        }
      >
        <Field label="Origins" hint="One per line. Leave * to accept every origin.">
          {(p) => <LinesInput {...p} value={origins} onChange={setOrigins} placeholder={'https://app.example.com\nhttps://*.example.com'} />}
        </Field>
      </SettingsCard>
      <SettingsCard
        title="Security headers"
        description="Browsers can report Content Security Policy violations to rupta. They are stored as events of this project."
      >
        <p className="mb-1.5 text-xs font-medium text-muted-foreground">Report URI</p>
        <CopyField value={reportUri} label="Copy report URI" />
        <p className="mt-4 mb-1.5 text-xs font-medium text-muted-foreground">Example header</p>
        <CodeBlock lang="bash" code={`Content-Security-Policy-Report-Only: default-src 'self'; report-uri ${reportUri}`} />
      </SettingsCard>
    </SettingsPage>
  )
}

// ---- rate limits & retention -------------------------------------------------

export function ProjectLimitsPage({ org, project }: { org: string; project: string }) {
  const q = useProject(org, project)
  const save = useSaveSettings(org, project)
  const { can } = useRole('admin')
  const [rate, setRate] = useState('')
  const [retention, setRetention] = useState('default')
  useEffect(() => {
    if (q.data) {
      setRate(q.data.settings.rate_limit_per_min ? String(q.data.settings.rate_limit_per_min) : '')
      setRetention(q.data.settings.retention_days ? String(q.data.settings.retention_days) : 'default')
    }
  }, [q.data])
  if (!q.data) return <Loading title="Rate limits & retention" />
  const rateNum = rate.trim() ? Number(rate) : null
  const rateInvalid = rateNum !== null && (!Number.isInteger(rateNum) || rateNum < 1)
  return (
    <SettingsPage title="Rate limits & retention">
      <SettingsCard
        title="Rate limit"
        description="Maximum events per minute for the whole project. Above it, SDKs get HTTP 429 with Retry-After and back off. Keys can have their own lower limit."
        footer={
          <Button size="sm" disabled={!can || rateInvalid || save.isPending} onClick={() => save.mutate({ rate_limit_per_min: rateNum })}>
            Save rate limit
          </Button>
        }
      >
        <Field label="Events per minute" error={rateInvalid ? 'Enter a whole number above 0, or leave empty.' : undefined} hint="Empty means unlimited.">
          {(p) => (
            <Input
              {...p}
              inputMode="numeric"
              value={rate}
              disabled={!can}
              onChange={(e) => setRate(e.target.value)}
              placeholder="Unlimited"
              className="max-w-48"
            />
          )}
        </Field>
      </SettingsCard>
      <SettingsCard
        title="Data retention"
        description="Events and attachments older than this are deleted by a daily job. Issues stay (with their counts) until you delete them."
        footer={
          <Button
            size="sm"
            disabled={!can || save.isPending}
            onClick={() => save.mutate({ retention_days: retention === 'default' ? null : Number(retention) })}
          >
            Save retention
          </Button>
        }
      >
        <Field label="Keep events for">
          {(p) => (
            <Select value={retention} onValueChange={setRetention} disabled={!can}>
              <SelectTrigger id={p.id} className="w-56">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="default">Instance default</SelectItem>
                {[7, 14, 30, 60, 90, 180, 365].map((d) => (
                  <SelectItem key={d} value={String(d)}>
                    {d} days
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        </Field>
      </SettingsCard>
    </SettingsPage>
  )
}

// ---- source maps -------------------------------------------------------------

export function ProjectSourceMapsPage({ org, project }: { org: string; project: string }) {
  const bundles = useQuery({ queryKey: qk.bundles(org, project), queryFn: () => api.projects.bundles(org, project) })
  const keys = useQuery({ queryKey: qk.keys(org, project), queryFn: () => api.projects.keys(org, project) })
  const { can } = useRole('admin')
  const del = useApiMutation((id: string) => api.projects.deleteBundle(org, project, id), { invalidate: [qk.bundles(org, project)], success: 'Bundle deleted' })
  const dsn = keys.data?.[0]?.dsn
  const publicUrl = dsn ? `${new URL(dsn).protocol}//${new URL(dsn).host}` : location.origin
  return (
    <SettingsPage
      title="Source maps"
      description="Upload source maps so minified JavaScript stack traces show original files, functions and lines. Events are symbolicated when they arrive."
    >
      <SettingsCard
        title="Upload with the CLI"
        description="Use the official upload CLI with an organization token that has the “Releases & source maps” scope."
      >
        <ol className="space-y-4">
          <li className="grid gap-2">
            <p className="text-[13px] font-medium">1. Install the CLI and authenticate</p>
            <CodeBlock lang="bash" code={`npm install --save-dev @sentry/cli\nexport SENTRY_AUTH_TOKEN=<organization token>`} />
          </li>
          <li className="grid gap-2">
            <p className="text-[13px] font-medium">2. After building, inject debug ids and upload</p>
            <CodeBlock
              lang="bash"
              code={`npx sentry-cli --url ${publicUrl} sourcemaps inject ./dist\nnpx sentry-cli --url ${publicUrl} sourcemaps upload --org ${org} --project ${project} ./dist`}
            />
          </li>
          <li className="grid gap-2">
            <p className="text-[13px] font-medium">3. Optional: create the release and record a deploy</p>
            <CodeBlock
              lang="bash"
              code={`VERSION=$(git rev-parse --short HEAD)\nnpx sentry-cli --url ${publicUrl} releases new --org ${org} --project ${project} "$VERSION"\nnpx sentry-cli --url ${publicUrl} releases finalize --org ${org} "$VERSION"\nnpx sentry-cli --url ${publicUrl} deploys new --org ${org} --release "$VERSION" -e production`}
            />
          </li>
        </ol>
        <p className="mt-4 text-xs text-muted-foreground">
          Supported commands: sourcemaps inject/upload, releases new/finalize and deploys new. Uploads with debug ids work without a release; legacy uploads
          need the same release as the SDK.
        </p>
      </SettingsCard>
      <section className="overflow-hidden rounded-lg border bg-card">
        <div className="border-b px-4 py-3">
          <h2 className="text-sm font-semibold">Uploaded bundles</h2>
        </div>
        {bundles.isPending ? (
          <ListSkeleton rows={3} />
        ) : bundles.isError && bundles.error instanceof ApiError && bundles.error.isNotFound ? (
          <EmptyState
            compact
            icon={<FileArchive />}
            title="Bundle list not available"
            description="This server does not list uploaded bundles yet. Uploads with the CLI still work and are used for symbolication."
          />
        ) : bundles.isError ? (
          <ErrorState error={bundles.error} />
        ) : bundles.data.length === 0 ? (
          <EmptyState compact icon={<FileArchive />} title="No source maps uploaded" description="Bundles appear here after the first upload." />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Bundle</TableHead>
                <TableHead>Release</TableHead>
                <TableHead className="text-right">Files</TableHead>
                <TableHead className="hidden text-right sm:table-cell">Size</TableHead>
                <TableHead className="hidden md:table-cell">Uploaded</TableHead>
                <TableHead className="w-0" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {bundles.data.map((b) => (
                <TableRow key={b.id}>
                  <TableCell className="font-mono text-xs">{b.checksum.slice(0, 12)}</TableCell>
                  <TableCell className="font-mono text-xs">{b.release ?? <span className="text-muted-foreground">debug ids only</span>}</TableCell>
                  <TableCell className="text-right tabular">{b.file_count}</TableCell>
                  <TableCell className="hidden text-right tabular sm:table-cell">{bytes(b.size)}</TableCell>
                  <TableCell className="hidden text-muted-foreground md:table-cell">
                    <RelativeTime date={b.created_at} suffix />
                  </TableCell>
                  <TableCell>
                    {can && (
                      <Button variant="ghost" size="icon-xs" aria-label={`Delete bundle ${b.checksum.slice(0, 12)}`} onClick={() => del.mutate(b.id)}>
                        <Trash2 />
                      </Button>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </section>
    </SettingsPage>
  )
}
