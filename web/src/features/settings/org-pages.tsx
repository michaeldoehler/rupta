import { useQuery, useQueryClient } from '@tanstack/react-query'
import { getRouteApi, useNavigate } from '@tanstack/react-router'
import { MailPlus, Plus, Search, UserMinus, Users, X } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { ApiError, errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { qk, useApiMutation, useMembers, useSession, useTeams } from '@/api/queries'
import type { Role } from '@/api/types'
import { UserAvatar } from '@/components/common/avatar'
import { CopyButton, CopyField } from '@/components/common/copy-button'
import { Field, FormError } from '@/components/common/field'
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
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { SettingsCard, SettingsPage } from './settings-layout'

export const ROLES: { role: Role; description: string }[] = [
  { role: 'owner', description: 'Everything, including SSO and deleting the organization' },
  { role: 'admin', description: 'Projects, teams, keys, alerts and members' },
  { role: 'member', description: 'Works on issues: resolve, assign, comment' },
  { role: 'viewer', description: 'Read-only access' },
]

const orgRoute = getRouteApi('/_auth/$org')

/** The current user's role in this org, and whether it is at least `min`. */
export function useRole(min: Role = 'admin') {
  const { membership } = orgRoute.useRouteContext()
  const rank: Record<Role, number> = { viewer: 0, member: 1, admin: 2, owner: 3 }
  return { role: membership.role, can: rank[membership.role] >= rank[min] }
}

export function OrgGeneralPage({ org }: { org: string }) {
  const q = useQuery({ queryKey: qk.org(org), queryFn: () => api.orgs.get(org) })
  const qc = useQueryClient()
  const navigate = useNavigate()
  const { can } = useRole('admin')
  const owner = useRole('owner').can
  const [name, setName] = useState('')
  const [slug, setSlug] = useState('')
  const [confirm, setConfirm] = useState('')
  const [deleting, setDeleting] = useState(false)
  useEffect(() => {
    if (q.data) {
      setName(q.data.name)
      setSlug(q.data.slug)
    }
  }, [q.data])
  const save = useApiMutation(() => api.orgs.update(org, { name, slug }), {
    invalidate: [qk.org(org), qk.session],
    success: 'Organization saved',
    onSuccess: (o) => {
      if (o.slug !== org) void navigate({ to: '/$org/settings', params: { org: o.slug } })
    },
  })
  if (q.isPending)
    return (
      <SettingsPage title="General">
        <CardsSkeleton count={2} />
      </SettingsPage>
    )
  if (q.isError) return <ErrorState error={q.error} />
  return (
    <SettingsPage title="General" description="Name and address of the organization.">
      <SettingsCard
        title="Organization"
        footer={
          <Button size="sm" disabled={!can || save.isPending || (name === q.data.name && slug === q.data.slug)} onClick={() => save.mutate(undefined)}>
            Save
          </Button>
        }
      >
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Name">{(p) => <Input {...p} value={name} disabled={!can} onChange={(e) => setName(e.target.value)} />}</Field>
          <Field label="Slug" hint={`Used in URLs: /${slug}/issues. Changing it breaks old links.`}>
            {(p) => (
              <Input
                {...p}
                value={slug}
                disabled={!can}
                onChange={(e) => setSlug(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, '-'))}
                className="font-mono"
              />
            )}
          </Field>
        </div>
      </SettingsCard>
      <SettingsCard
        title="Delete organization"
        danger
        description="Deletes all projects, issues, events, releases and settings of this organization. This cannot be undone."
      >
        <Button variant="destructive" size="sm" disabled={!owner} onClick={() => setDeleting(true)}>
          Delete organization…
        </Button>
        {!owner && <p className="mt-2 text-xs text-muted-foreground">Only owners can delete the organization.</p>}
      </SettingsCard>
      <AlertDialog open={deleting} onOpenChange={setDeleting}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {q.data.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              Type <span className="font-mono font-semibold text-foreground">{q.data.slug}</span> to confirm.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <Input value={confirm} onChange={(e) => setConfirm(e.target.value)} aria-label="Organization slug" className="font-mono" />
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              destructive
              disabled={confirm !== q.data.slug}
              onClick={async () => {
                try {
                  await api.orgs.remove(org)
                  await qc.invalidateQueries({ queryKey: qk.session })
                  toast.success('Organization deleted')
                  await navigate({ to: '/' })
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

function RoleSelect({ value, onChange, disabled, id }: { value: Role; onChange: (r: Role) => void; disabled?: boolean; id?: string }) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v as Role)} disabled={disabled}>
      <SelectTrigger id={id} size="sm" className="w-28 capitalize" aria-label="Role">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {ROLES.map((r) => (
          <SelectItem key={r.role} value={r.role} className="capitalize">
            {r.role}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

export function MembersPage({ org }: { org: string }) {
  const members = useMembers(org)
  const invites = useQuery({ queryKey: qk.invites(org), queryFn: () => api.orgs.invites(org) })
  const { data: session } = useSession()
  const { can } = useRole('admin')
  const [q, setQ] = useState('')
  const [inviteOpen, setInviteOpen] = useState(false)
  const [removing, setRemoving] = useState<{ id: string; name: string } | null>(null)
  const changeRole = useApiMutation((v: { userId: string; role: Role }) => api.orgs.updateMember(org, v.userId, { role: v.role }), {
    invalidate: [qk.members(org)],
    success: 'Role updated',
  })
  const remove = useApiMutation((userId: string) => api.orgs.removeMember(org, userId), { invalidate: [qk.members(org)], success: 'Member removed' })
  const revoke = useApiMutation((id: string) => api.orgs.revokeInvite(org, id), { invalidate: [qk.invites(org)], success: 'Invitation revoked' })
  const filtered = useMemo(() => {
    const n = q.toLowerCase()
    return (members.data ?? []).filter((m) => !n || m.user.name.toLowerCase().includes(n) || m.user.email.toLowerCase().includes(n))
  }, [members.data, q])

  return (
    <SettingsPage
      title="Members"
      description="People with access to this organization and its projects."
      actions={
        can && (
          <Button size="sm" variant="brand" onClick={() => setInviteOpen(true)}>
            <MailPlus /> Invite members
          </Button>
        )
      }
    >
      {invites.data && invites.data.length > 0 && (
        <SettingsCard title={`Pending invitations (${invites.data.length})`}>
          <ul className="-my-1 divide-y">
            {invites.data.map((i) => (
              <li key={i.id} className="flex flex-wrap items-center gap-3 py-2 text-[13px]">
                <UserAvatar name={i.email} />
                <span className="min-w-0 flex-1 truncate">{i.email}</span>
                <Badge variant="outline" className="capitalize">
                  {i.role}
                </Badge>
                <span className="text-xs text-muted-foreground">
                  expires <RelativeTime date={i.expires_at} />
                </span>
                {i.accept_url && <CopyButton value={i.accept_url} label="Copy invitation link" toastLabel="Invitation link copied" />}
                {can && (
                  <Button variant="ghost" size="xs" onClick={() => revoke.mutate(i.id)}>
                    Revoke
                  </Button>
                )}
              </li>
            ))}
          </ul>
        </SettingsCard>
      )}
      <section className="rounded-lg border bg-card">
        <div className="flex items-center gap-2 border-b px-3 py-2">
          <Search className="size-4 text-muted-foreground" />
          <Input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Filter members"
            className="h-7 border-0 px-0 shadow-none focus-visible:ring-0"
            aria-label="Filter members"
          />
          <span className="shrink-0 text-xs text-muted-foreground tabular">{members.data?.length ?? 0} members</span>
        </div>
        {members.isPending ? (
          <ListSkeleton rows={6} />
        ) : members.isError ? (
          <ErrorState error={members.error} />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Member</TableHead>
                <TableHead className="hidden md:table-cell">Teams</TableHead>
                <TableHead>Role</TableHead>
                <TableHead className="hidden sm:table-cell">Joined</TableHead>
                <TableHead className="w-0" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {filtered.map((m) => {
                const self = m.user.id === session?.user.id
                return (
                  <TableRow key={m.user.id}>
                    <TableCell>
                      <div className="flex items-center gap-2.5">
                        <UserAvatar name={m.user.name || m.user.email} src={m.user.avatar_url} className="size-7" />
                        <div className="min-w-0">
                          <div className="flex items-center gap-1.5 font-medium">
                            {m.user.name || m.user.email}
                            {self && <span className="text-xs font-normal text-muted-foreground">(you)</span>}
                            {m.sso && <Badge variant="outline">SSO</Badge>}
                          </div>
                          <div className="truncate text-xs text-muted-foreground">{m.user.email}</div>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell className="hidden md:table-cell">
                      <div className="flex flex-wrap gap-1">
                        {m.teams.map((t) => (
                          <Badge key={t} variant="secondary">
                            #{t}
                          </Badge>
                        ))}
                      </div>
                    </TableCell>
                    <TableCell>
                      <RoleSelect value={m.role} disabled={!can || self} onChange={(role) => changeRole.mutate({ userId: m.user.id, role })} />
                    </TableCell>
                    <TableCell className="hidden text-muted-foreground sm:table-cell">
                      <RelativeTime date={m.created_at} suffix />
                    </TableCell>
                    <TableCell>
                      {can && !self && (
                        <Button
                          variant="ghost"
                          size="icon-xs"
                          aria-label={`Remove ${m.user.name}`}
                          onClick={() => setRemoving({ id: m.user.id, name: m.user.name || m.user.email })}
                        >
                          <UserMinus />
                        </Button>
                      )}
                    </TableCell>
                  </TableRow>
                )
              })}
            </TableBody>
          </Table>
        )}
      </section>
      <SettingsCard title="Roles">
        <dl className="grid gap-2 sm:grid-cols-2">
          {ROLES.map((r) => (
            <div key={r.role} className="text-[13px]">
              <dt className="font-medium capitalize">{r.role}</dt>
              <dd className="text-muted-foreground">{r.description}</dd>
            </div>
          ))}
        </dl>
      </SettingsCard>
      <InviteDialog org={org} open={inviteOpen} onOpenChange={setInviteOpen} />
      <AlertDialog open={!!removing} onOpenChange={(o) => !o && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove {removing?.name}?</AlertDialogTitle>
            <AlertDialogDescription>They lose access to this organization immediately. Their comments and activity stay.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction destructive onClick={() => removing && remove.mutate(removing.id)}>
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SettingsPage>
  )
}

function InviteDialog({ org, open, onOpenChange }: { org: string; open: boolean; onOpenChange: (o: boolean) => void }) {
  const qc = useQueryClient()
  const [emails, setEmails] = useState('')
  const [role, setRole] = useState<Role>('member')
  const [shareLinks, setShareLinks] = useState<string[]>([])
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  async function submit(e: React.FormEvent) {
    e.preventDefault()
    const list = emails
      .split(/[\s,;]+/)
      .map((s) => s.trim())
      .filter(Boolean)
    if (!list.length) {
      setError('Enter at least one email address.')
      return
    }
    setBusy(true)
    setError(null)
    const failed: string[] = []
    const links: string[] = []
    for (const email of list) {
      try {
        const inv = await api.orgs.invite(org, { email, role })
        if (inv.email_sent === false && inv.accept_url) links.push(`${email}: ${inv.accept_url}`)
      } catch (err) {
        failed.push(`${email}: ${errorMessage(err)}`)
      }
    }
    setBusy(false)
    void qc.invalidateQueries({ queryKey: qk.invites(org) })
    if (links.length) setShareLinks(links)
    if (failed.length) setError(failed.join('\n'))
    else if (links.length) toast.success('Invitations created. Share the links below.')
    else {
      toast.success(list.length === 1 ? `Invitation sent to ${list[0]}` : `${list.length} invitations sent`)
      setEmails('')
      onOpenChange(false)
    }
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={submit} className="grid gap-4" noValidate>
          <DialogHeader>
            <DialogTitle>Invite members</DialogTitle>
            <DialogDescription>
              Each person gets an email with a link that is valid for 7 days. With SSO, they can also sign in through your identity provider.
            </DialogDescription>
          </DialogHeader>
          <FormError message={error} />
          <Field label="Email addresses" hint="Separate several with commas or spaces.">
            {(p) => <Input {...p} autoFocus value={emails} onChange={(e) => setEmails(e.target.value)} placeholder="ada@example.com, grace@example.com" />}
          </Field>
          <div className="grid gap-4">
            <Field label="Role">
              {(p) => (
                <Select value={role} onValueChange={(v) => setRole(v as Role)}>
                  <SelectTrigger id={p.id} className="w-full capitalize">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {ROLES.filter((r) => r.role !== 'owner').map((r) => (
                      <SelectItem key={r.role} value={r.role} className="capitalize">
                        {r.role}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
            </Field>
          </div>
          {shareLinks.length > 0 && (
            <div className="space-y-2 rounded-md border border-warning/40 bg-warning-soft p-3 text-[13px]">
              <p>No email could be sent (SMTP is not configured). Send these links yourself; each works once and expires in 7 days.</p>
              {shareLinks.map((l) => {
                const url = l.slice(l.indexOf(': ') + 2)
                return (
                  <div key={l}>
                    <p className="text-xs text-muted-foreground">{l.slice(0, l.indexOf(': '))}</p>
                    <CopyField value={url} label="Copy invitation link" />
                  </div>
                )
              })}
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              {shareLinks.length ? 'Done' : 'Cancel'}
            </Button>
            <Button type="submit" disabled={busy}>
              Send invitations
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function TeamsPage({ org }: { org: string }) {
  const teams = useTeams(org)
  const members = useMembers(org)
  const { can } = useRole('admin')
  const qc = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [name, setName] = useState('')
  const [nameError, setNameError] = useState<string | undefined>()
  const add = useApiMutation((v: { team: string; userId: string }) => api.orgs.addTeamMember(org, v.team, v.userId), {
    invalidate: [qk.teams(org), qk.members(org)],
  })
  const removeMember = useApiMutation((v: { team: string; userId: string }) => api.orgs.removeTeamMember(org, v.team, v.userId), {
    invalidate: [qk.teams(org), qk.members(org)],
  })
  const del = useApiMutation((team: string) => api.orgs.deleteTeam(org, team), { invalidate: [qk.teams(org)], success: 'Team deleted' })

  async function create(e: React.FormEvent) {
    e.preventDefault()
    if (!name.trim()) return setNameError('Enter a name.')
    try {
      await api.orgs.createTeam(org, { name: name.trim() })
      await qc.invalidateQueries({ queryKey: qk.teams(org) })
      toast.success('Team created')
      setName('')
      setCreateOpen(false)
    } catch (err) {
      setNameError(err instanceof ApiError ? (err.fieldErrors.name ?? err.message) : errorMessage(err))
    }
  }

  return (
    <SettingsPage
      title="Teams"
      description="Group members to assign issues, own projects and route alerts."
      actions={
        can && (
          <Button size="sm" variant="brand" onClick={() => setCreateOpen(true)}>
            <Plus /> New team
          </Button>
        )
      }
    >
      {teams.isPending ? (
        <CardsSkeleton count={3} />
      ) : teams.isError ? (
        <ErrorState error={teams.error} />
      ) : teams.data.length === 0 ? (
        <EmptyState icon={<Users />} title="No teams yet" description="Create teams like “frontend” or “payments” to assign issues to a group." />
      ) : (
        <div className="grid gap-3 md:grid-cols-2">
          {teams.data.map((t) => {
            const notIn = (members.data ?? []).filter((m) => !t.members.some((u) => u.id === m.user.id))
            return (
              <section key={t.id} className="rounded-lg border bg-card">
                <div className="flex items-center gap-2 border-b px-3 py-2.5">
                  <UserAvatar name={t.name} team className="size-7" />
                  <div className="min-w-0 flex-1">
                    <h2 className="text-[13px] font-semibold">#{t.slug}</h2>
                    <p className="text-xs text-muted-foreground">
                      {t.members.length} members · {t.projects.length ? t.projects.map((p) => p.slug).join(', ') : 'no projects'}
                    </p>
                  </div>
                  {can && (
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button variant="outline" size="xs">
                          <Plus /> Add
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end" className="max-h-72 w-60">
                        {notIn.length === 0 && <p className="px-2 py-2 text-xs text-muted-foreground">Everyone is already in this team.</p>}
                        {notIn.map((m) => (
                          <DropdownMenuItem key={m.user.id} onSelect={() => add.mutate({ team: t.slug, userId: m.user.id })}>
                            <UserAvatar name={m.user.name} className="size-5" /> {m.user.name}
                          </DropdownMenuItem>
                        ))}
                      </DropdownMenuContent>
                    </DropdownMenu>
                  )}
                  {can && (
                    <Button variant="ghost" size="icon-xs" aria-label={`Delete team ${t.slug}`} onClick={() => del.mutate(t.slug)}>
                      <X />
                    </Button>
                  )}
                </div>
                <ul className="divide-y">
                  {t.members.map((u) => (
                    <li key={u.id} className="group flex items-center gap-2.5 px-3 py-1.5 text-[13px]">
                      <UserAvatar name={u.name} className="size-5" />
                      <span className="min-w-0 flex-1 truncate">{u.name}</span>
                      <span className="hidden truncate text-xs text-muted-foreground sm:inline">{u.email}</span>
                      {can && (
                        <Button
                          variant="ghost"
                          size="icon-xs"
                          className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                          aria-label={`Remove ${u.name} from ${t.slug}`}
                          onClick={() => removeMember.mutate({ team: t.slug, userId: u.id })}
                        >
                          <X />
                        </Button>
                      )}
                    </li>
                  ))}
                  {t.members.length === 0 && <li className="px-3 py-3 text-xs text-muted-foreground">No members.</li>}
                </ul>
              </section>
            )
          })}
        </div>
      )}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <form onSubmit={create} className="grid gap-4" noValidate>
            <DialogHeader>
              <DialogTitle>New team</DialogTitle>
            </DialogHeader>
            <Field label="Name" error={nameError} hint={name ? `Slug: #${name.toLowerCase().replace(/[^a-z0-9]+/g, '-')}` : undefined}>
              {(p) => <Input {...p} autoFocus value={name} onChange={(e) => (setName(e.target.value), setNameError(undefined))} placeholder="Payments" />}
            </Field>
            <DialogFooter>
              <Button variant="outline" onClick={() => setCreateOpen(false)}>
                Cancel
              </Button>
              <Button type="submit">Create team</Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </SettingsPage>
  )
}
