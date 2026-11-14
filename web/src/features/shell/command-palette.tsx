import { useQuery } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { ArrowRight, Building2, CircleAlert, FolderCog, Hash, Keyboard, LayoutDashboard, Layers, Monitor, Moon, Route as RouteIcon, ScrollText, Sun } from 'lucide-react'
import { useDeferredValue, useEffect, useState } from 'react'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { useProjects, useSession } from '@/api/queries'
import { LevelDot } from '@/components/common/level'
import { PlatformIcon } from '@/components/common/platform-icon'
import { useTheme } from '@/components/theme-provider'
import { CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator, CommandShortcut } from '@/components/ui/command'
import { Kbd } from '@/components/ui/kbd'
import { NAV } from './sidebar'
import { useGlobalFilters } from './use-global-filters'

const SETTINGS_PAGES = [
  { to: '/$org/settings/account', label: 'Account: profile & preferences' },
  { to: '/$org/settings/account/security', label: 'Account: password & tokens' },
  { to: '/$org/settings/account/notifications', label: 'Account: notifications' },
  { to: '/$org/settings', label: 'Organization: general' },
  { to: '/$org/settings/members', label: 'Organization: members' },
  { to: '/$org/settings/teams', label: 'Organization: teams' },
  { to: '/$org/settings/sso', label: 'Organization: single sign-on' },
  { to: '/$org/settings/tokens', label: 'Organization: API tokens' },
  { to: '/$org/settings/audit-log', label: 'Organization: audit log' },
  { to: '/$org/settings/channels', label: 'Organization: notification channels' },
  { to: '/$org/settings/projects', label: 'Projects' },
] as const

export const SHORT_ID_RE = /^[a-z][a-z0-9]*-[0-9a-z]+$/i
export const TRACE_ID_RE = /^[0-9a-f]{32}$/i

export function CommandPalette({ open, onOpenChange, initial = '' }: { open: boolean; onOpenChange: (o: boolean) => void; initial?: string }) {
  const { org, update } = useGlobalFilters()
  const navigate = useNavigate()
  const { data: session } = useSession()
  const { data: projects } = useProjects(org)
  const { setTheme } = useTheme()
  const [input, setInput] = useState(initial)
  const q = useDeferredValue(input.trim())

  useEffect(() => {
    if (open) setInput(initial)
  }, [open, initial])

  const issues = useQuery({
    queryKey: ['palette-issues', org, q],
    queryFn: ({ signal }) => api.issues.list(org, { query: q, limit: 6, sort: 'last_seen' }, signal),
    enabled: open && q.length >= 2 && !SHORT_ID_RE.test(q) && !TRACE_ID_RE.test(q.replace(/-/g, '')),
    staleTime: 30_000,
  })

  const run = (fn: () => void) => () => {
    onOpenChange(false)
    fn()
  }

  async function jumpToShortId(id: string) {
    try {
      const issue = await api.issues.byShortId(org, id.toUpperCase())
      await navigate({ to: '/$org/issues/$issueId', params: { org, issueId: issue.id } })
    } catch (e) {
      toast.error(`Could not open ${id.toUpperCase()}`, { description: errorMessage(e) })
    }
  }

  return (
    <CommandDialog open={open} onOpenChange={onOpenChange} title="Command palette" description="Navigate, find issues, switch projects and more">
      <CommandInput placeholder="Type a command, page, or issue id like WEB-1A…" value={input} onValueChange={setInput} />
      <CommandList>
        <CommandEmpty>No results.</CommandEmpty>
        {SHORT_ID_RE.test(q) && (
          <CommandGroup heading="Issue">
            <CommandItem value={`goto-issue ${q}`} onSelect={run(() => void jumpToShortId(q))}>
              <Hash /> Go to issue <span className="font-mono font-medium">{q.toUpperCase()}</span>
              <CommandShortcut>
                <Kbd>↵</Kbd>
              </CommandShortcut>
            </CommandItem>
          </CommandGroup>
        )}
        {TRACE_ID_RE.test(q.replace(/-/g, '')) && (
          <CommandGroup heading="Trace">
            <CommandItem value={`goto-trace ${q}`} onSelect={run(() => void navigate({ to: '/$org/traces/$traceId', params: { org, traceId: q.replace(/-/g, '').toLowerCase() } }))}>
              <RouteIcon /> Open trace <span className="font-mono font-medium">{q.slice(0, 12)}…</span>
            </CommandItem>
            <CommandItem value={`trace-logs ${q}`} onSelect={run(() => void navigate({ to: '/$org/logs', params: { org }, search: { query: `trace:${q.replace(/-/g, '').toLowerCase()}` } }))}>
              <ScrollText /> Logs of this trace
            </CommandItem>
          </CommandGroup>
        )}
        {issues.data && issues.data.data.length > 0 && (
          <CommandGroup heading="Issues">
            {issues.data.data.map((i) => (
              <CommandItem
                key={i.id}
                value={`issue ${i.short_id} ${i.title} ${q}`}
                onSelect={run(() => void navigate({ to: '/$org/issues/$issueId', params: { org, issueId: i.id } }))}
              >
                <LevelDot level={i.level} />
                <span className="min-w-0 flex-1 truncate">{i.title}</span>
                <span className="font-mono text-[11px] text-muted-foreground">{i.short_id}</span>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        <CommandGroup heading="Navigate">
          {NAV.map((n) => (
            <CommandItem key={n.to} value={`go ${n.label}`} onSelect={run(() => void navigate({ to: n.to, params: { org } }))}>
              <n.icon /> {n.label}
              <CommandShortcut>
                {n.keys.split(' ').map((k) => (
                  <Kbd key={k}>{k.toUpperCase()}</Kbd>
                ))}
              </CommandShortcut>
            </CommandItem>
          ))}
          <CommandItem value="go dashboards metrics" onSelect={run(() => void navigate({ to: '/$org/dashboards', params: { org } }))}>
            <LayoutDashboard /> Dashboards
            <CommandShortcut>
              <Kbd>G</Kbd>
              <Kbd>D</Kbd>
            </CommandShortcut>
          </CommandItem>
          <CommandItem value="logs errors only level:error" onSelect={run(() => void navigate({ to: '/$org/logs', params: { org }, search: { query: 'level:[error,fatal]' } }))}>
            <ScrollText /> Error logs
          </CommandItem>
          <CommandItem
            value="search issues assigned to me"
            onSelect={run(() => void navigate({ to: '/$org/issues', params: { org }, search: { query: 'is:unresolved assigned:me' } }))}
          >
            <CircleAlert /> My unresolved issues
          </CommandItem>
        </CommandGroup>
        <CommandGroup heading="Settings">
          {SETTINGS_PAGES.map((s) => (
            <CommandItem key={s.to} value={`settings ${s.label}`} onSelect={run(() => void navigate({ to: s.to, params: { org } }))}>
              <ArrowRight /> {s.label}
            </CommandItem>
          ))}
        </CommandGroup>
        {projects && projects.length > 0 && (
          <CommandGroup heading="Projects">
            {projects.map((p) => (
              <CommandItem key={`f-${p.id}`} value={`filter project ${p.slug} ${p.name}`} onSelect={run(() => update({ project: [p.id] }))}>
                <PlatformIcon platform={p.platform} size="sm" /> Show only <span className="font-medium">{p.slug}</span>
                <Layers className="ml-auto" />
              </CommandItem>
            ))}
            {projects.map((p) => (
              <CommandItem
                key={`s-${p.id}`}
                value={`project settings ${p.slug} ${p.name}`}
                onSelect={run(() => void navigate({ to: '/$org/settings/projects/$project', params: { org, project: p.slug } }))}
              >
                <FolderCog /> Settings for <span className="font-medium">{p.slug}</span>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        {session && session.orgs.length > 1 && (
          <CommandGroup heading="Organizations">
            {session.orgs
              .filter((o) => o.slug !== org)
              .map((o) => (
                <CommandItem
                  key={o.id}
                  value={`switch organization ${o.name} ${o.slug}`}
                  onSelect={run(() => void navigate({ to: '/$org/issues', params: { org: o.slug } }))}
                >
                  <Building2 /> Switch to {o.name}
                </CommandItem>
              ))}
          </CommandGroup>
        )}
        <CommandSeparator />
        <CommandGroup heading="Preferences">
          <CommandItem value="theme light" onSelect={run(() => setTheme('light'))}>
            <Sun /> Light theme
          </CommandItem>
          <CommandItem value="theme dark" onSelect={run(() => setTheme('dark'))}>
            <Moon /> Dark theme
          </CommandItem>
          <CommandItem value="theme system" onSelect={run(() => setTheme('system'))}>
            <Monitor /> System theme
          </CommandItem>
          <CommandItem value="keyboard shortcuts help" onSelect={run(() => window.dispatchEvent(new CustomEvent('rupta:shortcuts')))}>
            <Keyboard /> Keyboard shortcuts
            <CommandShortcut>
              <Kbd>?</Kbd>
            </CommandShortcut>
          </CommandItem>
        </CommandGroup>
      </CommandList>
    </CommandDialog>
  )
}
