import { Link } from '@tanstack/react-router'
import { Bell, ChartLine, ChevronsLeft, ChevronsRight, CircleAlert, Clock, Film, Flame, Gauge, MessageSquareText, Package, Route as RouteIcon, ScrollText, Search, Settings } from 'lucide-react'
import { Logo } from '@/components/common/logo'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'
import { Tip } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { OrgSwitcher } from './org-switcher'
import { useShell } from './shell-context'
import { UserMenu } from './user-menu'

export const NAV = [
  { to: '/$org/issues', label: 'Issues', icon: CircleAlert, keys: 'g i', group: 'main' },
  { to: '/$org/releases', label: 'Releases', icon: Package, keys: 'g r', group: 'main' },
  { to: '/$org/crons', label: 'Crons', icon: Clock, keys: 'g c', group: 'main' },
  { to: '/$org/feedback', label: 'User feedback', icon: MessageSquareText, keys: 'g f', group: 'main' },
  { to: '/$org/alerts', label: 'Alerts', icon: Bell, keys: 'g a', group: 'main' },
  { to: '/$org/performance', label: 'Performance', icon: Gauge, keys: 'g p', group: 'explore' },
  { to: '/$org/traces', label: 'Traces', icon: RouteIcon, keys: 'g t', group: 'explore' },
  { to: '/$org/logs', label: 'Logs', icon: ScrollText, keys: 'g l', group: 'explore' },
  { to: '/$org/metrics', label: 'Metrics', icon: ChartLine, keys: 'g m', group: 'explore' },
  { to: '/$org/profiles', label: 'Profiles', icon: Flame, keys: 'g o', group: 'explore' },
  { to: '/$org/replays', label: 'Replays', icon: Film, keys: 'g v', group: 'explore' },
  { to: '/$org/settings', label: 'Settings', icon: Settings, keys: 'g s', group: 'settings' },
] as const

const NAV_GROUPS = [
  { id: 'main', label: null },
  { id: 'explore', label: 'Explore' },
  { id: 'settings', label: null },
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
            aria-label={collapsed ? 'Open command palette' : undefined}
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
      <nav aria-label="Main" className={cn('mt-3 flex min-h-0 flex-col gap-0.5 overflow-y-auto px-2', collapsed && 'px-1.5')}>
        {NAV_GROUPS.map((g) => (
          <div key={g.id} role="group" aria-label={g.label ?? undefined} className={cn('flex flex-col gap-0.5', g.id !== 'main' && 'mt-3')}>
            {g.label && !collapsed && <p className="px-2 pb-0.5 text-[11px] font-medium tracking-wide text-muted-foreground/80 uppercase">{g.label}</p>}
            {g.label && collapsed && <span className="mx-2 mb-1 h-px bg-border" aria-hidden />}
        {NAV.filter((item) => item.group === g.id).map((item) => (
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
          </div>
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
