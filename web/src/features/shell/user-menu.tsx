import { useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { Keyboard, LogOut, Monitor, Moon, Settings, Sun, UserRound } from 'lucide-react'
import { api } from '@/api/endpoints'
import { qk, useSession } from '@/api/queries'
import { UserAvatar } from '@/components/common/avatar'
import { useTheme, type Theme } from '@/components/theme-provider'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import { useShell } from './shell-context'

export function UserMenu({ org, collapsed }: { org: string; collapsed: boolean }) {
  const { data: session } = useSession()
  const { theme, setTheme } = useTheme()
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
        <Button variant="ghost" className={cn('h-9 w-full justify-start gap-2 px-1.5', collapsed && 'justify-center px-0')} aria-label="Account menu">
          <UserAvatar name={user.name || user.email} src={user.avatar_url} />
          {!collapsed && (
            <span className="min-w-0 flex-1 text-left">
              <span className="block truncate text-[13px] leading-tight font-medium">{user.name || user.email}</span>
              <span className="block truncate text-[11px] leading-tight text-muted-foreground">{user.email}</span>
            </span>
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" side="top" className="w-60">
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
        <DropdownMenuSub>
          <DropdownMenuSubTrigger>{theme === 'dark' ? <Moon /> : theme === 'light' ? <Sun /> : <Monitor />} Theme</DropdownMenuSubTrigger>
          <DropdownMenuSubContent>
            <DropdownMenuRadioGroup value={theme} onValueChange={(v) => setTheme(v as Theme)}>
              <DropdownMenuRadioItem value="light">Light</DropdownMenuRadioItem>
              <DropdownMenuRadioItem value="dark">Dark</DropdownMenuRadioItem>
              <DropdownMenuRadioItem value="system">System</DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>
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
