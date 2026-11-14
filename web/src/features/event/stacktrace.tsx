import { ChevronRight, CircleCheck, ExternalLink, ShieldAlert } from 'lucide-react'
import { useMemo, useState } from 'react'
import type { ExceptionValue, Frame } from '@/api/types'
import { CopyButton } from '@/components/common/copy-button'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tip } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { detectLang, useHighlight } from './highlight'
import { ValueView } from './value-view'

export type FrameView = 'symbolicated' | 'minified' | 'raw'

export interface StacktraceOptions {
  inAppOnly: boolean
  newestFirst: boolean
  view: FrameView
  expandAll: boolean
}

/** The frame to show: symbolicated, or its raw (minified) location. */
export function displayFrame(f: Frame, view: FrameView): Frame {
  if (view === 'minified' && f.raw) return { ...f, ...f.raw, in_app: f.in_app, raw: null, symbolicated: false, vars: null }
  return f
}

export function frameLocation(f: Frame): string {
  const file = f.filename || f.abs_path || f.module || '?'
  return f.lineno ? `${file}:${f.lineno}${f.colno ? `:${f.colno}` : ''}` : file
}

/** Plain-text stack trace in the style of the platform. */
export function rawStacktrace(values: ExceptionValue[], platform: string, view: FrameView = 'symbolicated'): string {
  const parts: string[] = []
  for (const ex of values) {
    const frames = (ex.stacktrace?.frames ?? []).map((f) => displayFrame(f, view === 'raw' ? 'symbolicated' : view))
    const head = `${ex.type ?? 'Error'}${ex.value ? `: ${ex.value}` : ''}`
    if (platform === 'python') {
      parts.push(
        [
          'Traceback (most recent call last):',
          ...frames.map(
            (f) => `  File "${f.abs_path ?? f.filename}", line ${f.lineno}, in ${f.function}${f.context_line ? `\n    ${f.context_line.trim()}` : ''}`,
          ),
          head,
        ].join('\n'),
      )
    } else if (platform === 'php') {
      parts.push([head, ...[...frames].reverse().map((f, i) => `#${i} ${f.abs_path ?? f.filename}(${f.lineno}): ${f.function}()`)].join('\n'))
    } else if (platform === 'go') {
      parts.push([head, '', ...[...frames].reverse().flatMap((f) => [`${f.function}()`, `\t${f.abs_path ?? f.filename}:${f.lineno}`])].join('\n'))
    } else {
      parts.push(
        [
          head,
          ...[...frames].reverse().map((f) => `    at ${f.function ?? '<anonymous>'} (${f.abs_path ?? f.filename}:${f.lineno ?? 0}:${f.colno ?? 0})`),
        ].join('\n'),
      )
    }
  }
  return parts.join(platform === 'python' ? '\n\nThe above exception was the direct cause of the following exception:\n\n' : '\n\nCaused by: ')
}

const EMPTY: string[] = []

function SourceContext({ frame, platform }: { frame: Frame; platform: string }) {
  const pre = frame.pre_context ?? EMPTY
  const post = frame.post_context ?? EMPTY
  const lines = useMemo(() => [...pre, frame.context_line ?? '', ...post], [pre, post, frame.context_line])
  const lang = detectLang(frame.filename ?? frame.abs_path, frame.platform ?? platform)
  const tokens = useHighlight(lines, lang)
  const start = (frame.lineno ?? pre.length + 1) - pre.length
  if (!frame.context_line) return <p className="px-3 py-2 text-xs text-muted-foreground">No source code available for this frame.</p>
  return (
    <div className="overflow-x-auto bg-code-bg">
      <table className="shiki-code w-full border-collapse font-mono text-[12.5px] leading-[1.6]">
        <tbody>
          {lines.map((line, i) => {
            const n = start + i
            const isErr = i === pre.length
            return (
              <tr key={n} className={cn(isErr && 'bg-code-line-hl')} aria-current={isErr ? 'true' : undefined}>
                <td
                  className={cn(
                    'w-12 border-r px-2 text-right align-top text-muted-foreground/70 tabular select-none',
                    isErr && 'font-semibold text-foreground',
                  )}
                >
                  {n}
                </td>
                <td className="px-3 whitespace-pre">
                  {tokens?.[i]
                    ? tokens[i].map((t, j) => (
                        <span key={j} style={{ '--shiki-light': t.light, '--shiki-dark': t.dark } as React.CSSProperties}>
                          {t.content}
                        </span>
                      ))
                    : line || ' '}
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

function FrameVars({ vars }: { vars: Record<string, unknown> }) {
  const entries = Object.entries(vars)
  return (
    <div className="border-t">
      <table className="w-full table-fixed text-[12.5px]">
        <caption className="sr-only">Local variables</caption>
        <tbody className="divide-y">
          {entries.map(([k, v]) => (
            <tr key={k} className="align-top">
              <th scope="row" className="w-1/4 truncate px-3 py-1 text-left font-mono font-normal text-muted-foreground">
                {k}
              </th>
              <td className="px-3 py-1">
                <ValueView value={v} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function FrameRow({
  frame,
  platform,
  defaultOpen,
  anchor,
  isCrash,
}: {
  frame: Frame
  platform: string
  defaultOpen: boolean
  anchor: string
  isCrash: boolean
}) {
  const [open, setOpen] = useState(defaultOpen)
  const hasDetails = Boolean(frame.context_line || (frame.vars && Object.keys(frame.vars).length))
  const file = frame.filename || frame.abs_path || frame.module || '<unknown>'
  const link = `${location.origin}${location.pathname}${location.search}#${anchor}`
  return (
    <li id={anchor} className={cn('scroll-mt-20', !frame.in_app && 'text-muted-foreground')}>
      <div
        className={cn(
          'group flex min-h-8 items-center gap-2 px-2 py-1 text-[12.5px] hover:bg-row-hover',
          hasDetails && 'cursor-pointer',
          open && 'bg-muted/50',
        )}
        onClick={() => hasDetails && setOpen((o) => !o)}
      >
        <button
          type="button"
          className={cn('flex size-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground', !hasDetails && 'invisible')}
          aria-expanded={open}
          aria-label={`${open ? 'Collapse' : 'Expand'} frame ${file}`}
          onClick={(e) => {
            e.stopPropagation()
            setOpen((o) => !o)
          }}
        >
          <ChevronRight className={cn('size-3.5 transition-transform', open && 'rotate-90')} />
        </button>
        <div className="min-w-0 flex-1 font-mono leading-5">
          <span className={cn('break-all', frame.in_app && 'font-medium text-foreground')}>{file}</span>
          {frame.function && (
            <>
              <span className="text-muted-foreground"> in </span>
              <span className={cn('break-all', frame.in_app && 'text-foreground')}>{frame.function}</span>
            </>
          )}
          {frame.lineno != null && (
            <>
              <span className="text-muted-foreground"> at line </span>
              <span className="tabular">
                {frame.lineno}
                {frame.colno != null && `:${frame.colno}`}
              </span>
            </>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {isCrash && (
            <Badge variant="destructive" className="hidden sm:inline-flex">
              crashed here
            </Badge>
          )}
          {frame.symbolicated && (
            <Tip content={frame.raw ? `Mapped from ${frameLocation(frame.raw)}` : 'Mapped with an uploaded source map'}>
              <Badge variant="success">
                <CircleCheck /> source mapped
              </Badge>
            </Tip>
          )}
          {frame.in_app ? <Badge variant="brand">in app</Badge> : null}
          <span className="opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100" onClick={(e) => e.stopPropagation()}>
            <CopyButton value={link} label="Copy link to frame" toastLabel="Link copied" />
          </span>
        </div>
      </div>
      {open && hasDetails && (
        <div className="mx-2 mb-2 overflow-hidden rounded-md border">
          <SourceContext frame={frame} platform={platform} />
          {frame.vars && Object.keys(frame.vars).length > 0 && <FrameVars vars={frame.vars} />}
        </div>
      )}
    </li>
  )
}

function CollapsedFrames({ count, onExpand }: { count: number; onExpand: () => void }) {
  return (
    <li>
      <button
        type="button"
        onClick={onExpand}
        className="flex w-full items-center gap-2 px-3 py-1 text-left text-xs text-muted-foreground hover:bg-row-hover hover:text-foreground"
      >
        <span className="h-px w-3 bg-border" /> {count} library {count === 1 ? 'frame' : 'frames'} hidden · show
      </button>
    </li>
  )
}

export function StacktraceFrames({ frames, platform, opts, exIndex }: { frames: Frame[]; platform: string; opts: StacktraceOptions; exIndex: number }) {
  const [revealed, setRevealed] = useState<Set<number>>(new Set())
  const shown = frames.map((f, i) => ({ f: displayFrame(f, opts.view), i }))
  const ordered = opts.newestFirst ? [...shown].reverse() : shown
  const hasInApp = frames.some((f) => f.in_app)
  const crashIndex = (() => {
    for (let i = frames.length - 1; i >= 0; i--) if (frames[i].in_app) return i
    return frames.length - 1
  })()

  // Group consecutive library frames when "in app only" is on.
  const rows: React.ReactNode[] = []
  let hidden: number[] = []
  const flush = () => {
    if (hidden.length) {
      const group = hidden
      rows.push(<CollapsedFrames key={`c-${group[0]}`} count={group.length} onExpand={() => setRevealed((r) => new Set([...r, ...group]))} />)
      hidden = []
    }
  }
  for (const { f, i } of ordered) {
    const visible = !opts.inAppOnly || !hasInApp || f.in_app || revealed.has(i)
    if (!visible) {
      hidden.push(i)
      continue
    }
    flush()
    rows.push(
      <FrameRow
        key={`${opts.view}-${opts.expandAll}-${i}`}
        frame={f}
        platform={platform}
        defaultOpen={opts.expandAll || i === crashIndex}
        anchor={`frame-${exIndex}-${i}`}
        isCrash={i === crashIndex && f.in_app === true}
      />,
    )
  }
  flush()
  return <ol className="divide-y">{rows}</ol>
}

export function ExceptionBlock({
  ex,
  index,
  platform,
  opts,
  label,
}: {
  ex: ExceptionValue
  index: number
  platform: string
  opts: StacktraceOptions
  label?: string
}) {
  const frames = ex.stacktrace?.frames ?? []
  const handled = ex.mechanism?.handled
  return (
    <div className="overflow-hidden rounded-lg border bg-card" id={`exception-${index}`}>
      <div className="space-y-1 border-b px-3 py-2.5">
        <div className="flex flex-wrap items-center gap-2">
          {label && <span className="w-full text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{label}</span>}
          <h3 className="font-mono text-[13.5px] font-semibold break-all">{ex.type ?? 'Error'}</h3>
          {ex.module && <span className="font-mono text-xs text-muted-foreground">{ex.module}</span>}
          {ex.mechanism && (
            <span className="ml-auto flex items-center gap-1.5">
              <Badge variant="outline" className="font-mono">
                mechanism: {ex.mechanism.type}
              </Badge>
              {handled !== undefined && (
                <Badge variant={handled ? 'secondary' : 'destructive'}>
                  {!handled && <ShieldAlert />} {handled ? 'handled' : 'unhandled'}
                </Badge>
              )}
              {ex.mechanism.help_link && (
                <a
                  href={ex.mechanism.help_link}
                  target="_blank"
                  rel="noreferrer noopener"
                  className="text-muted-foreground hover:text-foreground"
                  aria-label="Help for this error"
                >
                  <ExternalLink className="size-3.5" />
                </a>
              )}
            </span>
          )}
        </div>
        {ex.value && <p className="font-mono text-[13px] break-words whitespace-pre-wrap text-foreground/85">{ex.value}</p>}
      </div>
      {frames.length > 0 ? (
        <StacktraceFrames frames={frames} platform={platform} opts={opts} exIndex={index} />
      ) : (
        <p className="px-3 py-3 text-xs text-muted-foreground">The SDK sent no stack trace for this exception.</p>
      )}
    </div>
  )
}

export function StacktraceToolbar({
  opts,
  onChange,
  hasMinified,
  chained,
}: {
  opts: StacktraceOptions
  onChange: (o: Partial<StacktraceOptions>) => void
  hasMinified: boolean
  chained: boolean
}) {
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
      <div className="flex items-center gap-2">
        <Switch id="in-app-only" checked={opts.inAppOnly} onCheckedChange={(v) => onChange({ inAppOnly: v })} />
        <Label htmlFor="in-app-only" className="font-normal">
          In-app frames only
        </Label>
      </div>
      <ToggleGroup type="single" value={opts.view} onValueChange={(v) => v && onChange({ view: v as FrameView })} aria-label="Frame display">
        <ToggleGroupItem value="symbolicated">{hasMinified ? 'Symbolicated' : 'Frames'}</ToggleGroupItem>
        {hasMinified && <ToggleGroupItem value="minified">Minified</ToggleGroupItem>}
        <ToggleGroupItem value="raw">Raw text</ToggleGroupItem>
      </ToggleGroup>
      <ToggleGroup
        type="single"
        value={opts.newestFirst ? 'newest' : 'oldest'}
        onValueChange={(v) => v && onChange({ newestFirst: v === 'newest' })}
        aria-label={chained ? 'Exception and frame order' : 'Frame order'}
      >
        <ToggleGroupItem value="newest">Newest first</ToggleGroupItem>
        <ToggleGroupItem value="oldest">Oldest first</ToggleGroupItem>
      </ToggleGroup>
      {opts.view !== 'raw' && (
        <Button
          variant="ghost"
          size="xs"
          onClick={() => onChange(opts.expandAll ? { expandAll: false, inAppOnly: true } : { expandAll: true, inAppOnly: false })}
        >
          {opts.expandAll ? 'Collapse frames' : 'Full stack'}
        </Button>
      )}
    </div>
  )
}
