import { useNavigate } from '@tanstack/react-router'
import { Menu } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import { TooltipProvider } from '@/components/ui/tooltip'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/components/ui/sheet'
import { Logo } from '@/components/common/logo'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { cn } from '@/lib/utils'
import { CommandPalette } from './command-palette'
import { ShellContext } from './shell-context'
import { ShortcutsDialog } from './shortcuts-dialog'
import { SidebarContent } from './sidebar'

const COLLAPSE_KEY = 'rupta-sidebar-collapsed'

export function AppShell({ org, children }: { org: string; children: ReactNode }) {
  const navigate = useNavigate()
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem(COLLAPSE_KEY) === '1'
    } catch {
      return false
    }
  })
  const [mobileOpen, setMobileOpen] = useState(false)
  const [palette, setPalette] = useState<{ open: boolean; initial: string }>({ open: false, initial: '' })
  const [shortcuts, setShortcuts] = useState(false)

  const toggleSidebar = useCallback(() => {
    setCollapsed((c) => {
      try {
        localStorage.setItem(COLLAPSE_KEY, c ? '0' : '1')
      } catch {
        /* ignore */
      }
      return !c
    })
  }, [])

  useEffect(() => {
    try {
      localStorage.setItem('rupta-last-org', org)
    } catch {
      /* ignore */
    }
  }, [org])

  useEffect(() => {
    const on = () => setShortcuts(true)
    window.addEventListener('rupta:shortcuts', on)
    return () => window.removeEventListener('rupta:shortcuts', on)
  }, [])

  useHotkeys(
    {
      'mod+k': () => setPalette((p) => ({ open: !p.open, initial: '' })),
      '?': () => setShortcuts(true),
      '[': toggleSidebar,
      'g i': () => void navigate({ to: '/$org/issues', params: { org } }),
      'g r': () => void navigate({ to: '/$org/releases', params: { org } }),
      'g c': () => void navigate({ to: '/$org/crons', params: { org } }),
      'g f': () => void navigate({ to: '/$org/feedback', params: { org } }),
      'g a': () => void navigate({ to: '/$org/alerts', params: { org } }),
      'g s': () => void navigate({ to: '/$org/settings', params: { org } }),
      'g p': () => void navigate({ to: '/$org/performance', params: { org } }),
      'g t': () => void navigate({ to: '/$org/traces', params: { org } }),
      'g l': () => void navigate({ to: '/$org/logs', params: { org } }),
      'g m': () => void navigate({ to: '/$org/metrics', params: { org } }),
      'g d': () => void navigate({ to: '/$org/dashboards', params: { org } }),
      'g o': () => void navigate({ to: '/$org/profiles', params: { org } }),
      'g v': () => void navigate({ to: '/$org/replays', params: { org } }),
    },
    [org, toggleSidebar],
  )

  const ctx = useMemo(
    () => ({ openPalette: (initial = '') => setPalette({ open: true, initial }), openShortcuts: () => setShortcuts(true), toggleSidebar }),
    [toggleSidebar],
  )

  return (
    <TooltipProvider>
      <ShellContext.Provider value={ctx}>
        <a href="#main" className="sr-only z-50 rounded-md bg-background px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
          Skip to content
        </a>
        <div className="flex h-svh overflow-hidden">
          <aside
            className={cn('hidden shrink-0 border-r bg-sidebar transition-[width] duration-150 lg:block', collapsed ? 'w-14' : 'w-60')}
            aria-label="Sidebar"
          >
            <SidebarContent org={org} collapsed={collapsed} />
          </aside>
          <Sheet open={mobileOpen} onOpenChange={setMobileOpen}>
            <SheetContent side="left" className="w-64">
              <SheetTitle>Navigation</SheetTitle>
              <SheetDescription>Main navigation</SheetDescription>
              <SidebarContent org={org} collapsed={false} onNavigate={() => setMobileOpen(false)} />
            </SheetContent>
          </Sheet>
          <div className="flex min-w-0 flex-1 flex-col">
            <div className="flex h-11 shrink-0 items-center gap-2 border-b px-3 lg:hidden">
              <Button variant="ghost" size="icon-sm" onClick={() => setMobileOpen(true)} aria-label="Open navigation">
                <Menu />
              </Button>
              <Logo />
            </div>
            <main id="main" className="min-h-0 flex-1 overflow-y-auto" tabIndex={-1}>
              {children}
            </main>
          </div>
        </div>
        <CommandPalette open={palette.open} initial={palette.initial} onOpenChange={(open) => setPalette((p) => ({ ...p, open }))} />
        <ShortcutsDialog open={shortcuts} onOpenChange={setShortcuts} />
      </ShellContext.Provider>
    </TooltipProvider>
  )
}
