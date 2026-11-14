import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ChevronRight, FileText, Film, Image, Link2, MessageSquareText, Paperclip, Route as RouteIcon, TriangleAlert } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { api } from '@/api/endpoints'
import { qk } from '@/api/queries'
import type { Issue, IssueDetail, RuptaEvent } from '@/api/types'
import { CopyButton } from '@/components/common/copy-button'
import { KeyValueTable } from '@/components/common/page'
import { RelativeTime } from '@/components/common/time'
import { Badge } from '@/components/ui/badge'
import { Tip } from '@/components/ui/tooltip'
import { bytes } from '@/lib/format'
import { formatFilter } from '@/lib/search/parse'
import { cn } from '@/lib/utils'
import { Breadcrumbs } from './breadcrumbs'
import { Contexts } from './contexts'
import { RequestSection } from './request'
import { ExceptionBlock, rawStacktrace, StacktraceToolbar, type StacktraceOptions } from './stacktrace'
import { ValueView } from './value-view'

export function EventSection({
  id,
  title,
  children,
  actions,
  defaultOpen = true,
  count,
}: {
  id: string
  title: string
  children: ReactNode
  actions?: ReactNode
  defaultOpen?: boolean
  count?: number
}) {
  const [open, setOpen] = useState(defaultOpen)
  return (
    <section id={id} className="scroll-mt-28 border-b px-4 py-4 last:border-b-0 sm:px-6" aria-labelledby={`${id}-title`}>
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          className="group flex items-center gap-1.5 rounded-sm text-left focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          aria-expanded={open}
        >
          <ChevronRight className={cn('size-3.5 text-muted-foreground transition-transform', open && 'rotate-90')} />
          <h2 id={`${id}-title`} className="text-[13px] font-semibold">
            {title}
          </h2>
          {count !== undefined && <span className="text-xs text-muted-foreground tabular">{count}</span>}
        </button>
        <a
          href={`#${id}`}
          className="text-muted-foreground/60 opacity-0 transition-opacity hover:text-foreground focus-visible:opacity-100 [section:hover_&]:opacity-100"
          aria-label={`Link to ${title}`}
        >
          <Link2 className="size-3.5" />
        </a>
        {open && actions && <div className="ml-auto flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {open && children}
    </section>
  )
}

const HIGHLIGHT_KEYS = ['environment', 'release', 'level', 'handled', 'browser', 'os', 'device', 'runtime', 'server_name', 'transaction', 'url', 'user']

function Highlights({ event, org }: { event: RuptaEvent; org: string }) {
  const tags = new Map(event.tags)
  const items = HIGHLIGHT_KEYS.filter((k) => tags.has(k)).map((k) => [k, tags.get(k)!] as const)
  return (
    <dl className="grid grid-cols-2 overflow-hidden rounded-lg border bg-card sm:grid-cols-3 lg:grid-cols-4">
      {items.map(([k, v]) => (
        <div key={k} className="-mt-px -ml-px min-w-0 border-t border-l px-3 py-2">
          <dt className="text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{k.replace('_', ' ')}</dt>
          <dd className="mt-0.5 flex min-w-0 items-center gap-1 text-[13px]">
            {k === 'release' ? (
              <Link to="/$org/releases/$version" params={{ org, version: v }} className="truncate font-mono text-brand-text hover:underline">
                {v}
              </Link>
            ) : k === 'handled' ? (
              <Badge variant={v === 'yes' ? 'secondary' : 'destructive'}>{v === 'yes' ? 'yes' : 'no'}</Badge>
            ) : k === 'url' ? (
              <a href={v} target="_blank" rel="noreferrer noopener" className="truncate font-mono text-[12.5px] hover:underline" title={v}>
                {v.replace(/^https?:\/\//, '')}
              </a>
            ) : (
              <Link
                to="/$org/issues"
                params={{ org }}
                search={{ query: `is:unresolved ${formatFilter(k, v)}` }}
                className="truncate hover:underline"
                title={`Search issues with ${k}:${v}`}
              >
                {v}
              </Link>
            )}
          </dd>
        </div>
      ))}
    </dl>
  )
}

/** Links from the event to its trace and session replay. */
function Related({ event, org }: { event: RuptaEvent; org: string }) {
  const trace = (event.trace_id ?? (event.contexts?.trace?.trace_id as string | undefined)) || null
  const span = (event.span_id ?? (event.contexts?.trace?.span_id as string | undefined)) || undefined
  const replay = (event.replay_id ?? (event.contexts?.replay?.replay_id as string | undefined)) || null
  if (!trace && !replay) return null
  return (
    <div className="mt-3 flex flex-wrap gap-2" aria-label="Related telemetry">
      {trace && (
        <Link
          to="/$org/traces/$traceId"
          params={{ org, traceId: trace }}
          search={span ? { span } : {}}
          className="inline-flex items-center gap-2 rounded-md border bg-card px-2.5 py-1.5 text-[13px] hover:bg-row-hover"
        >
          <RouteIcon className="size-4 text-muted-foreground" />
          <span>Trace</span>
          <span className="font-mono text-xs text-muted-foreground">{trace.slice(0, 8)}</span>
          <ChevronRight className="size-3.5 text-muted-foreground" />
        </Link>
      )}
      {replay && (
        <Link
          to="/$org/replays/$project/$replayId"
          params={{ org, project: String(event.project_id), replayId: replay }}
          className="inline-flex items-center gap-2 rounded-md border bg-card px-2.5 py-1.5 text-[13px] hover:bg-row-hover"
        >
          <Film className="size-4 text-muted-foreground" />
          <span>Session replay</span>
          <ChevronRight className="size-3.5 text-muted-foreground" />
        </Link>
      )}
    </div>
  )
}

function TagChips({ tags, org }: { tags: [string, string][]; org: string }) {
  return (
    <div className="flex flex-wrap gap-1.5">
      {tags.map(([k, v]) => (
        <span key={`${k}=${v}`} className="inline-flex max-w-full min-w-0 items-stretch overflow-hidden rounded-md border text-[12px]">
          <span className="bg-muted px-1.5 py-0.5 text-muted-foreground">{k}</span>
          <Link
            to="/$org/issues"
            params={{ org }}
            search={{ query: `is:unresolved ${formatFilter(k, v)}` }}
            className="truncate px-1.5 py-0.5 font-mono hover:bg-row-hover"
            title={`Search issues with ${k}:${v}`}
          >
            {v}
          </Link>
        </span>
      ))}
    </div>
  )
}

function Attachments({ issueId, eventId }: { issueId: string; eventId: string }) {
  const q = useQuery({ queryKey: qk.issues.attachments(issueId), queryFn: () => api.issues.attachments(issueId) })
  const list = (q.data ?? []).filter((a) => a.event_id === eventId)
  if (!list.length) return null
  return (
    <EventSection id="attachments" title="Attachments" count={list.length}>
      <ul className="grid gap-2 sm:grid-cols-2">
        {list.map((a) => (
          <li key={a.id} className="flex items-center gap-3 rounded-lg border bg-card px-3 py-2">
            <span className="flex size-8 items-center justify-center rounded-md bg-muted text-muted-foreground">
              {a.content_type?.startsWith('image/') ? <Image className="size-4" /> : <FileText className="size-4" />}
            </span>
            <div className="min-w-0 flex-1">
              <a href={a.download_url ?? api.issues.attachmentUrl(a.id)} className="block truncate text-[13px] font-medium hover:underline" download>
                {a.name}
              </a>
              <div className="text-xs text-muted-foreground">
                {a.content_type ?? 'unknown type'} · {bytes(a.size)}
              </div>
            </div>
            <Paperclip className="size-3.5 text-muted-foreground" />
          </li>
        ))}
      </ul>
    </EventSection>
  )
}

const ERROR_TEXT: Record<string, string> = {
  js_no_source: 'The source file could not be fetched, so this frame is not source mapped.',
  js_invalid_source: 'The source map for this file is invalid.',
  js_missing_source_content: 'The source map has no source content for this file.',
  invalid_data: 'Some data was invalid and was removed.',
  value_too_long: 'A value was longer than allowed and was truncated.',
  past_timestamp: 'The event timestamp was too far in the past and was adjusted.',
}

export function EventView({ event, issue, org }: { event: RuptaEvent; issue: Issue | IssueDetail; org: string }) {
  const [opts, setOpts] = useState<StacktraceOptions>({ inAppOnly: true, newestFirst: true, view: 'symbolicated', expandAll: false })
  const values = event.exception?.values ?? []
  const ordered = opts.newestFirst ? [...values].reverse() : values
  const hasMinified = values.some((v) => v.stacktrace?.frames.some((f) => f.raw))
  const chained = values.length > 1
  const crumbs = event.breadcrumbs?.values ?? []
  const raw = opts.view === 'raw' ? rawStacktrace(values, event.platform) : ''

  const nav: [string, string][] = [['highlights', 'Highlights']]
  if (values.length) nav.push(['exception', 'Stack trace'])
  if (crumbs.length) nav.push(['breadcrumbs', 'Breadcrumbs'])
  if (event.request) nav.push(['request', 'Request'])
  nav.push(['contexts', 'Contexts'], ['tags', 'Tags'])
  if (event.extra && Object.keys(event.extra).length) nav.push(['extra', 'Extra'])

  return (
    <div>
      <nav aria-label="Event sections" className="sticky top-0 z-10 flex gap-1 overflow-x-auto border-b bg-background/95 px-4 py-1.5 backdrop-blur sm:px-6">
        {nav.map(([id, label]) => (
          <a key={id} href={`#${id}`} className="shrink-0 rounded-md px-2 py-1 text-xs font-medium text-muted-foreground hover:bg-accent hover:text-foreground">
            {label}
          </a>
        ))}
      </nav>

      <EventSection id="highlights" title="Highlights">
        <Highlights event={event} org={org} />
        <Related event={event} org={org} />
        {event.errors && event.errors.length > 0 && (
          <div className="mt-3 flex gap-2.5 rounded-lg border border-warning/40 bg-warning-soft px-3 py-2 text-[13px]" role="note">
            <TriangleAlert className="mt-0.5 size-4 shrink-0 text-warning" />
            <div>
              <p className="font-medium">There {event.errors.length === 1 ? 'was 1 problem' : `were ${event.errors.length} problems`} processing this event</p>
              <ul className="mt-1 space-y-0.5 text-muted-foreground">
                {event.errors.map((e, i) => (
                  <li key={i}>
                    {ERROR_TEXT[e.type] ?? e.type}
                    {e.value !== undefined && <span className="font-mono text-xs"> ({String(e.value)})</span>}
                  </li>
                ))}
              </ul>
            </div>
          </div>
        )}
      </EventSection>

      {event.message && !values.length && (
        <EventSection id="message" title="Message">
          <pre className="overflow-x-auto rounded-lg border bg-code-bg p-3 font-mono text-[13px] whitespace-pre-wrap">{event.message}</pre>
        </EventSection>
      )}

      {values.length > 0 && (
        <EventSection
          id="exception"
          title={chained ? `Exceptions (${values.length} chained)` : 'Stack trace'}
          actions={<StacktraceToolbar opts={opts} onChange={(o) => setOpts((p) => ({ ...p, ...o }))} hasMinified={hasMinified} chained={chained} />}
        >
          {opts.view === 'raw' ? (
            <div className="relative">
              <div className="absolute top-2 right-2">
                <CopyButton value={raw} label="Copy stack trace" toastLabel="Stack trace copied" />
              </div>
              <pre className="overflow-x-auto rounded-lg border bg-code-bg p-3 pr-10 font-mono text-[12.5px] leading-relaxed">{raw}</pre>
            </div>
          ) : (
            <div className="space-y-3">
              {ordered.map((ex, i) => {
                const original = opts.newestFirst ? values.length - 1 - i : i
                const label = !chained
                  ? undefined
                  : opts.newestFirst
                    ? i > 0
                      ? 'Caused by'
                      : undefined
                    : i > 0
                      ? 'The above exception was the direct cause of'
                      : 'Root cause'
                return <ExceptionBlock key={original} ex={ex} index={original} platform={event.platform} opts={opts} label={label} />
              })}
            </div>
          )}
        </EventSection>
      )}

      {crumbs.length > 0 && (
        <EventSection id="breadcrumbs" title="Breadcrumbs" count={crumbs.length}>
          <Breadcrumbs crumbs={crumbs} eventTime={event.timestamp} />
        </EventSection>
      )}

      {event.request && (
        <EventSection id="request" title="Request">
          <RequestSection req={event.request} />
        </EventSection>
      )}

      <EventSection id="contexts" title="Contexts">
        <Contexts contexts={event.contexts ?? {}} user={event.user} />
      </EventSection>

      <EventSection
        id="tags"
        title="Tags"
        count={event.tags.length}
        actions={
          <Link to="/$org/issues/$issueId/tags" params={{ org, issueId: issue.id }} className="text-xs font-medium text-brand-text hover:underline">
            See distributions
          </Link>
        }
      >
        <TagChips tags={event.tags} org={org} />
      </EventSection>

      {event.extra && Object.keys(event.extra).length > 0 && (
        <EventSection id="extra" title="Additional data">
          <KeyValueTable rows={Object.entries(event.extra).map(([k, v]) => [k, <ValueView key={k} value={v} />])} mono={false} />
        </EventSection>
      )}

      <Attachments issueId={issue.id} eventId={event.event_id} />

      {event.modules && Object.keys(event.modules).length > 0 && (
        <EventSection id="modules" title="Packages" count={Object.keys(event.modules).length} defaultOpen={false}>
          <KeyValueTable rows={Object.entries(event.modules)} />
        </EventSection>
      )}

      {event.sdk && (
        <EventSection id="sdk" title="SDK" defaultOpen={false}>
          <KeyValueTable
            rows={[
              ['Name', event.sdk.name],
              ['Version', event.sdk.version],
              ...(event.sdk.integrations?.length ? ([['Integrations', event.sdk.integrations.join(', ')]] as [string, string][]) : []),
              ...(event.sdk.packages ?? []).map((p) => [`Package`, `${p.name}@${p.version}`] as [string, string]),
            ]}
          />
        </EventSection>
      )}

      <section className="flex flex-wrap items-center gap-x-4 gap-y-1 px-4 py-4 text-xs text-muted-foreground sm:px-6">
        <span>
          Occurred <RelativeTime date={event.timestamp} long />
        </span>
        <Tip content="Time between the event and when the server received it">
          <span>Received {Math.max(0, Math.round((new Date(event.received).getTime() - new Date(event.timestamp).getTime()) / 100) / 10)}s later</span>
        </Tip>
        {event.fingerprint.length > 0 && <span className="font-mono">fingerprint: {event.fingerprint.join(', ')}</span>}
        <Link to="/$org/issues/$issueId/feedback" params={{ org, issueId: issue.id }} className="inline-flex items-center gap-1 hover:text-foreground">
          <MessageSquareText className="size-3.5" /> User feedback
        </Link>
      </section>
    </div>
  )
}
