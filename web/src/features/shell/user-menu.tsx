import { useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { Keyboard, LogOut, Settings, UserRound } from 'lucide-react'
import { api } from '@/api/endpoints'
import { qk, useSession } from '@/api/queries'
import { UserAvatar } from '@/components/common/avatar'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import { useShell } from './shell-context'

/** Account menu; `compact` shows only the avatar (top bar). */
export function UserMenu({ org, compact = false }: { org: string; compact?: boolean }) {
  const { data: session } = useSession()
  const { openShortcuts } = useShell()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const user = session?.user
  if (!user) return null

  async function signOut() {
    try {
      await api.auth.logout()
    } finally {
      qc.setQueryData(qk.session, null)
      qc.removeQueries({ predicate: (q) => q.queryKey[0] !== 'auth' })
      await navigate({ to: '/login' })
    }
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          className={cn('justify-start gap-2', compact ? 'size-8 rounded-full p-0' : 'h-9 w-full px-1.5')}
          aria-label={compact ? `Account menu (${user.email})` : undefined}
          title="Account menu"
        >
          <UserAvatar name={user.name || user.email} src={user.avatar_url} />
          {!compact && (
            <span className="min-w-0 flex-1 text-left">
              <span className="block truncate text-[13px] leading-tight font-medium">{user.name || user.email}</span>
              <span className="block truncate text-[11px] leading-tight text-muted-foreground">{user.email}</span>
            </span>
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" side="bottom" className="w-60">
        <DropdownMenuLabel className="normal-case">{user.email}</DropdownMenuLabel>
        <DropdownMenuItem asChild>
          <Link to="/$org/settings/account" params={{ org }}>
            <UserRound /> Account settings
          </Link>
        </DropdownMenuItem>
        <DropdownMenuItem asChild>
          <Link to="/$org/settings" params={{ org }}>
            <Settings /> Organization settings
          </Link>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={openShortcuts}>
          <Keyboard /> Keyboard shortcuts <DropdownMenuShortcut>?</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => void signOut()}>
          <LogOut /> Sign out
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
