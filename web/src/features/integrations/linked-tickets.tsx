// Tickets linked to an issue (context panel section) and the dialog that
// creates a ticket from the issue or links an existing one.

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ExternalLink, Link2, Plus, RefreshCw, Ticket, Unlink } from 'lucide-react'
import { lazy, Suspense, useEffect, useState } from 'react'
import { toast } from 'sonner'
import { ApiError, errorMessage } from '@/api/client'
import { ik, integrationsApi, type Integration, type IssueLink } from '@/api/integrations'
import { qk } from '@/api/queries'
import { Field, FormError } from '@/components/common/field'
import { RelativeTime } from '@/components/common/time'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Textarea } from '@/components/ui/textarea'
import { Tip } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { kindMeta, TrackerIcon } from './kinds'

const Markdown = lazy(() => import('@/features/issue-detail/markdown'))

/** Opens the ticket dialog of the issue page from anywhere (header menu, preview). */
export function openTicketDialog(mode: 'create' | 'link') {
  window.dispatchEvent(new CustomEvent('rupta:ticket', { detail: { mode } }))
}

const sentence = (t: string) => (/[.!?]$/.test(t) ? t : `${t}.`)

function StatusBadge({ link }: { link: IssueLink }) {
  if (link.status === 'deleted') return <Badge variant="destructive">deleted</Badge>
  return (
    <Badge variant={link.status_category === 'done' ? 'success' : 'palm'} title={`Status category: ${link.status_category}`}>
      {link.status ?? link.status_category}
    </Badge>
  )
}

export function TicketDialog({
  org,
  issueId,
  projectSlug,
  open,
  mode,
  onOpenChange,
}: {
  org: string
  issueId: string
  projectSlug: string
  open: boolean
  mode: 'create' | 'link'
  onOpenChange: (o: boolean) => void
}) {
  const qc = useQueryClient()
  const integrations = useQuery({ queryKey: ik.list(org), queryFn: () => integrationsApi.list(org), enabled: open })
  const defaults = useQuery({ queryKey: ik.projectDefaults(org, projectSlug), queryFn: () => integrationsApi.projectDefaults(org, projectSlug), enabled: open })
  const draft = useQuery({ queryKey: ik.draft(issueId), queryFn: () => integrationsApi.draft(issueId), enabled: open && mode === 'create', staleTime: 60_000 })
  const enabled = (integrations.data ?? []).filter((i) => i.enabled)
  const [tab, setTab] = useState<'create' | 'link'>(mode)
  const [integrationId, setIntegrationId] = useState<string>('')
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [preview, setPreview] = useState(false)
  const [reference, setReference] = useState('')
  const [target, setTarget] = useState('')
  const [error, setError] = useState<string | null>(null)
  useEffect(() => setTab(mode), [mode, open])
  useEffect(() => {
    if (!integrationId && enabled.length) {
      const withDefaults = enabled.find((i) => defaults.data?.some((d) => d.integration.id === i.id))
      setIntegrationId((withDefaults ?? enabled[0]).id)
    }
  }, [enabled, defaults.data, integrationId])
  useEffect(() => {
    if (draft.data) {
      setTitle(draft.data.title)
      setDescription(draft.data.description)
    }
  }, [draft.data])
  const integration: Integration | undefined = enabled.find((i) => i.id === integrationId)
  const projectDefault = defaults.data?.find((d) => d.integration.id === integrationId)
  const m = integration ? kindMeta(integration.kind) : null
  const submit = useMutation({
    mutationFn: () =>
      tab === 'link'
        ? integrationsApi.createLink(issueId, { integration_id: integrationId, mode: 'link', reference: reference.trim() })
        : integrationsApi.createLink(issueId, {
            integration_id: integrationId,
            mode: 'create',
            title: title.trim() || undefined,
            description: description.trim() || undefined,
            ...(target.trim() ? { target: target.trim() } : {}),
          }),
    onSuccess: (l) => {
      toast.success(tab === 'link' ? `Linked ${l.external_key}` : `Created ${l.external_key}`, {
        action: { label: 'Open', onClick: () => window.open(l.url, '_blank', 'noopener') },
      })
      void qc.invalidateQueries({ queryKey: ik.links(issueId) })
      void qc.invalidateQueries({ queryKey: qk.issues.activity(issueId) })
      onOpenChange(false)
    },
    onError: (e) => {
      const ae = e instanceof ApiError ? e : null
      setError(
        ae?.status === 409
          ? 'This ticket is already linked to the issue.'
          : ae?.status === 422 && tab === 'link'
            ? `${sentence(Object.values(ae.fieldErrors)[0] ?? errorMessage(e))} Use a key or URL such as ${m?.referenceExample ?? 'OPS-42'}.`
            : ae?.status === 502
              ? `${m?.label ?? 'The tracker'} refused the request: ${errorMessage(e)}${typeof ae.details.reason === 'string' ? ` (${ae.details.reason})` : ''}`
              : errorMessage(e),
      )
    },
  })
  const needsTarget = tab === 'create' && !projectDefault
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <form
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault()
            setError(null)
            submit.mutate()
          }}
        >
          <DialogHeader>
            <DialogTitle>{tab === 'create' ? 'Create a ticket' : 'Link an existing ticket'}</DialogTitle>
            <DialogDescription>The ticket links back to this issue; statuses sync as set in the project's integration defaults.</DialogDescription>
          </DialogHeader>
          {integrations.isPending ? (
            <Skeleton className="h-40 w-full" />
          ) : enabled.length === 0 ? (
            <p className="rounded-md border bg-muted/40 px-3 py-3 text-[13px]">
              No issue tracker is connected.{' '}
              <Link to="/$org/settings/integrations" params={{ org }} className="text-palm-text hover:text-brand">
                Connect one in the organization settings
              </Link>
              .
            </p>
          ) : (
            <>
              <FormError message={error} />
              <Tabs value={tab} onValueChange={(v) => setTab(v as 'create' | 'link')}>
                <TabsList>
                  <TabsTrigger value="create">
                    <Plus className="size-3.5" /> New ticket
                  </TabsTrigger>
                  <TabsTrigger value="link">
                    <Link2 className="size-3.5" /> Existing ticket
                  </TabsTrigger>
                </TabsList>
                <div className="mt-3 grid gap-4">
                  <Field label="Tracker">
                    {(p) => (
                      <Select value={integrationId} onValueChange={setIntegrationId}>
                        <SelectTrigger id={p.id} className="w-full">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {enabled.map((i) => (
                            <SelectItem key={i.id} value={i.id}>
                              <TrackerIcon kind={i.kind} className="size-5 text-[9px]" /> {i.name}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    )}
                  </Field>
                  <TabsContent value="create" className="mt-0 grid gap-4">
                    {projectDefault ? (
                      <p className="text-xs text-muted-foreground">
                        Goes to <span className="font-medium text-foreground">{projectDefault.target_name ?? projectDefault.target}</span>
                        {projectDefault.issue_type && <> as {projectDefault.issue_type}</>}
                        {projectDefault.labels.length > 0 && <>, labels {projectDefault.labels.join(', ')}</>} (project defaults).
                      </p>
                    ) : (
                      <Field label={m?.targetLabel ?? 'Target'} hint="This project has no defaults for this tracker; choose where the ticket goes.">
                        {(p) => (
                          <Input
                            {...p}
                            value={target}
                            onChange={(e) => setTarget(e.target.value)}
                            placeholder={integration?.kind === 'github' ? 'owner/repo' : integration?.kind === 'linear' ? 'team id' : 'OPS'}
                            className="font-mono text-[13px]"
                            required={needsTarget}
                          />
                        )}
                      </Field>
                    )}
                    <Field label="Title">
                      {(p) => <Input {...p} value={title} onChange={(e) => setTitle(e.target.value)} placeholder={draft.isPending ? 'Loading…' : ''} />}
                    </Field>
                    <div className="grid gap-1.5">
                      <div className="flex items-center justify-between">
                        <span className="text-[13px] leading-none font-medium">Description</span>
                        <Button variant="ghost" size="xs" onClick={() => setPreview((v) => !v)}>
                          {preview ? 'Edit' : 'Preview'}
                        </Button>
                      </div>
                      {preview ? (
                        <div className="max-h-72 overflow-y-auto rounded-md border bg-card px-3 py-2 text-[13px]">
                          <Suspense fallback={<Skeleton className="h-24 w-full" />}>
                            <Markdown text={description} />
                          </Suspense>
                        </div>
                      ) : (
                        <Textarea
                          value={description}
                          onChange={(e) => setDescription(e.target.value)}
                          rows={9}
                          className="font-mono text-[12.5px]"
                          aria-label="Description"
                        />
                      )}
                      <p className="text-xs text-muted-foreground">Markdown. A link back to the issue is added in any case.</p>
                    </div>
                  </TabsContent>
                  <TabsContent value="link" className="mt-0">
                    <Field label="Ticket" hint={m ? `Key or URL, for example ${m.referenceExample}.` : undefined}>
                      {(p) => (
                        <Input
                          {...p}
                          value={reference}
                          onChange={(e) => setReference(e.target.value)}
                          className="font-mono text-[13px]"
                          autoFocus={mode === 'link'}
                        />
                      )}
                    </Field>
                  </TabsContent>
                </div>
              </Tabs>
            </>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={!integration || submit.isPending || (tab === 'link' ? !reference.trim() : needsTarget && !target.trim())}>
              {submit.isPending ? (tab === 'link' ? 'Linking…' : 'Creating…') : tab === 'link' ? 'Link ticket' : 'Create ticket'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** Context panel section: linked tickets with status, refresh and unlink. */
export function LinkedTickets({ org, issueId, projectSlug }: { org: string; issueId: string; projectSlug: string }) {
  const qc = useQueryClient()
  const q = useQuery({ queryKey: ik.links(issueId), queryFn: () => integrationsApi.links(issueId), staleTime: 30_000 })
  const [dialog, setDialog] = useState<{ open: boolean; mode: 'create' | 'link' }>({ open: false, mode: 'create' })
  useEffect(() => {
    const on = (e: Event) => setDialog({ open: true, mode: ((e as CustomEvent).detail?.mode as 'create' | 'link') ?? 'create' })
    window.addEventListener('rupta:ticket', on)
    return () => window.removeEventListener('rupta:ticket', on)
  }, [])
  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: ik.links(issueId) })
    void qc.invalidateQueries({ queryKey: qk.issues.activity(issueId) })
  }
  const refresh = useMutation({
    mutationFn: (id: string) => integrationsApi.refreshLink(issueId, id),
    onSettled: invalidate,
    onError: (e) => toast.error('Could not refresh', { description: errorMessage(e) }),
  })
  const unlink = useMutation({
    mutationFn: (id: string) => integrationsApi.unlink(issueId, id),
    onSuccess: () => toast.success('Ticket unlinked (it stays in the tracker)'),
    onSettled: invalidate,
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <div data-testid="linked-tickets">
      {q.isPending ? (
        <Skeleton className="h-10 w-full" />
      ) : q.isError ? (
        <p className="text-xs text-destructive">{errorMessage(q.error)}</p>
      ) : q.data.length === 0 ? (
        <p className="text-xs text-muted-foreground">No tickets yet.</p>
      ) : (
        <ul className="space-y-1.5">
          {q.data.map((l) => (
            <li key={l.id} className={cn('group rounded-md border bg-background/40 px-2.5 py-1.5', l.status === 'deleted' && 'opacity-70')}>
              <div className="flex items-center gap-2">
                <TrackerIcon kind={l.integration.kind} className="size-5 text-[9px]" />
                <a
                  href={l.url}
                  target="_blank"
                  rel="noreferrer noopener"
                  className="font-mono text-[12.5px] font-medium hover:text-brand"
                  title={`Open in ${l.integration.name}`}
                >
                  {l.external_key}
                </a>
                <StatusBadge link={l} />
                <span className="ml-auto flex opacity-60 group-hover:opacity-100 focus-within:opacity-100">
                  <Tip content="Read the ticket again">
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      aria-label={`Refresh ${l.external_key}`}
                      onClick={() => refresh.mutate(l.id)}
                      disabled={refresh.isPending && refresh.variables === l.id}
                    >
                      <RefreshCw className={refresh.isPending && refresh.variables === l.id ? 'animate-spin' : undefined} />
                    </Button>
                  </Tip>
                  <Tip content="Unlink (the ticket stays)">
                    <Button variant="ghost" size="icon-xs" aria-label={`Unlink ${l.external_key}`} onClick={() => unlink.mutate(l.id)}>
                      <Unlink />
                    </Button>
                  </Tip>
                  <Button asChild variant="ghost" size="icon-xs">
                    <a href={l.url} target="_blank" rel="noreferrer noopener" aria-label={`Open ${l.external_key} in ${l.integration.name}`}>
                      <ExternalLink />
                    </a>
                  </Button>
                </span>
              </div>
              {l.title && (
                <p className="mt-0.5 truncate text-[12.5px]" title={l.title}>
                  {l.title}
                </p>
              )}
              <p className="text-[11px] text-muted-foreground">
                {l.assignee ? `${l.assignee} · ` : ''}
                {l.synced_at ? (
                  <>
                    synced <RelativeTime date={l.synced_at} suffix plain />
                  </>
                ) : (
                  'not synced yet'
                )}
              </p>
            </li>
          ))}
        </ul>
      )}
      <div className="mt-2 flex gap-2">
        <Button variant="outline" size="xs" onClick={() => setDialog({ open: true, mode: 'create' })}>
          <Ticket /> Create ticket
        </Button>
        <Button variant="ghost" size="xs" onClick={() => setDialog({ open: true, mode: 'link' })}>
          <Link2 /> Link existing
        </Button>
      </div>
      {dialog.open && (
        <TicketDialog
          org={org}
          issueId={issueId}
          projectSlug={projectSlug}
          open={dialog.open}
          mode={dialog.mode}
          onOpenChange={(o) => setDialog((d) => ({ ...d, open: o }))}
        />
      )}
    </div>
  )
}
