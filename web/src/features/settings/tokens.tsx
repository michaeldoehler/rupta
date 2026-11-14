import { KeyRound, Plus, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { ApiError, errorMessage } from '@/api/client'
import type { ApiToken, TokenScope } from '@/api/types'
import { CopyField } from '@/components/common/copy-button'
import { Field, FormError } from '@/components/common/field'
import { EmptyState, ListSkeleton } from '@/components/common/states'
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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

export const SCOPES: { scope: TokenScope; label: string; description: string }[] = [
  { scope: 'project:releases', label: 'Releases & source maps', description: 'Create releases, deploys and upload source maps (upload CLI)' },
  { scope: 'project:read', label: 'Read projects', description: 'List projects, keys and settings' },
  { scope: 'project:write', label: 'Write projects', description: 'Create and change projects and keys' },
  { scope: 'event:read', label: 'Read issues & events', description: 'Search issues, read events' },
  { scope: 'event:write', label: 'Write issues', description: 'Resolve, assign and delete issues' },
  { scope: 'org:read', label: 'Read organization', description: 'Organization details and stats' },
  { scope: 'org:write', label: 'Write organization', description: 'Change organization settings' },
  { scope: 'member:read', label: 'Read members', description: 'List members and teams' },
  { scope: 'member:write', label: 'Write members', description: 'Invite and remove members' },
  { scope: 'alerts:write', label: 'Alerts', description: 'Manage alert rules and channels' },
]

const EXPIRY: [string, number | null][] = [
  ['Never', null],
  ['30 days', 30],
  ['90 days', 90],
  ['1 year', 365],
]

export function CreateTokenDialog({
  open,
  onOpenChange,
  onCreate,
  defaultScopes = ['project:releases', 'project:read'],
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  onCreate: (body: { name: string; scopes: TokenScope[]; expires_at: string | null }) => Promise<ApiToken>
  defaultScopes?: TokenScope[]
}) {
  const [name, setName] = useState('')
  const [scopes, setScopes] = useState<TokenScope[]>(defaultScopes)
  const [expiry, setExpiry] = useState('Never')
  const [created, setCreated] = useState<ApiToken | null>(null)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  function reset(o: boolean) {
    onOpenChange(o)
    if (!o) {
      setTimeout(() => {
        setName('')
        setScopes(defaultScopes)
        setExpiry('Never')
        setCreated(null)
        setErrors({})
        setError(null)
      }, 200)
    }
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    const errs: Record<string, string> = {}
    if (!name.trim()) errs.name = 'Enter a name.'
    if (!scopes.length) errs.scopes = 'Choose at least one scope.'
    setErrors(errs)
    if (Object.keys(errs).length) return
    setBusy(true)
    setError(null)
    try {
      const days = EXPIRY.find(([l]) => l === expiry)?.[1] ?? null
      const t = await onCreate({ name: name.trim(), scopes, expires_at: days ? new Date(Date.now() + days * 86_400_000).toISOString() : null })
      setCreated(t)
    } catch (err) {
      if (err instanceof ApiError) setErrors(err.fieldErrors)
      setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={reset}>
      <DialogContent className="sm:max-w-xl">
        {created ? (
          <>
            <DialogHeader>
              <DialogTitle>Token created</DialogTitle>
              <DialogDescription>Copy it now. For security it is stored only as a hash and will not be shown again.</DialogDescription>
            </DialogHeader>
            <CopyField value={created.token ?? ''} label="Copy token" />
            <div className="flex gap-2 rounded-md border border-warning/40 bg-warning-soft px-3 py-2 text-[13px]">
              <TriangleAlert className="mt-0.5 size-4 shrink-0 text-warning" /> Store it in a secret manager or CI secret. Anyone with this token can act with
              its scopes.
            </div>
            <DialogFooter>
              <Button onClick={() => reset(false)}>Done</Button>
            </DialogFooter>
          </>
        ) : (
          <form onSubmit={submit} className="grid gap-4" noValidate>
            <DialogHeader>
              <DialogTitle>Create API token</DialogTitle>
              <DialogDescription>Tokens authenticate scripts, CI and the upload CLI with “Authorization: Bearer”.</DialogDescription>
            </DialogHeader>
            <FormError message={error} />
            <div className="grid gap-4 sm:grid-cols-[1fr_140px]">
              <Field label="Name" error={errors.name}>
                {(p) => <Input {...p} autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. CI source map upload" />}
              </Field>
              <Field label="Expires">
                {(p) => (
                  <Select value={expiry} onValueChange={setExpiry}>
                    <SelectTrigger id={p.id} className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {EXPIRY.map(([l]) => (
                        <SelectItem key={l} value={l}>
                          {l}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              </Field>
            </div>
            <fieldset>
              <legend className="mb-2 text-[13px] font-medium">Scopes</legend>
              <div className="grid max-h-64 gap-1 overflow-y-auto rounded-md border p-1 sm:grid-cols-2">
                {SCOPES.map((s) => (
                  <label key={s.scope} className="flex cursor-pointer items-start gap-2 rounded-sm px-2 py-1.5 hover:bg-accent">
                    <Checkbox
                      className="mt-0.5"
                      checked={scopes.includes(s.scope)}
                      onCheckedChange={(c) => setScopes((prev) => (c ? [...prev, s.scope] : prev.filter((x) => x !== s.scope)))}
                    />
                    <span className="min-w-0">
                      <span className="block text-[13px] font-medium">{s.label}</span>
                      <span className="block text-xs text-muted-foreground">{s.description}</span>
                    </span>
                  </label>
                ))}
              </div>
              {errors.scopes && <p className="mt-1 text-xs text-destructive">{errors.scopes}</p>}
            </fieldset>
            <DialogFooter>
              <Button variant="outline" onClick={() => reset(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={busy}>
                Create token
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  )
}

export function TokenTable({
  tokens,
  loading,
  onDelete,
  onCreate,
  canManage = true,
}: {
  tokens: ApiToken[] | undefined
  loading: boolean
  onDelete: (t: ApiToken) => void
  onCreate: () => void
  canManage?: boolean
}) {
  const [toDelete, setToDelete] = useState<ApiToken | null>(null)
  if (loading) return <ListSkeleton rows={3} />
  if (!tokens?.length)
    return (
      <EmptyState
        compact
        icon={<KeyRound />}
        title="No tokens"
        description="Create a token for scripts, CI pipelines or the upload CLI."
        action={
          canManage ? (
            <Button size="sm" variant="outline" onClick={onCreate}>
              <Plus /> Create token
            </Button>
          ) : undefined
        }
      />
    )
  return (
    <>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead className="hidden md:table-cell">Scopes</TableHead>
            <TableHead className="hidden sm:table-cell">Last used</TableHead>
            <TableHead className="hidden lg:table-cell">Expires</TableHead>
            <TableHead className="w-0" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {tokens.map((t) => (
            <TableRow key={t.id}>
              <TableCell>
                <div className="font-medium">{t.name}</div>
                <div className="font-mono text-xs text-muted-foreground">{t.token_hint}</div>
              </TableCell>
              <TableCell className="hidden md:table-cell">
                <div className="flex flex-wrap gap-1">
                  {t.scopes.map((s) => (
                    <Badge key={s} variant="outline" className="font-mono">
                      {s}
                    </Badge>
                  ))}
                </div>
              </TableCell>
              <TableCell className="hidden text-muted-foreground sm:table-cell">
                {t.last_used_at ? <RelativeTime date={t.last_used_at} suffix /> : 'never'}
              </TableCell>
              <TableCell className="hidden text-muted-foreground lg:table-cell">{t.expires_at ? <RelativeTime date={t.expires_at} /> : 'never'}</TableCell>
              <TableCell>
                {canManage && (
                  <Button variant="ghost" size="xs" className="text-destructive" onClick={() => setToDelete(t)}>
                    Revoke
                  </Button>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
      <AlertDialog open={!!toDelete} onOpenChange={(o) => !o && setToDelete(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Revoke “{toDelete?.name}”?</AlertDialogTitle>
            <AlertDialogDescription>Anything using this token stops working immediately.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction destructive onClick={() => toDelete && onDelete(toDelete)}>
              Revoke
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
