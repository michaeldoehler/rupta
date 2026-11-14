import { useNavigate } from '@tanstack/react-router'
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/components/ui/sheet'
import { TooltipProvider } from '@/components/ui/tooltip'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { CommandPalette } from './command-palette'
import { ShellContext } from './shell-context'
import { ShortcutsDialog } from './shortcuts-dialog'
import { MobileNav, NAV, TopBar } from './top-bar'

export function AppShell({ org, children }: { org: string; children: ReactNode }) {
  const navigate = useNavigate()
  const [mobileOpen, setMobileOpen] = useState(false)
  const [palette, setPalette] = useState<{ open: boolean; initial: string }>({ open: false, initial: '' })
  const [shortcuts, setShortcuts] = useState(false)

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

  // g-sequences for every page in the navigation.
  const goKeys = useMemo(() => Object.fromEntries(NAV.map((n) => [n.keys, () => void navigate({ to: n.to, params: { org } })])), [navigate, org])
  useHotkeys(
    {
      'mod+k': () => setPalette((p) => ({ open: !p.open, initial: '' })),
      '?': () => setShortcuts(true),
      ...goKeys,
    },
    [goKeys],
  )

  const ctx = useMemo(() => ({ openPalette: (initial = '') => setPalette({ open: true, initial }), openShortcuts: () => setShortcuts(true) }), [])

  return (
    <TooltipProvider>
      <ShellContext.Provider value={ctx}>
        <a href="#main" className="sr-only z-50 rounded-md bg-background px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
          Skip to content
        </a>
        <div className="flex h-svh flex-col overflow-hidden">
          <TopBar org={org} onOpenMenu={() => setMobileOpen(true)} />
          <Sheet open={mobileOpen} onOpenChange={setMobileOpen}>
            <SheetContent side="left" className="w-64">
              <SheetTitle>Navigation</SheetTitle>
              <SheetDescription>Main navigation</SheetDescription>
              <MobileNav org={org} onNavigate={() => setMobileOpen(false)} />
            </SheetContent>
          </Sheet>
          <main id="main" className="min-h-0 flex-1 overflow-y-auto" tabIndex={-1}>
            {children}
          </main>
        </div>
        <CommandPalette open={palette.open} initial={palette.initial} onOpenChange={(open) => setPalette((p) => ({ ...p, open }))} />
        <ShortcutsDialog open={shortcuts} onOpenChange={setShortcuts} />
      </ShellContext.Provider>
    </TooltipProvider>
  )
}
