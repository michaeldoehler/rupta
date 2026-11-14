import { Link } from '@tanstack/react-router'
import { Bell, ChevronsLeft, ChevronsRight, CircleAlert, Clock, MessageSquareText, Package, Search, Settings } from 'lucide-react'
import { Logo } from '@/components/common/logo'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'
import { Tip } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { OrgSwitcher } from './org-switcher'
import { useShell } from './shell-context'
import { UserMenu } from './user-menu'

export const NAV = [
  { to: '/$org/issues', label: 'Issues', icon: CircleAlert, keys: 'g i' },
  { to: '/$org/releases', label: 'Releases', icon: Package, keys: 'g r' },
  { to: '/$org/crons', label: 'Crons', icon: Clock, keys: 'g c' },
  { to: '/$org/feedback', label: 'User feedback', icon: MessageSquareText, keys: 'g f' },
  { to: '/$org/alerts', label: 'Alerts', icon: Bell, keys: 'g a' },
  { to: '/$org/settings', label: 'Settings', icon: Settings, keys: 'g s' },
] as const

export function SidebarContent({ org, collapsed, onNavigate }: { org: string; collapsed: boolean; onNavigate?: () => void }) {
  const { openPalette, toggleSidebar } = useShell()
  return (
    <div className="flex h-full flex-col">
      <div className={cn('flex h-12 items-center gap-1 px-3', collapsed && 'justify-center px-2')}>
        <Link
          to="/$org/issues"
          params={{ org }}
          className="rounded-md focus-visible:ring-2 focus-visible:ring-ring"
          aria-label="rupta home"
          onClick={onNavigate}
        >
          <Logo collapsed={collapsed} />
        </Link>
      </div>
      <div className={cn('px-2', collapsed && 'px-1.5')}>
        <OrgSwitcher org={org} collapsed={collapsed} />
      </div>
      <div className={cn('px-2 pt-2', collapsed && 'px-1.5')}>
        <Tip content={collapsed ? 'Search (⌘K)' : undefined} side="right">
          <button
            type="button"
            onClick={() => openPalette()}
            className={cn(
              'flex h-8 w-full items-center gap-2 rounded-md border bg-card px-2 text-[13px] text-muted-foreground shadow-xs transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none',
              collapsed && 'justify-center px-0',
            )}
            aria-label="Open command palette"
          >
            <Search className="size-3.5" />
            {!collapsed && (
              <>
                <span className="flex-1 text-left">Search or jump to…</span>
                <Kbd>⌘K</Kbd>
              </>
            )}
          </button>
        </Tip>
      </div>
      <nav aria-label="Main" className={cn('mt-3 flex flex-col gap-0.5 px-2', collapsed && 'px-1.5')}>
        {NAV.map((item) => (
          <Tip key={item.to} content={collapsed ? item.label : undefined} side="right">
            <Link
              to={item.to}
              params={{ org }}
              onClick={onNavigate}
              className={cn(
                'group flex h-8 items-center gap-2.5 rounded-md px-2 text-[13px] font-medium text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none',
                collapsed && 'justify-center px-0',
              )}
              activeProps={{ className: 'bg-sidebar-accent text-foreground', 'aria-current': 'page' }}
            >
              <item.icon className="size-4 shrink-0" />
              {!collapsed && <span className="flex-1">{item.label}</span>}
            </Link>
          </Tip>
        ))}
      </nav>
      <div className="flex-1" />
      <div className={cn('border-t px-2 py-2', collapsed && 'px-1.5')}>
        <UserMenu org={org} collapsed={collapsed} />
        <Button
          variant="ghost"
          size="sm"
          onClick={toggleSidebar}
          className={cn('mt-1 hidden w-full justify-start text-muted-foreground lg:flex', collapsed && 'justify-center')}
          aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          {collapsed ? <ChevronsRight /> : <ChevronsLeft />}
          {!collapsed && <span>Collapse</span>}
          {!collapsed && <Kbd className="ml-auto">[</Kbd>}
        </Button>
      </div>
    </div>
  )
}
