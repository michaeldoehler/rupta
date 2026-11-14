import { Link, useRouterState } from '@tanstack/react-router'
import {
  Bell,
  ChartLine,
  ChevronDown,
  CircleAlert,
  Clock,
  Compass,
  Film,
  Flame,
  Gauge,
  LayoutDashboard,
  Menu,
  MessageSquareText,
  Package,
  Route as RouteIcon,
  ScrollText,
  Search,
  Settings,
} from 'lucide-react'
import { Logo } from '@/components/common/logo'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Kbd } from '@/components/ui/kbd'
import { Tip } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { OrgSwitcher } from './org-switcher'
import { useShell } from './shell-context'
import { UserMenu } from './user-menu'

/** Every top-level page, for the top bar, the mobile menu and the command palette. */
export const NAV = [
  { to: '/$org/issues', label: 'Issues', icon: CircleAlert, keys: 'g i', group: 'main' },
  { to: '/$org/performance', label: 'Performance', icon: Gauge, keys: 'g p', group: 'explore' },
  { to: '/$org/traces', label: 'Traces', icon: RouteIcon, keys: 'g t', group: 'explore' },
  { to: '/$org/logs', label: 'Logs', icon: ScrollText, keys: 'g l', group: 'explore' },
  { to: '/$org/metrics', label: 'Metrics', icon: ChartLine, keys: 'g m', group: 'explore' },
  { to: '/$org/dashboards', label: 'Dashboards', icon: LayoutDashboard, keys: 'g d', group: 'explore' },
  { to: '/$org/profiles', label: 'Profiles', icon: Flame, keys: 'g o', group: 'explore' },
  { to: '/$org/replays', label: 'Replays', icon: Film, keys: 'g v', group: 'explore' },
  { to: '/$org/releases', label: 'Releases', icon: Package, keys: 'g r', group: 'main' },
  { to: '/$org/crons', label: 'Crons', icon: Clock, keys: 'g c', group: 'main' },
  { to: '/$org/feedback', label: 'Feedback', icon: MessageSquareText, keys: 'g f', group: 'main' },
  { to: '/$org/alerts', label: 'Alerts', icon: Bell, keys: 'g a', group: 'main' },
  { to: '/$org/settings', label: 'Settings', icon: Settings, keys: 'g s', group: 'settings' },
] as const

type NavItem = (typeof NAV)[number]
const EXPLORE = NAV.filter((n) => n.group === 'explore')
const MAIN_BEFORE = NAV.filter((n) => n.to === '/$org/issues')
const MAIN_AFTER = NAV.filter((n) => n.group === 'main' && n.to !== '/$org/issues')

function pathOf(to: string, org: string) {
  return to.replace('$org', org)
}

function useActive(org: string) {
  const pathname = useRouterState({ select: (s) => s.location.pathname })
  return (to: string) => pathname === pathOf(to, org) || pathname.startsWith(`${pathOf(to, org)}/`)
}

const itemCls =
  'relative flex h-12 items-center gap-1.5 px-2.5 text-[13px] font-medium text-muted-foreground transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none focus-visible:ring-inset'
const indicator = 'after:absolute after:inset-x-2 after:bottom-0 after:h-0.5 after:rounded-full after:bg-brand'

function NavLink({ item, org }: { item: NavItem; org: string }) {
  return (
    <Link to={item.to} params={{ org }} className={itemCls} activeProps={{ className: cn('text-foreground', indicator), 'aria-current': 'page' }}>
      {item.label}
    </Link>
  )
}

function ExploreMenu({ org }: { org: string }) {
  const isActive = useActive(org)
  const current = EXPLORE.find((e) => isActive(e.to))
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className={cn(itemCls, current && cn('text-foreground', indicator))} aria-current={current ? 'page' : undefined}>
        <Compass className="size-3.5" />
        {current ? current.label : 'Explore'}
        <ChevronDown className="size-3.5 opacity-60" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-60">
        <DropdownMenuLabel>Explore</DropdownMenuLabel>
        {EXPLORE.map((e) => (
          <DropdownMenuItem key={e.to} asChild className={cn(isActive(e.to) && 'bg-accent')}>
            <Link to={e.to} params={{ org }}>
              <e.icon /> {e.label}
              <DropdownMenuShortcut>{e.keys}</DropdownMenuShortcut>
            </Link>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export function TopBar({ org, onOpenMenu }: { org: string; onOpenMenu: () => void }) {
  const { openPalette } = useShell()
  return (
    <header className="sticky top-0 z-30 flex h-12 shrink-0 items-center gap-1 border-b bg-sidebar/95 px-2 backdrop-blur sm:px-3" aria-label="Top bar">
      <Button variant="ghost" size="icon-sm" className="lg:hidden" onClick={onOpenMenu} aria-label="Open navigation">
        <Menu />
      </Button>
      <Link
        to="/$org/issues"
        params={{ org }}
        className="mr-1 rounded-md px-1 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        aria-label="rupta home"
      >
        <Logo />
      </Link>
      <span className="mx-1 hidden h-5 w-px bg-border sm:block" aria-hidden />
      <div className="hidden sm:block">
        <OrgSwitcher org={org} compact />
      </div>
      <nav aria-label="Main" className="ml-2 hidden items-stretch lg:flex">
        {MAIN_BEFORE.map((n) => (
          <NavLink key={n.to} item={n} org={org} />
        ))}
        <ExploreMenu org={org} />
        {MAIN_AFTER.map((n) => (
          <NavLink key={n.to} item={n} org={org} />
        ))}
      </nav>
      <div className="ml-auto flex items-center gap-1">
        <button
          type="button"
          onClick={() => openPalette()}
          className="flex h-8 items-center gap-2 rounded-md border bg-card px-2 text-[13px] text-muted-foreground transition-colors hover:border-border-strong hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none xl:w-64"
          aria-label="Search or jump to (⌘K)"
        >
          <Search className="size-3.5" />
          <span className="hidden flex-1 text-left xl:inline">Search or jump to…</span>
          <Kbd className="hidden sm:inline-flex">⌘K</Kbd>
        </button>
        <Tip content="Settings (g s)">
          <Button asChild variant="ghost" size="icon-sm">
            <Link to="/$org/settings" params={{ org }} aria-label="Settings" activeProps={{ className: 'text-brand' }}>
              <Settings />
            </Link>
          </Button>
        </Tip>
        <UserMenu org={org} compact />
      </div>
    </header>
  )
}

/** The navigation as a list, for the menu on narrow screens. */
export function MobileNav({ org, onNavigate }: { org: string; onNavigate: () => void }) {
  const isActive = useActive(org)
  const group = (title: string | null, items: readonly NavItem[]) => (
    <div className="space-y-0.5">
      {title && <p className="px-2 pt-3 pb-1 text-[11px] font-medium tracking-wide text-subtle-foreground uppercase">{title}</p>}
      {items.map((n) => (
        <Link
          key={n.to}
          to={n.to}
          params={{ org }}
          onClick={onNavigate}
          className={cn(
            'flex h-9 items-center gap-2.5 rounded-md border-l-2 border-transparent px-2 text-[13px] font-medium text-muted-foreground hover:bg-accent hover:text-foreground',
            isActive(n.to) && 'border-brand bg-accent text-foreground',
          )}
          aria-current={isActive(n.to) ? 'page' : undefined}
        >
          <n.icon className="size-4" /> {n.label}
        </Link>
      ))}
    </div>
  )
  return (
    <div className="flex h-full flex-col gap-2 overflow-y-auto p-3">
      <div className="flex h-9 items-center px-1">
        <Logo />
      </div>
      <OrgSwitcher org={org} />
      <nav aria-label="Main">
        {group(
          null,
          NAV.filter((n) => n.group === 'main'),
        )}
        {group('Explore', EXPLORE)}
        {group(
          null,
          NAV.filter((n) => n.group === 'settings'),
        )}
      </nav>
    </div>
  )
}
