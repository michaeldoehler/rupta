import { AppWindow, Cpu, Globe2, HardDrive, Languages, Monitor, Network, Smartphone, Terminal, UserRound } from 'lucide-react'
import type { EventUser } from '@/api/types'
import { UserAvatar } from '@/components/common/avatar'
import { CopyButton } from '@/components/common/copy-button'
import { bytes } from '@/lib/format'
import { ValueView } from './value-view'

const ICONS: Record<string, React.ComponentType<{ className?: string }>> = {
  user: UserRound,
  browser: Globe2,
  os: Monitor,
  device: Smartphone,
  runtime: Terminal,
  app: AppWindow,
  trace: Network,
  culture: Languages,
  gpu: Cpu,
}

const TITLES: Record<string, string> = {
  os: 'Operating system',
  trace: 'Trace',
  culture: 'Culture',
  app: 'App',
  runtime: 'Runtime',
  device: 'Device',
  browser: 'Browser',
  gpu: 'GPU',
  react: 'React',
}

function headline(key: string, ctx: Record<string, unknown>): string | null {
  const name = ctx.name as string | undefined
  const version = ctx.version as string | undefined
  if (name) return `${name}${version ? ` ${version}` : ''}`
  if (key === 'device') return (ctx.model as string) ?? (ctx.family as string) ?? (ctx.arch as string) ?? null
  if (key === 'react' && version) return `React ${version}`
  return null
}

function fmtValue(k: string, v: unknown): React.ReactNode {
  if (typeof v === 'number' && /memory|size|storage/.test(k)) return bytes(v)
  if (typeof v === 'string' && (k === 'trace_id' || k === 'span_id'))
    return (
      <span className="inline-flex items-center gap-1">
        <span className="font-mono break-all">{v}</span>
        <CopyButton value={v} label={`Copy ${k}`} />
      </span>
    )
  if (typeof v === 'string') return <span className="font-mono break-all">{v}</span>
  return <ValueView value={v} />
}

function Card({
  title,
  sub,
  icon: Icon,
  rows,
}: {
  title: string
  sub: string | null
  icon: React.ComponentType<{ className?: string }>
  rows: [string, React.ReactNode][]
}) {
  return (
    <div className="min-w-0 rounded-lg border bg-card">
      <div className="flex items-center gap-2.5 border-b px-3 py-2">
        <span className="flex size-7 items-center justify-center rounded-md bg-muted text-muted-foreground">
          <Icon className="size-3.5" />
        </span>
        <div className="min-w-0">
          <div className="truncate text-[13px] font-semibold">{sub ?? title}</div>
          {sub && <div className="text-[11px] text-muted-foreground">{title}</div>}
        </div>
      </div>
      <dl className="divide-y text-[12.5px]">
        {rows.map(([k, v]) => (
          <div key={k} className="grid grid-cols-[minmax(0,2fr)_minmax(0,3fr)] gap-2 px-3 py-1.5">
            <dt className="truncate text-muted-foreground" title={k}>
              {k}
            </dt>
            <dd className="min-w-0">{v}</dd>
          </div>
        ))}
      </dl>
    </div>
  )
}

export function UserCard({ user }: { user: EventUser }) {
  const label = user.email ?? user.username ?? user.id ?? user.ip_address ?? 'Anonymous'
  const rows: [string, React.ReactNode][] = []
  if (user.id) rows.push(['ID', fmtValue('id', user.id)])
  if (user.email) rows.push(['Email', fmtValue('email', user.email)])
  if (user.username) rows.push(['Username', fmtValue('username', user.username)])
  if (user.ip_address) rows.push(['IP address', fmtValue('ip', user.ip_address)])
  if (user.geo) rows.push(['Location', [user.geo.city, user.geo.region, user.geo.country_code].filter(Boolean).join(', ')])
  for (const [k, v] of Object.entries(user.data ?? {})) rows.push([k, fmtValue(k, v)])
  return (
    <div className="min-w-0 rounded-lg border bg-card">
      <div className="flex items-center gap-2.5 border-b px-3 py-2">
        <UserAvatar name={label} className="size-7" />
        <div className="min-w-0">
          <div className="truncate text-[13px] font-semibold">{label}</div>
          <div className="text-[11px] text-muted-foreground">User</div>
        </div>
      </div>
      <dl className="divide-y text-[12.5px]">
        {rows.map(([k, v]) => (
          <div key={k} className="grid grid-cols-[minmax(0,2fr)_minmax(0,3fr)] gap-2 px-3 py-1.5">
            <dt className="text-muted-foreground">{k}</dt>
            <dd className="min-w-0">{v}</dd>
          </div>
        ))}
      </dl>
    </div>
  )
}

export function Contexts({ contexts, user }: { contexts: Record<string, Record<string, unknown>>; user: EventUser | null }) {
  const order = ['browser', 'os', 'device', 'runtime', 'app', 'trace', 'culture']
  const keys = Object.keys(contexts).sort((a, b) => (order.indexOf(a) + 1 || 99) - (order.indexOf(b) + 1 || 99))
  return (
    <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
      {user && <UserCard user={user} />}
      {keys.map((k) => {
        const ctx = contexts[k]
        const rows = Object.entries(ctx)
          .filter(([key]) => key !== 'type')
          .map(([key, v]) => [key, fmtValue(key, v)] as [string, React.ReactNode])
        return <Card key={k} title={TITLES[k] ?? k} sub={headline(k, ctx)} icon={ICONS[k] ?? HardDrive} rows={rows} />
      })}
    </div>
  )
}
