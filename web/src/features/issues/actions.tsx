import { Check, ChevronDown, CircleCheck, CircleSlash, RotateCcw, Search, UserMinus, UserRound } from 'lucide-react'
import { useMemo, useState } from 'react'
import type { Assignee, IssueUpdate } from '@/api/types'
import { useMembers, useSession, useTeams } from '@/api/queries'
import { UserAvatar } from '@/components/common/avatar'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Tip } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

export type ActionHandler = (update: IssueUpdate, message: string, opts?: { assignee?: Assignee | null; undo?: IssueUpdate }) => void

export const IGNORE_DURATIONS: [string, number][] = [
  ['30 minutes', 30],
  ['2 hours', 120],
  ['6 hours', 360],
  ['24 hours', 1440],
  ['1 week', 10080],
]
export const IGNORE_COUNTS = [10, 100, 1000, 10000]

export function ResolveButton({
  resolved,
  onAction,
  size = 'sm',
  count = 1,
  disabled,
  currentRelease,
}: {
  resolved: boolean
  onAction: ActionHandler
  size?: 'sm' | 'xs'
  count?: number
  disabled?: boolean
  currentRelease?: string | null
}) {
  const noun = count === 1 ? 'Issue' : `${count} issues`
  if (resolved)
    return (
      <Button variant="outline" size={size} disabled={disabled} onClick={() => onAction({ status: 'unresolved' }, `${noun} marked unresolved`)}>
        <RotateCcw /> Unresolve
      </Button>
    )
  return (
    <div className="inline-flex">
      <Tip content="Resolve (e)">
        <Button
          variant="outline"
          size={size}
          className="rounded-r-none border-r-0"
          disabled={disabled}
          onClick={() => onAction({ status: 'resolved' }, `${noun} resolved`, { undo: { status: 'unresolved' } })}
        >
          <CircleCheck className="text-success" /> Resolve
        </Button>
      </Tip>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="outline"
            size={size === 'sm' ? 'icon-sm' : 'icon-xs'}
            className="rounded-l-none"
            disabled={disabled}
            aria-label="More resolve options"
          >
            <ChevronDown />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-64">
          <DropdownMenuLabel>Resolve</DropdownMenuLabel>
          <DropdownMenuItem onSelect={() => onAction({ status: 'resolved' }, `${noun} resolved`, { undo: { status: 'unresolved' } })}>
            Resolved now <DropdownMenuShortcut>E</DropdownMenuShortcut>
          </DropdownMenuItem>
          <DropdownMenuItem
            onSelect={() =>
              onAction({ status: 'resolved', resolved_in_next_release: true }, `${noun} will be resolved in the next release`, {
                undo: { status: 'unresolved' },
              })
            }
          >
            In the next release
          </DropdownMenuItem>
          {currentRelease && (
            <DropdownMenuItem
              onSelect={() =>
                onAction({ status: 'resolved', resolved_in_release: currentRelease }, `${noun} resolved in ${currentRelease}`, {
                  undo: { status: 'unresolved' },
                })
              }
            >
              <span className="truncate">
                In the current release <span className="font-mono text-xs text-muted-foreground">{currentRelease}</span>
              </span>
            </DropdownMenuItem>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}

export function IgnoreButton({
  ignored,
  onAction,
  size = 'sm',
  count = 1,
  disabled,
}: {
  ignored: boolean
  onAction: ActionHandler
  size?: 'sm' | 'xs'
  count?: number
  disabled?: boolean
}) {
  const noun = count === 1 ? 'Issue' : `${count} issues`
  if (ignored)
    return (
      <Button variant="outline" size={size} disabled={disabled} onClick={() => onAction({ status: 'unresolved' }, `${noun} restored`)}>
        <RotateCcw /> Unignore
      </Button>
    )
  const ignore = (u: IssueUpdate, what: string) => onAction({ status: 'ignored', ...u }, `${noun} ignored ${what}`, { undo: { status: 'unresolved' } })
  return (
    <div className="inline-flex">
      <Tip content="Ignore until it escalates (i)">
        <Button
          variant="outline"
          size={size}
          className="rounded-r-none border-r-0"
          disabled={disabled}
          onClick={() => ignore({ substatus: 'archived_until_escalating' }, 'until it escalates')}
        >
          <CircleSlash /> Ignore
        </Button>
      </Tip>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="outline"
            size={size === 'sm' ? 'icon-sm' : 'icon-xs'}
            className="rounded-l-none"
            disabled={disabled}
            aria-label="More ignore options"
          >
            <ChevronDown />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-60">
          <DropdownMenuLabel>Ignore</DropdownMenuLabel>
          <DropdownMenuItem onSelect={() => ignore({ substatus: 'archived_until_escalating' }, 'until it escalates')}>
            Until it escalates <DropdownMenuShortcut>I</DropdownMenuShortcut>
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => ignore({ substatus: 'archived_forever' }, 'forever')}>Forever</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>For a while…</DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              {IGNORE_DURATIONS.map(([label, min]) => (
                <DropdownMenuItem key={min} onSelect={() => ignore({ ignore_duration: min }, `for ${label}`)}>
                  {label}
                </DropdownMenuItem>
              ))}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>Until it happens again…</DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              {IGNORE_COUNTS.map((n) => (
                <DropdownMenuItem key={n} onSelect={() => ignore({ ignore_count: n }, `until ${n.toLocaleString()} more events`)}>
                  {n.toLocaleString()} more times
                </DropdownMenuItem>
              ))}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>Until it affects…</DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              {IGNORE_COUNTS.slice(0, 3).map((n) => (
                <DropdownMenuItem key={n} onSelect={() => ignore({ ignore_user_count: n }, `until ${n.toLocaleString()} more users are affected`)}>
                  {n.toLocaleString()} more users
                </DropdownMenuItem>
              ))}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}

/** Popover to pick a user or team; `null` unassigns. */
export function AssigneePicker({
  org,
  value,
  onChange,
  trigger,
  align = 'end',
}: {
  org: string
  value: Assignee | null
  onChange: (a: Assignee | null) => void
  trigger: React.ReactNode
  align?: 'start' | 'end'
}) {
  const [open, setOpen] = useState(false)
  const [q, setQ] = useState('')
  const { data: session } = useSession()
  const members = useMembers(org)
  const teams = useTeams(org)
  const me = session?.user
  const options = useMemo(() => {
    const users: Assignee[] = (members.data ?? [])
      .filter((m) => m.role !== 'viewer')
      .map((m) => ({ type: 'user', id: m.user.id, name: m.user.name || m.user.email, email: m.user.email }))
    const ts: Assignee[] = (teams.data ?? []).map((t) => ({ type: 'team', id: t.id, name: `#${t.slug}` }))
    const needle = q.toLowerCase()
    const match = (a: Assignee) => !needle || a.name.toLowerCase().includes(needle) || (a.email ?? '').toLowerCase().includes(needle)
    return { users: users.filter(match), teams: ts.filter(match) }
  }, [members.data, teams.data, q])

  const pick = (a: Assignee | null) => {
    onChange(a)
    setOpen(false)
    setQ('')
  }
  const isSel = (a: Assignee) => value?.type === a.type && value.id === a.id

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{trigger}</PopoverTrigger>
      <PopoverContent align={align} className="w-64 p-0" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center gap-2 border-b px-2.5">
          <Search className="size-3.5 text-muted-foreground" />
          <Input
            autoFocus
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Assign to…"
            className="h-9 border-0 bg-transparent px-0 shadow-none focus-visible:ring-0"
            aria-label="Filter people and teams"
          />
        </div>
        <div className="max-h-72 overflow-y-auto p-1">
          {me && !q && (
            <button
              type="button"
              className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-[13px] hover:bg-accent"
              onClick={() => pick({ type: 'user', id: me.id, name: me.name || me.email, email: me.email })}
            >
              <UserRound className="size-4 text-muted-foreground" /> Assign to me
            </button>
          )}
          {value && !q && (
            <button type="button" className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-[13px] hover:bg-accent" onClick={() => pick(null)}>
              <UserMinus className="size-4 text-muted-foreground" /> Unassign
            </button>
          )}
          {options.teams.length > 0 && <p className="px-2 pt-2 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">Teams</p>}
          {options.teams.map((t) => (
            <button
              key={`t${t.id}`}
              type="button"
              className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-[13px] hover:bg-accent"
              onClick={() => pick(t)}
            >
              <UserAvatar name={t.name} team className="size-5" />
              <span className="flex-1 truncate text-left">{t.name}</span>
              {isSel(t) && <Check className="size-3.5" />}
            </button>
          ))}
          {options.users.length > 0 && <p className="px-2 pt-2 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">People</p>}
          {options.users.map((u) => (
            <button
              key={`u${u.id}`}
              type="button"
              className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-[13px] hover:bg-accent"
              onClick={() => pick(u)}
            >
              <UserAvatar name={u.name} className="size-5" />
              <span className="min-w-0 flex-1 truncate text-left">
                {u.name}
                {u.id === me?.id && <span className="text-muted-foreground"> (you)</span>}
              </span>
              {isSel(u) && <Check className="size-3.5" />}
            </button>
          ))}
          {members.isPending && <p className="px-2 py-3 text-center text-xs text-muted-foreground">Loading…</p>}
          {!members.isPending && options.users.length + options.teams.length === 0 && (
            <p className="px-2 py-3 text-center text-xs text-muted-foreground">Nobody matches “{q}”</p>
          )}
        </div>
      </PopoverContent>
    </Popover>
  )
}

export function AssigneeAvatar({ assignee, className }: { assignee: Assignee | null; className?: string }) {
  if (!assignee)
    return (
      <span className={cn('flex size-6 items-center justify-center rounded-full border border-dashed text-muted-foreground', className)}>
        <UserRound className="size-3" />
      </span>
    )
  return <UserAvatar name={assignee.name} team={assignee.type === 'team'} className={className} />
}
