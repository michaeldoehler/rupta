import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query'
import { BellRing, Globe, Hash, Mail, MessagesSquare, Pencil, Plus, ScrollText, Send, Trash2, Webhook } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { qk, useApiMutation, useMembers } from '@/api/queries'
import type { Channel, ChannelKind } from '@/api/types'
import { UserAvatar } from '@/components/common/avatar'
import { Field, FormError } from '@/components/common/field'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { absolute } from '@/lib/format'
import { useRole } from './org-pages'
import { SettingsCard, SettingsPage } from './settings-layout'
import { CreateTokenDialog, TokenTable } from './tokens'

export function OrgTokensPage({ org }: { org: string }) {
  const tokens = useQuery({ queryKey: qk.orgTokens(org), queryFn: () => api.orgs.tokens(org) })
  const qc = useQueryClient()
  const { can } = useRole('admin')
  const [open, setOpen] = useState(false)
  const del = useApiMutation((id: string) => api.orgs.deleteToken(org, id), { invalidate: [qk.orgTokens(org)], success: 'Token revoked' })
  return (
    <SettingsPage
      title="API tokens"
      description="Organization tokens are not tied to a person, so they keep working when people leave. Use them for CI, the upload CLI and integrations."
      actions={
        can && (
          <Button size="sm" variant="brand" onClick={() => setOpen(true)}>
            <Plus /> Create token
          </Button>
        )
      }
    >
      <section className="overflow-hidden rounded-lg border bg-card">
        {tokens.isError ? (
          <ErrorState error={tokens.error} />
        ) : (
          <TokenTable tokens={tokens.data} loading={tokens.isPending} onDelete={(t) => del.mutate(t.id)} onCreate={() => setOpen(true)} canManage={can} />
        )}
      </section>
      <CreateTokenDialog
        open={open}
        onOpenChange={setOpen}
        onCreate={async (body) => {
          const t = await api.orgs.createToken(org, body)
          void qc.invalidateQueries({ queryKey: qk.orgTokens(org) })
          return t
        }}
      />
    </SettingsPage>
  )
}

const ACTION_GROUPS = [
  ['', 'All actions'],
  ['auth.', 'Sign-ins'],
  ['member.', 'Members'],
  ['team.', 'Teams'],
  ['project', 'Projects & keys'],
  ['alert_rule.', 'Alert rules'],
  ['channel.', 'Channels'],
  ['api_token.', 'API tokens'],
  ['sso.', 'Single sign-on'],
  ['issue.', 'Issues'],
  ['org.', 'Organization'],
] as const

function actionTone(a: string): 'destructive' | 'success' | 'secondary' | 'outline' {
  if (a.endsWith('delete') || a.endsWith('remove') || a.endsWith('failed') || a.endsWith('disable')) return 'destructive'
  if (a.endsWith('create') || a.endsWith('invite')) return 'success'
  if (a.startsWith('auth.')) return 'outline'
  return 'secondary'
}

export function AuditLogPage({ org }: { org: string }) {
  const [action, setAction] = useState('')
  const [actor, setActor] = useState('')
  const members = useMembers(org)
  const filters = { action, actor }
  const q = useInfiniteQuery({
    queryKey: qk.audit(org, filters),
    queryFn: ({ pageParam }) => api.orgs.auditLog(org, { action: action || undefined, actor: actor || undefined, cursor: pageParam ?? undefined, limit: 50 }),
    initialPageParam: null as string | null,
    getNextPageParam: (l) => l.next_cursor,
  })
  const rows = q.data?.pages.flatMap((p) => p.data) ?? []
  return (
    <SettingsPage
      title="Audit log"
      description="Security-relevant changes and sign-ins in this organization. Entries are kept as long as the organization exists."
    >
      <div className="flex flex-wrap gap-2">
        <Select value={action || '__all'} onValueChange={(v) => setAction(v === '__all' ? '' : v)}>
          <SelectTrigger size="sm" className="w-48" aria-label="Filter by action">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {ACTION_GROUPS.map(([v, label]) => (
              <SelectItem key={label} value={v || '__all'}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={actor || '__all'} onValueChange={(v) => setActor(v === '__all' ? '' : v)}>
          <SelectTrigger size="sm" className="w-56" aria-label="Filter by actor">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all">Everyone</SelectItem>
            {members.data?.map((m) => (
              <SelectItem key={m.user.id} value={m.user.id}>
                {m.user.name || m.user.email}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <section className="overflow-hidden rounded-lg border bg-card">
        {q.isPending ? (
          <ListSkeleton rows={8} />
        ) : q.isError ? (
          <ErrorState error={q.error} retry={() => q.refetch()} />
        ) : rows.length === 0 ? (
          <EmptyState compact icon={<ScrollText />} title="No entries" description="Nothing matches these filters." />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Action</TableHead>
                <TableHead>Actor</TableHead>
                <TableHead className="hidden md:table-cell">Details</TableHead>
                <TableHead className="hidden lg:table-cell">IP address</TableHead>
                <TableHead>Time</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((e) => (
                <TableRow key={e.id}>
                  <TableCell>
                    <Badge variant={actionTone(e.action)} className="font-mono">
                      {e.action}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    <span className="flex items-center gap-2">
                      {e.actor.type === 'user' ? <UserAvatar name={e.actor.name} className="size-5" /> : <Badge variant="outline">{e.actor.type}</Badge>}
                      <span className="truncate">{e.actor.name}</span>
                    </span>
                  </TableCell>
                  <TableCell className="hidden max-w-80 truncate font-mono text-xs text-muted-foreground md:table-cell" title={JSON.stringify(e.data)}>
                    {e.target_type && (
                      <span className="text-foreground/80">
                        {e.target_type}:{e.target_id}{' '}
                      </span>
                    )}
                    {Object.entries(e.data)
                      .map(([k, v]) => `${k}=${Array.isArray(v) ? v.join(',') : String(v)}`)
                      .join(' ')}
                  </TableCell>
                  <TableCell className="hidden font-mono text-xs text-muted-foreground lg:table-cell">{e.ip ?? '—'}</TableCell>
                  <TableCell className="whitespace-nowrap text-muted-foreground" title={absolute(e.created_at, { seconds: true })}>
                    <RelativeTime date={e.created_at} suffix />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </section>
      {q.hasNextPage && (
        <Button variant="outline" size="sm" onClick={() => q.fetchNextPage()} disabled={q.isFetchingNextPage}>
          Load older entries
        </Button>
      )}
    </SettingsPage>
  )
}

const CHANNEL_KINDS: {
  kind: ChannelKind
  label: string
  icon: React.ComponentType<{ className?: string }>
  fields: { key: string; label: string; placeholder: string; secret?: boolean; hint?: string }[]
}[] = [
  {
    kind: 'slack',
    label: 'Slack',
    icon: Hash,
    fields: [
      { key: 'webhook_url', label: 'Incoming webhook URL', placeholder: 'https://hooks.slack.com/services/…', secret: true },
      { key: 'channel', label: 'Channel', placeholder: '#alerts' },
    ],
  },
  {
    kind: 'teams',
    label: 'Microsoft Teams',
    icon: MessagesSquare,
    fields: [{ key: 'webhook_url', label: 'Workflow webhook URL', placeholder: 'https://…webhook.office.com/…', secret: true }],
  },
  {
    kind: 'email',
    label: 'Email',
    icon: Mail,
    fields: [{ key: 'recipients', label: 'Recipients', placeholder: 'oncall@example.com, team@example.com', hint: 'Separate addresses with commas.' }],
  },
  {
    kind: 'webhook',
    label: 'Webhook',
    icon: Webhook,
    fields: [
      { key: 'url', label: 'URL', placeholder: 'https://example.com/hooks/rupta' },
      {
        key: 'secret',
        label: 'Signing secret',
        placeholder: 'optional',
        secret: true,
        hint: 'If set, each request is signed with an HMAC-SHA256 of the body using this secret.',
      },
    ],
  },
]

function ChannelDialog({ org, channel, open, onOpenChange }: { org: string; channel: Channel | null; open: boolean; onOpenChange: (o: boolean) => void }) {
  const qc = useQueryClient()
  const [kind, setKind] = useState<ChannelKind>(channel?.kind ?? 'slack')
  const [name, setName] = useState(channel?.name ?? '')
  const [config, setConfig] = useState<Record<string, string>>(channel?.config ?? {})
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const def = CHANNEL_KINDS.find((k) => k.kind === kind)!
  async function submit(e: React.FormEvent) {
    e.preventDefault()
    if (!name.trim()) return setError('Enter a name.')
    const missing = def.fields.find((f) => f.placeholder !== 'optional' && !config[f.key]?.trim())
    if (missing) return setError(`Enter the ${missing.label.toLowerCase()}.`)
    setBusy(true)
    setError(null)
    try {
      if (channel) await api.orgs.updateChannel(org, channel.id, { name, config })
      else await api.orgs.createChannel(org, { kind, name, config })
      await qc.invalidateQueries({ queryKey: qk.channels(org) })
      toast.success(channel ? 'Channel saved' : 'Channel added')
      onOpenChange(false)
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={submit} className="grid gap-4" noValidate>
          <DialogHeader>
            <DialogTitle>{channel ? 'Edit channel' : 'Add notification channel'}</DialogTitle>
            <DialogDescription>Alert rules send notifications to channels.</DialogDescription>
          </DialogHeader>
          <FormError message={error} />
          {!channel && (
            <div className="grid grid-cols-2 gap-2 sm:grid-cols-4" role="radiogroup" aria-label="Channel type">
              {CHANNEL_KINDS.map((k) => (
                <button
                  key={k.kind}
                  type="button"
                  role="radio"
                  aria-checked={kind === k.kind}
                  onClick={() => {
                    setKind(k.kind)
                    setConfig({})
                  }}
                  className={`flex flex-col items-center gap-1.5 rounded-md border px-2 py-2.5 text-xs font-medium hover:bg-accent ${kind === k.kind ? 'border-brand bg-brand-soft text-brand-text' : ''}`}
                >
                  <k.icon className="size-4" /> {k.label}
                </button>
              ))}
            </div>
          )}
          <Field label="Name">
            {(p) => <Input {...p} value={name} onChange={(e) => setName(e.target.value)} placeholder={kind === 'slack' ? '#alerts-prod' : 'On-call'} />}
          </Field>
          {def.fields.map((f) => (
            <Field key={f.key} label={f.label} hint={f.hint} optional={f.placeholder === 'optional'}>
              {(p) => (
                <Input
                  {...p}
                  type={f.secret ? 'password' : 'text'}
                  value={config[f.key] ?? ''}
                  placeholder={f.placeholder === 'optional' ? '' : f.placeholder}
                  onChange={(e) => setConfig((c) => ({ ...c, [f.key]: e.target.value }))}
                  className="font-mono text-[13px]"
                  autoComplete="off"
                />
              )}
            </Field>
          ))}
          <DialogFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy}>
              {channel ? 'Save' : 'Add channel'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function ChannelsPage({ org }: { org: string }) {
  const q = useQuery({ queryKey: qk.channels(org), queryFn: () => api.orgs.channels(org) })
  const { can } = useRole('admin')
  const [editing, setEditing] = useState<{ open: boolean; channel: Channel | null }>({ open: false, channel: null })
  const del = useApiMutation((id: string) => api.orgs.deleteChannel(org, id), { invalidate: [qk.channels(org)], success: 'Channel deleted' })
  const [testing, setTesting] = useState<string | null>(null)
  async function test(c: Channel) {
    setTesting(c.id)
    try {
      const r = await api.orgs.testChannel(org, c.id)
      if (r.ok) toast.success(r.message)
      else toast.error('Test failed', { description: r.message })
    } catch (e) {
      toast.error('Test failed', { description: errorMessage(e) })
    } finally {
      setTesting(null)
    }
  }
  return (
    <SettingsPage
      title="Notification channels"
      description="Where alert rules deliver notifications. Secrets in channel settings are stored encrypted."
      actions={
        can && (
          <Button size="sm" variant="brand" onClick={() => setEditing({ open: true, channel: null })}>
            <Plus /> Add channel
          </Button>
        )
      }
    >
      {q.isPending ? (
        <ListSkeleton rows={4} />
      ) : q.isError ? (
        <ErrorState error={q.error} />
      ) : q.data.length === 0 ? (
        <SettingsCard>
          <EmptyState
            compact
            icon={<BellRing />}
            title="No channels"
            description="Add Slack, Microsoft Teams, email or webhook channels to use them in alert rules."
          />
        </SettingsCard>
      ) : (
        <ul className="divide-y rounded-lg border bg-card">
          {q.data.map((c) => {
            const def = CHANNEL_KINDS.find((k) => k.kind === c.kind)
            const Icon = def?.icon ?? Globe
            const target =
              c.config.channel ??
              c.config.recipients ??
              c.config.url ??
              (c.config.webhook_url ? c.config.webhook_url.replace(/(https:\/\/[^/]+\/).+/, '$1…') : '')
            return (
              <li key={c.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
                <span className="flex size-8 items-center justify-center rounded-md bg-muted text-muted-foreground">
                  <Icon className="size-4" />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 font-medium">
                    {c.name} <Badge variant="outline">{def?.label ?? c.kind}</Badge>
                  </div>
                  <div className="truncate font-mono text-xs text-muted-foreground">{target}</div>
                </div>
                <div className="flex gap-1">
                  <Button variant="outline" size="xs" onClick={() => void test(c)} disabled={testing === c.id}>
                    <Send /> Test
                  </Button>
                  {can && (
                    <>
                      <Button variant="ghost" size="icon-xs" aria-label={`Edit ${c.name}`} onClick={() => setEditing({ open: true, channel: c })}>
                        <Pencil />
                      </Button>
                      <Button variant="ghost" size="icon-xs" aria-label={`Delete ${c.name}`} onClick={() => del.mutate(c.id)}>
                        <Trash2 />
                      </Button>
                    </>
                  )}
                </div>
              </li>
            )
          })}
        </ul>
      )}
      {editing.open && (
        <ChannelDialog
          key={editing.channel?.id ?? 'new'}
          org={org}
          channel={editing.channel}
          open={editing.open}
          onOpenChange={(o) => setEditing((e) => ({ ...e, open: o }))}
        />
      )}
    </SettingsPage>
  )
}
