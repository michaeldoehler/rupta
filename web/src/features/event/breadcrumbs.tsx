import { ArrowRightLeft, Bug, Compass, Database, Globe, Info, MousePointerClick, Search, SquareTerminal, TriangleAlert, UserRound } from 'lucide-react'
import { useMemo, useState } from 'react'
import type { Breadcrumb } from '@/api/types'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { absolute } from '@/lib/format'
import { cn } from '@/lib/utils'
import { ValueView } from './value-view'

function crumbKind(b: Breadcrumb): string {
  if (b.category === 'exception' || b.type === 'error') return 'error'
  if (b.type === 'navigation' || b.category === 'navigation') return 'navigation'
  if (b.type === 'http' || b.category === 'fetch' || b.category === 'xhr' || b.category === 'httplib') return 'http'
  if (b.category?.startsWith('ui.') || b.type === 'user') return 'user'
  if (b.type === 'query' || b.category?.startsWith('db')) return 'query'
  if (b.category === 'console' || b.category === 'logger' || b.type === 'debug') return 'log'
  return 'default'
}

const KIND_ICON: Record<string, React.ComponentType<{ className?: string }>> = {
  error: Bug,
  navigation: Compass,
  http: ArrowRightLeft,
  user: MousePointerClick,
  query: Database,
  log: SquareTerminal,
  default: Info,
}

const KIND_LABEL: Record<string, string> = {
  error: 'Error',
  navigation: 'Navigation',
  http: 'HTTP',
  user: 'User action',
  query: 'Query',
  log: 'Console / log',
  default: 'Other',
}

const LEVEL_STYLE: Record<string, string> = {
  fatal: 'bg-level-fatal text-white',
  error: 'bg-level-error text-white',
  warning: 'bg-level-warning text-black/80',
  info: 'bg-muted text-muted-foreground',
  debug: 'bg-muted text-muted-foreground',
  log: 'bg-muted text-muted-foreground',
}

function summary(b: Breadcrumb): React.ReactNode {
  const d = b.data ?? {}
  if (crumbKind(b) === 'http' && d.url) {
    const status = d.status_code as number | undefined
    return (
      <span className="font-mono">
        <span className="font-semibold">{String(d.method ?? 'GET')}</span> {String(d.url)}{' '}
        {status !== undefined && (
          <span className={cn('font-semibold', status >= 500 ? 'text-level-error' : status >= 400 ? 'text-warning' : 'text-success')}>[{status}]</span>
        )}
      </span>
    )
  }
  if (crumbKind(b) === 'navigation' && d.to)
    return (
      <span className="font-mono">
        {String(d.from ?? '')} → {String(d.to)}
      </span>
    )
  return <span className={cn(b.type === 'query' && 'font-mono')}>{b.message}</span>
}

export function Breadcrumbs({ crumbs, eventTime }: { crumbs: Breadcrumb[]; eventTime: string }) {
  const [q, setQ] = useState('')
  const [kinds, setKinds] = useState<Set<string>>(new Set())
  const [levels, setLevels] = useState<Set<string>>(new Set())
  const [timeMode, setTimeMode] = useState<'relative' | 'absolute'>('relative')
  const [showAll, setShowAll] = useState(false)
  const end = new Date(eventTime).getTime()

  const allKinds = useMemo(() => [...new Set(crumbs.map(crumbKind))], [crumbs])
  const allLevels = useMemo(() => [...new Set(crumbs.map((c) => c.level ?? 'info'))], [crumbs])
  const filtered = useMemo(() => {
    const needle = q.toLowerCase()
    return crumbs
      .filter((c) => (kinds.size === 0 || kinds.has(crumbKind(c))) && (levels.size === 0 || levels.has(c.level ?? 'info')))
      .filter((c) => !needle || JSON.stringify(c).toLowerCase().includes(needle))
      .reverse()
  }, [crumbs, q, kinds, levels])
  const visible = showAll || q || kinds.size || levels.size ? filtered : filtered.slice(0, 8)

  const toggle = (set: Set<string>, setter: (s: Set<string>) => void, v: string) => {
    const n = new Set(set)
    if (n.has(v)) n.delete(v)
    else n.add(v)
    setter(n)
  }

  function rel(ts: Breadcrumb['timestamp']) {
    const t = typeof ts === 'number' ? ts * 1000 : new Date(ts).getTime()
    const s = (t - end) / 1000
    if (Math.abs(s) < 0.05) return '0.0s'
    return `${s < 0 ? '−' : '+'}${Math.abs(s) >= 60 ? `${Math.floor(Math.abs(s) / 60)}m${Math.round(Math.abs(s) % 60)}s` : `${Math.abs(s).toFixed(1)}s`}`
  }

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative max-w-xs min-w-40 flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Search breadcrumbs"
            className="h-7 pl-7 text-[13px]"
            aria-label="Search breadcrumbs"
          />
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size="sm" className="font-normal">
              Filter{kinds.size + levels.size > 0 && ` (${kinds.size + levels.size})`}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuLabel>Type</DropdownMenuLabel>
            {allKinds.map((k) => (
              <DropdownMenuCheckboxItem key={k} checked={kinds.has(k)} onCheckedChange={() => toggle(kinds, setKinds, k)} onSelect={(e) => e.preventDefault()}>
                {KIND_LABEL[k]}
              </DropdownMenuCheckboxItem>
            ))}
            <DropdownMenuSeparator />
            <DropdownMenuLabel>Level</DropdownMenuLabel>
            {allLevels.map((l) => (
              <DropdownMenuCheckboxItem
                key={l}
                checked={levels.has(l)}
                onCheckedChange={() => toggle(levels, setLevels, l)}
                onSelect={(e) => e.preventDefault()}
                className="capitalize"
              >
                {l}
              </DropdownMenuCheckboxItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <ToggleGroup
          type="single"
          value={timeMode}
          onValueChange={(v) => v && setTimeMode(v as 'relative' | 'absolute')}
          aria-label="Time display"
          className="ml-auto"
        >
          <ToggleGroupItem value="relative">Relative</ToggleGroupItem>
          <ToggleGroupItem value="absolute">Absolute</ToggleGroupItem>
        </ToggleGroup>
      </div>
      <div className="overflow-hidden rounded-lg border bg-card">
        <div className="grid grid-cols-[28px_1fr_auto] gap-x-3 border-b bg-muted/40 px-3 py-1.5 text-[11px] font-medium tracking-wide text-muted-foreground uppercase sm:grid-cols-[28px_110px_1fr_70px_auto]">
          <span />
          <span className="hidden sm:block">Category</span>
          <span>Description</span>
          <span className="hidden sm:block">Level</span>
          <span className="text-right">Time</span>
        </div>
        <ol className="relative divide-y">
          {visible.map((c, i) => {
            const kind = crumbKind(c)
            const Icon =
              c.level === 'warning'
                ? TriangleAlert
                : kind === 'user' && c.category === 'ui.input'
                  ? UserRound
                  : kind === 'http' && String(c.category).includes('xhr')
                    ? Globe
                    : KIND_ICON[kind]
            const data = c.data && Object.keys(c.data).length && kind !== 'navigation' ? c.data : null
            return (
              <li
                key={i}
                className={cn(
                  'grid grid-cols-[28px_1fr_auto] items-start gap-x-3 px-3 py-2 text-[12.5px] sm:grid-cols-[28px_110px_1fr_70px_auto]',
                  kind === 'error' && 'bg-destructive/5',
                )}
              >
                <span
                  className={cn(
                    'mt-px flex size-6 items-center justify-center rounded-md border bg-card',
                    kind === 'error' && 'border-level-error/40 text-level-error',
                    c.level === 'warning' && 'border-level-warning/50 text-warning',
                    kind === 'navigation' && 'text-level-info',
                    kind === 'user' && 'text-brand-text',
                  )}
                >
                  <Icon className="size-3.5" />
                </span>
                <span className="hidden truncate pt-0.5 font-mono text-[12px] text-muted-foreground sm:block">{c.category ?? c.type}</span>
                <div className="min-w-0 pt-0.5 break-words">
                  {summary(c)}
                  {data && kind !== 'http' && (
                    <div className="mt-1">
                      <ValueView value={data} className="text-[12px]" />
                    </div>
                  )}
                </div>
                <span className="hidden pt-0.5 sm:block">
                  <span className={cn('rounded-sm px-1 py-px text-[10.5px] font-medium capitalize', LEVEL_STYLE[c.level ?? 'info'] ?? LEVEL_STYLE.info)}>
                    {c.level ?? 'info'}
                  </span>
                </span>
                <time className="pt-0.5 text-right font-mono text-[11.5px] whitespace-nowrap text-muted-foreground tabular" dateTime={String(c.timestamp)}>
                  {timeMode === 'relative'
                    ? rel(c.timestamp)
                    : absolute(typeof c.timestamp === 'number' ? c.timestamp * 1000 : c.timestamp, { date: false, ms: true })}
                </time>
              </li>
            )
          })}
          {visible.length === 0 && <li className="px-3 py-6 text-center text-xs text-muted-foreground">No breadcrumbs match.</li>}
        </ol>
        {!showAll && filtered.length > visible.length && (
          <button
            type="button"
            onClick={() => setShowAll(true)}
            className="w-full border-t py-1.5 text-center text-xs font-medium text-muted-foreground hover:bg-row-hover hover:text-foreground"
          >
            Show {filtered.length - visible.length} older breadcrumbs
          </button>
        )}
      </div>
    </div>
  )
}
