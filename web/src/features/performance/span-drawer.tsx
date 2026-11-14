import { Link } from '@tanstack/react-router'
import { ArrowUpRight, Flame, Gauge, ScrollText, X } from 'lucide-react'
import type { ReactNode } from 'react'
import type { TraceError, TraceLogMarker } from '@/api/telemetry-types'
import { CopyButton } from '@/components/common/copy-button'
import { LevelDot } from '@/components/common/level'
import { KeyValueTable } from '@/components/common/page'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { absolute } from '@/lib/format'
import { cn } from '@/lib/utils'
import { ValueView } from '@/features/event/value-view'
import { LOG_LEVEL_DOT, ms, opColor } from '@/features/telemetry/format'
import { opCategory, selfTime, type SpanNode } from './span-tree'

function Block({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="border-b px-4 py-3 last:border-b-0">
      <h3 className="mb-2 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{title}</h3>
      {children}
    </section>
  )
}

export function SpanDrawer({
  org,
  node,
  traceStart,
  traceId,
  errors,
  logs,
  projectName,
  onClose,
  onSelect,
}: {
  org: string
  node: SpanNode
  traceStart: number
  traceId: string
  errors: TraceError[]
  logs: TraceLogMarker[]
  projectName: (id: string | null) => string | null
  onClose: () => void
  onSelect: (id: string) => void
}) {
  const s = node.span
  const failed = s.status && s.status !== 'ok'
  const self = selfTime(node)
  const data = Object.entries(s.data)
  const tags = Object.entries(s.tags)
  return (
    <aside
      aria-label="Span details"
      className="fixed inset-y-0 right-0 z-40 flex w-full flex-col border-l bg-card shadow-xl sm:w-[440px] lg:absolute lg:inset-y-0 lg:shadow-lg"
      data-testid="span-drawer"
    >
      <div className="flex items-start gap-2 border-b px-4 py-3">
        <span className="mt-1.5 size-2.5 shrink-0 rounded-[2px]" style={{ background: opColor(opCategory(s.op)) }} aria-hidden />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <Badge variant="outline" className="font-mono font-normal">
              {s.op ?? 'span'}
            </Badge>
            {s.is_segment && <Badge variant="brand">transaction</Badge>}
            <Badge variant={failed ? 'destructive' : 'success'}>{s.status ?? 'unknown'}</Badge>
          </div>
          <h2 className="mt-1 text-sm font-semibold break-words">{s.is_segment ? (s.transaction ?? s.description) : (s.description ?? 'Span')}</h2>
        </div>
        <Button variant="ghost" size="icon-sm" onClick={onClose} aria-label="Close span details (Esc)">
          <X />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="grid grid-cols-3 gap-2 border-b px-4 py-3">
          <div>
            <div className="text-[11px] text-muted-foreground uppercase">Duration</div>
            <div className="font-semibold tabular">{ms(s.duration_ms)}</div>
          </div>
          <div>
            <div className="text-[11px] text-muted-foreground uppercase">Self time</div>
            <div className="font-semibold tabular">{ms(self)}</div>
          </div>
          <div>
            <div className="text-[11px] text-muted-foreground uppercase">Starts at</div>
            <div className="font-semibold tabular">+{ms(s.start - traceStart)}</div>
          </div>
        </div>
        <div className="flex flex-wrap gap-1.5 border-b px-4 py-2.5">
          {s.is_segment && s.transaction && (
            <Button asChild variant="outline" size="xs">
              <Link
                to="/$org/performance/summary"
                params={{ org }}
                search={(prev: Record<string, unknown>) => ({ ...prev, name: s.transaction!, tx_project: s.project_id ?? undefined })}
              >
                <Gauge /> Transaction summary
              </Link>
            </Button>
          )}
          {s.profile_id && s.project_id && (
            <Button asChild variant="outline" size="xs">
              <Link to="/$org/profiles/$project/$profileId" params={{ org, project: s.project_id, profileId: s.profile_id }}>
                <Flame /> Profile
              </Link>
            </Button>
          )}
          <Button asChild variant="outline" size="xs">
            <Link to="/$org/logs" params={{ org }} search={(prev: Record<string, unknown>) => ({ ...prev, query: `trace:${traceId} span:${s.span_id}` })}>
              <ScrollText /> Logs for this span
            </Link>
          </Button>
        </div>
        {errors.length > 0 && (
          <Block title={`Errors (${errors.length})`}>
            <ul className="space-y-1.5">
              {errors.map((e) => (
                <li key={e.event_id}>
                  {e.issue_id ? (
                    <Link
                      to="/$org/issues/$issueId"
                      params={{ org, issueId: e.issue_id }}
                      className="flex items-center gap-2 rounded-md border px-2 py-1.5 text-[13px] hover:bg-row-hover"
                    >
                      <LevelDot level={e.level} />
                      <span className="min-w-0 flex-1 truncate">{e.title}</span>
                      {e.short_id && <span className="font-mono text-[11px] text-muted-foreground">{e.short_id}</span>}
                      <ArrowUpRight className="size-3.5 text-muted-foreground" />
                    </Link>
                  ) : (
                    <span className="flex items-center gap-2 text-[13px]">
                      <LevelDot level={e.level} /> {e.title}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          </Block>
        )}
        {logs.length > 0 && (
          <Block title={`Logs (${logs.length})`}>
            <ul className="space-y-1 font-mono text-[12px]">
              {logs.slice(0, 50).map((l, i) => (
                <li key={i} className="flex items-start gap-2">
                  <span className={cn('mt-1.5 size-1.5 shrink-0 rounded-full', LOG_LEVEL_DOT[l.level as 'info'] ?? 'bg-level-info')} />
                  <span className="shrink-0 text-muted-foreground tabular">+{ms(l.timestamp - traceStart)}</span>
                  <span className="min-w-0 break-words">{l.body}</span>
                </li>
              ))}
            </ul>
          </Block>
        )}
        <Block title="Span">
          <KeyValueTable
            rows={[
              [
                'span id',
                <span key="s" className="inline-flex items-center gap-1">
                  {s.span_id}
                  <CopyButton value={s.span_id} label="Copy span id" />
                </span>,
              ],
              [
                'parent',
                s.parent_span_id ? (
                  <button key="p" type="button" className="text-brand-text hover:underline" onClick={() => onSelect(s.parent_span_id!)}>
                    {s.parent_span_id}
                  </button>
                ) : (
                  '—'
                ),
              ],
              ['project', projectName(s.project_id) ?? '—'],
              ['start', absolute(s.start, { ms: true })],
              ['end', absolute(s.start + s.duration_ms, { ms: true })],
              ['children', String(node.children.length)],
            ]}
          />
        </Block>
        {data.length > 0 && (
          <Block title="Data">
            <KeyValueTable rows={data.map(([k, v]) => [k, typeof v === 'object' && v !== null ? <ValueView key={k} value={v} /> : String(v)])} />
          </Block>
        )}
        {tags.length > 0 && (
          <Block title="Tags">
            <KeyValueTable rows={tags} />
          </Block>
        )}
      </div>
    </aside>
  )
}
