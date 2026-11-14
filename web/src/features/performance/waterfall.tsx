// The trace waterfall: a virtualised span tree on the left, bars on a
// zoomable time axis on the right, a minimap to pick the visible window,
// and markers for errors and log lines.

import { useVirtualizer } from '@tanstack/react-virtual'
import { ChevronRight, CircleAlert } from 'lucide-react'
import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import type { TraceError, TraceLogMarker } from '@/api/telemetry-types'
import { useTheme } from '@/components/theme-provider'
import { cn } from '@/lib/utils'
import { LOG_LEVEL_VAR, ms, opColor } from '@/features/telemetry/format'
import { opCategory, timeTicks, type FlatRow } from './span-tree'

export const ROW_HEIGHT = 28

export interface ViewWindow {
  /** Offsets from the trace start, in milliseconds. */
  start: number
  end: number
}

interface Props {
  rows: FlatRow[]
  traceStart: number
  traceDuration: number
  view: ViewWindow
  onView: (v: ViewWindow) => void
  selectedId: string | null
  onSelect: (id: string, open?: boolean) => void
  onToggle: (id: string) => void
  critical: Set<string> | null
  matches: Set<string>
  errorsBySpan: Map<string, TraceError[]>
  logsBySpan: Map<string, TraceLogMarker[]>
  projectName: (id: string | null) => string | null
  scrollToIndex: number | null
}

const MIN_WINDOW = 0.05 // ms

export function clampView(v: ViewWindow, total: number): ViewWindow {
  let { start, end } = v
  const width = Math.max(MIN_WINDOW, Math.min(total, end - start))
  start = Math.max(0, Math.min(start, total - width))
  end = start + width
  return { start, end }
}

/** Zooms the window by `factor` (<1 zooms in) around `anchor` (0..1 of the window). */
export function zoomView(v: ViewWindow, factor: number, anchor: number, total: number): ViewWindow {
  const width = v.end - v.start
  const next = Math.max(MIN_WINDOW, Math.min(total, width * factor))
  const at = v.start + width * anchor
  return clampView({ start: at - next * anchor, end: at - next * anchor + next }, total)
}

function useElementWidth<T extends HTMLElement>() {
  const ref = useRef<T>(null)
  const [width, setWidth] = useState(800)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    setWidth(el.clientWidth || 800)
    const ro = new ResizeObserver((entries) => setWidth(entries[0]?.contentRect.width || el.clientWidth || 800))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  return [ref, width] as const
}

/** Minimap: every span as a thin line, the visible window as a draggable frame. */
export function Minimap({ rows, traceStart, traceDuration, view, onView }: Pick<Props, 'rows' | 'traceStart' | 'traceDuration' | 'view' | 'onView'>) {
  const [wrapRef, width] = useElementWidth<HTMLDivElement>()
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const { resolved } = useTheme()
  const height = 44
  const drag = useRef<{ mode: 'move' | 'select'; x0: number; view0: ViewWindow } | null>(null)
  const [selecting, setSelecting] = useState<{ a: number; b: number } | null>(null)

  useEffect(() => {
    const c = canvasRef.current
    const ctx = c?.getContext?.('2d')
    if (!c || !ctx) return
    const dpr = window.devicePixelRatio || 1
    c.width = width * dpr
    c.height = height * dpr
    ctx.scale(dpr, dpr)
    ctx.clearRect(0, 0, width, height)
    const css = getComputedStyle(c)
    const n = rows.length
    const rowH = Math.max(0.5, Math.min(3, (height - 4) / Math.max(1, n)))
    const step = n * rowH > height - 4 ? (height - 4) / n : rowH
    rows.forEach((r, i) => {
      const s = r.node.span
      const x = ((s.start - traceStart) / traceDuration) * width
      const w = Math.max(1, (s.duration_ms / traceDuration) * width)
      const color = opColor(opCategory(s.op))
      ctx.fillStyle = color.startsWith('var(') ? css.getPropertyValue(color.slice(4, -1)).trim() || '#999' : color
      ctx.fillRect(x, 2 + i * step, w, Math.max(0.6, step - (step > 1.5 ? 0.4 : 0)))
    })
  }, [rows, traceStart, traceDuration, width, resolved])

  const toMs = (clientX: number) => {
    const rect = wrapRef.current!.getBoundingClientRect()
    return Math.max(0, Math.min(traceDuration, ((clientX - rect.left) / rect.width) * traceDuration))
  }
  const onDown = (e: ReactPointerEvent) => {
    if (e.button !== 0) return
    const t = toMs(e.clientX)
    const inside = t >= view.start && t <= view.end && view.end - view.start < traceDuration * 0.98
    drag.current = { mode: inside ? 'move' : 'select', x0: t, view0: view }
    ;(e.target as Element).setPointerCapture?.(e.pointerId)
    if (!inside) setSelecting({ a: t, b: t })
  }
  const onMove = (e: ReactPointerEvent) => {
    const d = drag.current
    if (!d) return
    const t = toMs(e.clientX)
    if (d.mode === 'move') {
      const delta = t - d.x0
      onView(clampView({ start: d.view0.start + delta, end: d.view0.end + delta }, traceDuration))
    } else setSelecting({ a: d.x0, b: t })
  }
  const onUp = (e: ReactPointerEvent) => {
    const d = drag.current
    drag.current = null
    if (d?.mode === 'select') {
      const t = toMs(e.clientX)
      const [a, b] = [Math.min(d.x0, t), Math.max(d.x0, t)]
      // A click (no drag) centres the current window there.
      if (b - a < traceDuration * 0.004) {
        const w = view.end - view.start
        onView(clampView({ start: a - w / 2, end: a + w / 2 }, traceDuration))
      } else onView(clampView({ start: a, end: b }, traceDuration))
    }
    setSelecting(null)
  }
  const pct = (v: number) => `${(v / traceDuration) * 100}%`
  const zoomed = view.end - view.start < traceDuration * 0.999
  return (
    <div
      ref={wrapRef}
      className="relative h-11 cursor-crosshair touch-none overflow-hidden rounded-md border bg-card select-none"
      onPointerDown={onDown}
      onPointerMove={onMove}
      onPointerUp={onUp}
      onDoubleClick={() => onView({ start: 0, end: traceDuration })}
      role="slider"
      aria-label="Visible part of the trace"
      aria-valuemin={0}
      aria-valuemax={Math.round(traceDuration)}
      aria-valuenow={Math.round(view.start)}
      aria-valuetext={`${ms(view.start)} to ${ms(view.end)}`}
      tabIndex={0}
      onKeyDown={(e) => {
        const w = view.end - view.start
        if (e.key === 'ArrowLeft') onView(clampView({ start: view.start - w * 0.2, end: view.end - w * 0.2 }, traceDuration))
        else if (e.key === 'ArrowRight') onView(clampView({ start: view.start + w * 0.2, end: view.end + w * 0.2 }, traceDuration))
        else return
        e.preventDefault()
      }}
    >
      <canvas ref={canvasRef} style={{ width, height }} className="absolute inset-0" aria-hidden />
      {zoomed && (
        <>
          <div className="absolute inset-y-0 left-0 bg-background/70" style={{ width: pct(view.start) }} />
          <div className="absolute inset-y-0 right-0 bg-background/70" style={{ width: `calc(100% - ${pct(view.end)})` }} />
          <div className="absolute inset-y-0 cursor-grab rounded-sm border-2 border-brand/80 active:cursor-grabbing" style={{ left: pct(view.start), width: `max(4px, calc(${pct(view.end)} - ${pct(view.start)}))` }} />
        </>
      )}
      {selecting && <div className="absolute inset-y-0 bg-brand/15 ring-1 ring-brand/50" style={{ left: pct(Math.min(selecting.a, selecting.b)), width: pct(Math.abs(selecting.b - selecting.a)) }} />}
    </div>
  )
}

export function TimeAxis({ view, className }: { view: ViewWindow; className?: string }) {
  const width = view.end - view.start
  const ticks = timeTicks(width, 6)
  return (
    <div className={cn('relative h-full', className)} aria-hidden>
      {ticks.filter((t) => t / width < 0.94).map((t) => (
        <span key={t} className="absolute top-0 flex h-full items-center border-l border-border pl-1 text-[10.5px] whitespace-nowrap text-muted-foreground tabular" style={{ left: `${(t / width) * 100}%` }}>
          {ms(view.start + t)}
        </span>
      ))}
    </div>
  )
}

const Row = memo(function Row({
  row,
  index,
  selected,
  matched,
  critical,
  traceStart,
  view,
  errors,
  logs,
  leftWidth,
  onSelect,
  onToggle,
  projectName,
}: {
  row: FlatRow
  index: number
  selected: boolean
  matched: boolean
  critical: boolean | null
  traceStart: number
  view: ViewWindow
  errors: TraceError[] | undefined
  logs: TraceLogMarker[] | undefined
  leftWidth: number
  onSelect: (id: string, open?: boolean) => void
  onToggle: (id: string) => void
  projectName: (id: string | null) => string | null
}) {
  const s = row.node.span
  const width = view.end - view.start
  const offset = s.start - traceStart
  const left = ((offset - view.start) / width) * 100
  const w = (s.duration_ms / width) * 100
  const hasChildren = row.node.children.length > 0
  const failed = s.status !== null && s.status !== 'ok' && s.status !== 'unknown'
  const label = s.is_segment ? (s.transaction ?? s.description) : s.description
  const color = opColor(opCategory(s.op))
  const labelRight = left + w < 82
  const labelInside = !labelRight && w > 14
  return (
    <div
      role="row"
      aria-rowindex={index + 1}
      aria-selected={selected}
      aria-level={row.depth + 1}
      aria-expanded={hasChildren ? !row.collapsed : undefined}
      data-span-id={s.span_id}
      className={cn('group flex h-7 cursor-default items-stretch border-b border-border/60 text-[12.5px]', selected ? 'bg-row-selected' : matched ? 'bg-brand-soft/60' : 'hover:bg-row-hover')}
      onClick={() => onSelect(s.span_id, true)}
    >
      <div role="gridcell" className="relative flex min-w-0 shrink-0 items-center gap-1 border-r pr-2" style={{ width: leftWidth, paddingLeft: 6 + row.depth * 14 }}>
        {Array.from({ length: row.depth }, (_, d) => (
          <span key={d} className="absolute inset-y-0 w-px bg-border" style={{ left: 12 + d * 14 }} aria-hidden />
        ))}
        {hasChildren ? (
          <button
            type="button"
            tabIndex={-1}
            className="relative z-[1] flex size-4 shrink-0 items-center justify-center rounded-sm bg-background text-muted-foreground hover:bg-accent hover:text-foreground"
            aria-label={row.collapsed ? `Expand ${row.node.descendants} spans` : 'Collapse'}
            onClick={(e) => {
              e.stopPropagation()
              onToggle(s.span_id)
            }}
          >
            <ChevronRight className={cn('size-3 transition-transform', !row.collapsed && 'rotate-90')} />
          </button>
        ) : (
          <span className="size-4 shrink-0" />
        )}
        <span className="size-2 shrink-0 rounded-[2px]" style={{ background: color }} aria-hidden />
        {row.collapsed && <span className="shrink-0 rounded bg-muted px-1 text-[10.5px] text-muted-foreground tabular">{row.node.descendants}</span>}
        <span className={cn('shrink-0 font-mono text-[11.5px] text-muted-foreground', s.is_segment && 'text-foreground')}>{s.op ?? 'span'}</span>
        <span className={cn('min-w-0 truncate', s.is_segment && 'font-medium')} title={label ?? undefined}>
          {label ?? <span className="text-muted-foreground">no description</span>}
        </span>
        {s.is_segment && s.project_id && <span className="ml-auto hidden shrink-0 rounded bg-muted px-1 text-[10.5px] text-muted-foreground xl:inline">{projectName(s.project_id)}</span>}
        {(errors?.length || failed) && <CircleAlert className="size-3.5 shrink-0 text-destructive" aria-label={errors?.length ? `${errors.length} errors` : `status ${s.status}`} />}
      </div>
      <div role="gridcell" className="relative min-w-0 flex-1 overflow-hidden">
        <div
          className={cn('absolute top-1/2 h-3.5 -translate-y-1/2 rounded-[3px]', critical === false && 'opacity-25', critical === true && 'ring-2 ring-foreground/60 ring-offset-1 ring-offset-background')}
          style={{ left: `${left}%`, width: `max(2px, ${w}%)`, background: failed ? 'var(--destructive)' : color }}
        />
        <span
          className={cn('pointer-events-none absolute top-1/2 -translate-y-1/2 px-1.5 text-[11px] whitespace-nowrap tabular', labelInside ? 'font-medium text-white' : 'text-muted-foreground')}
          style={labelRight ? { left: `max(0px, ${left + w}%)` } : labelInside ? { right: `max(2px, ${100 - left - w}%)` } : { right: `max(0px, ${100 - left}%)` }}
        >
          {ms(s.duration_ms)}
        </span>
        {logs?.map((l, i) => (
          <span key={i} className="absolute top-0.5 h-1.5 w-[3px] rounded-full" style={{ left: `${((l.timestamp - traceStart - view.start) / width) * 100}%`, background: LOG_LEVEL_VAR[l.level as 'info'] ?? 'var(--level-info)' }} title={`${l.level}: ${l.body}`} />
        ))}
        {errors?.map((e) => (
          <span
            key={e.event_id}
            className="absolute top-1/2 size-2.5 -translate-x-1/2 -translate-y-1/2 rotate-45 rounded-[1px] bg-destructive ring-2 ring-background"
            style={{ left: `${((e.timestamp - traceStart - view.start) / width) * 100}%` }}
            title={e.title}
          />
        ))}
      </div>
    </div>
  )
})

export function Waterfall({ rows, traceStart, traceDuration, view, onView, selectedId, onSelect, onToggle, critical, matches, errorsBySpan, logsBySpan, projectName, scrollToIndex }: Props) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const [leftWidth, setLeftWidth] = useState(() => {
    try {
      return Number(localStorage.getItem('rupta-waterfall-left')) || 420
    } catch {
      return 420
    }
  })
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    initialRect: { width: 1200, height: 800 },
    overscan: 16,
    getItemKey: (i) => rows[i].node.span.span_id,
  })
  useEffect(() => {
    if (scrollToIndex !== null && scrollToIndex >= 0) virtualizer.scrollToIndex(scrollToIndex, { align: 'auto' })
  }, [scrollToIndex, virtualizer])

  // Ctrl/⌘ + wheel (or a trackpad pinch) zooms around the pointer.
  const timelineRef = useRef<HTMLDivElement>(null)
  const viewRef = useRef(view)
  viewRef.current = view
  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey) && !e.shiftKey) return
      const tl = timelineRef.current?.getBoundingClientRect()
      if (!tl || e.clientX < tl.left) return
      e.preventDefault()
      const v = viewRef.current
      if (e.shiftKey && !(e.ctrlKey || e.metaKey)) {
        const w = v.end - v.start
        const delta = ((e.deltaY || e.deltaX) / tl.width) * w
        onView(clampView({ start: v.start + delta, end: v.end + delta }, traceDuration))
        return
      }
      const anchor = Math.max(0, Math.min(1, (e.clientX - tl.left) / tl.width))
      onView(zoomView(v, Math.exp(e.deltaY * 0.01), anchor, traceDuration))
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [onView, traceDuration])

  const startResize = useCallback(
    (e: ReactPointerEvent) => {
      const x0 = e.clientX
      const w0 = leftWidth
      const move = (ev: PointerEvent) => setLeftWidth(Math.max(220, Math.min(900, w0 + ev.clientX - x0)))
      const up = () => {
        window.removeEventListener('pointermove', move)
        window.removeEventListener('pointerup', up)
        setLeftWidth((w) => {
          try {
            localStorage.setItem('rupta-waterfall-left', String(w))
          } catch {
            /* ignore */
          }
          return w
        })
      }
      window.addEventListener('pointermove', move)
      window.addEventListener('pointerup', up)
    },
    [leftWidth],
  )

  const items = virtualizer.getVirtualItems()
  return (
    <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto" data-testid="waterfall-scroll">
      <div className="sticky top-0 z-10 flex h-7 border-b bg-background/95 backdrop-blur">
        <div className="relative flex shrink-0 items-center border-r px-3 text-[11px] font-medium tracking-wide text-muted-foreground uppercase" style={{ width: leftWidth }}>
          Span
          <div
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize span column"
            tabIndex={0}
            className="absolute inset-y-0 -right-1 z-20 w-2 cursor-col-resize hover:bg-brand/30"
            onPointerDown={startResize}
            onKeyDown={(e) => {
              if (e.key === 'ArrowLeft') setLeftWidth((w) => Math.max(220, w - 20))
              if (e.key === 'ArrowRight') setLeftWidth((w) => Math.min(900, w + 20))
            }}
          />
        </div>
        <div ref={timelineRef} className="min-w-0 flex-1">
          <TimeAxis view={view} />
        </div>
      </div>
      <div role="treegrid" aria-label="Spans" aria-rowcount={rows.length} className="relative" style={{ height: virtualizer.getTotalSize(), minWidth: leftWidth + 240 }}>
        {items.map((v) => {
          const row = rows[v.index]
          const id = row.node.span.span_id
          return (
            <div key={v.key} className="absolute inset-x-0" style={{ transform: `translateY(${v.start}px)` }}>
              <Row
                row={row}
                index={v.index}
                selected={id === selectedId}
                matched={matches.has(id)}
                critical={critical ? critical.has(id) : null}
                traceStart={traceStart}
                view={view}
                errors={errorsBySpan.get(id)}
                logs={logsBySpan.get(id)}
                leftWidth={leftWidth}
                onSelect={onSelect}
                onToggle={onToggle}
                projectName={projectName}
              />
            </div>
          )
        })}
      </div>
    </div>
  )
}
