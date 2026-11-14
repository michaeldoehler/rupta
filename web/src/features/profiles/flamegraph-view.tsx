// Canvas flamegraph with zoom, pan, search highlighting and colouring by
// application/library or package, plus a call-tree table, a function
// table and frame details. Loaded lazily: only the profile pages use it.

import { useVirtualizer } from '@tanstack/react-virtual'
import { ChevronRight, Search, ZoomOut } from 'lucide-react'
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import type { CallTreeNode, Flamegraph } from '@/api/telemetry-types'
import { useTheme } from '@/components/theme-provider'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Kbd } from '@/components/ui/kbd'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { cn } from '@/lib/utils'
import { ms } from '@/features/telemetry/format'
import { callersOf, frameColor, functionTable, layoutFlamegraph, matchedWeight, rowsByDepth, hitTestRow, searchFrames, stackOf, type ColorMode, type FlameOrder } from './flamegraph-layout'

const ROW_H = 18

interface Viewport {
  x0: number
  x1: number
}

function weightLabel(w: number, total: number, msPerSample: number | null) {
  const pct = total ? (w / total) * 100 : 0
  return `${msPerSample ? `${ms(w * msPerSample)} · ` : ''}${w.toLocaleString('en-US')} samples · ${pct < 0.1 ? pct.toFixed(2) : pct.toFixed(1)}%`
}

export default function FlamegraphView({ flamegraph, durationMs }: { flamegraph: Flamegraph; durationMs?: number | null }) {
  const { resolved } = useTheme()
  const dark = resolved === 'dark'
  const [order, setOrder] = useState<FlameOrder>('time')
  const [color, setColor] = useState<ColorMode>('application')
  const [search, setSearch] = useState('')
  const [selected, setSelected] = useState<number>(-1)
  const [hover, setHover] = useState<{ i: number; x: number; y: number } | null>(null)
  const layout = useMemo(() => layoutFlamegraph(flamegraph, order), [flamegraph, order])
  const rows = useMemo(() => rowsByDepth(layout), [layout])
  const total = layout.total || 1
  const [view, setView] = useState<Viewport>({ x0: 0, x1: total })
  useEffect(() => {
    setView({ x0: 0, x1: layout.total || 1 })
    setSelected(-1)
  }, [layout])
  const matches = useMemo(() => searchFrames(flamegraph.frames, search), [flamegraph.frames, search])
  const matchW = useMemo(() => matchedWeight(layout, matches), [layout, matches])
  const msPerSample = durationMs && flamegraph.profiles <= 1 ? durationMs / total : null
  const searchRef = useRef<HTMLInputElement>(null)

  // ---- canvas
  const wrapRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [width, setWidth] = useState(900)
  useLayoutEffect(() => {
    const el = wrapRef.current
    if (!el) return
    setWidth(el.clientWidth || 900)
    const ro = new ResizeObserver((e) => setWidth(e[0]?.contentRect.width || el.clientWidth || 900))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  const height = (layout.maxDepth + 1) * ROW_H + 4
  const selectedFrame = selected >= 0 ? layout.rects[selected]?.frame : -1

  useEffect(() => {
    const c = canvasRef.current
    const ctx = c?.getContext?.('2d')
    if (!c || !ctx) return
    const dpr = window.devicePixelRatio || 1
    c.width = Math.round(width * dpr)
    c.height = Math.round(height * dpr)
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, width, height)
    const scale = width / (view.x1 - view.x0)
    const css = getComputedStyle(c)
    const fg = css.getPropertyValue('--foreground').trim() || (dark ? '#eee' : '#222')
    ctx.font = '11px "Geist Mono Variable", ui-monospace, monospace'
    ctx.textBaseline = 'middle'
    const searching = matches.size > 0
    for (let depth = 0; depth < rows.length; depth++) {
      const y = depth * ROW_H
      for (const i of rows[depth]) {
        const r = layout.rects[i]
        if (r.x + r.w < view.x0 || r.x > view.x1) continue
        const x = (r.x - view.x0) * scale
        const w = r.w * scale
        if (w < 0.4) continue
        const frame = flamegraph.frames[r.frame]
        ctx.globalAlpha = searching && !matches.has(r.frame) ? 0.22 : 1
        ctx.fillStyle = frameColor(frame, color, dark)
        ctx.fillRect(x + 0.5, y + 0.5, Math.max(0.5, w - 1), ROW_H - 1)
        if (r.frame === selectedFrame) {
          ctx.globalAlpha = 1
          ctx.strokeStyle = fg
          ctx.lineWidth = i === selected ? 2 : 1
          ctx.strokeRect(x + 1, y + 1, Math.max(1, w - 2), ROW_H - 2)
        }
        if (w > 28) {
          ctx.globalAlpha = searching && !matches.has(r.frame) ? 0.4 : 1
          ctx.fillStyle = fg
          const label = frame?.name ?? '?'
          const max = w - 8
          let text = label
          if (ctx.measureText(text).width > max) {
            let lo = 0
            let hi = label.length
            while (lo < hi) {
              const mid = (lo + hi + 1) >> 1
              if (ctx.measureText(`${label.slice(0, mid)}…`).width <= max) lo = mid
              else hi = mid - 1
            }
            text = lo > 1 ? `${label.slice(0, lo)}…` : ''
          }
          if (text) ctx.fillText(text, Math.max(x, 0) + 4, y + ROW_H / 2 + 0.5)
        }
      }
    }
    ctx.globalAlpha = 1
  }, [layout, rows, view, width, height, color, dark, matches, flamegraph.frames, selected, selectedFrame])

  const at = (clientX: number, clientY: number) => {
    const rect = canvasRef.current!.getBoundingClientRect()
    const x = view.x0 + ((clientX - rect.left) / rect.width) * (view.x1 - view.x0)
    const depth = Math.floor((clientY - rect.top) / ROW_H)
    if (depth < 0 || depth >= rows.length) return -1
    return hitTestRow(layout, rows[depth], x)
  }
  const drag = useRef<{ x: number; view: Viewport; moved: boolean } | null>(null)
  const onDown = (e: ReactPointerEvent) => {
    drag.current = { x: e.clientX, view, moved: false }
    ;(e.target as Element).setPointerCapture?.(e.pointerId)
  }
  const onMove = (e: ReactPointerEvent) => {
    const d = drag.current
    if (d && Math.abs(e.clientX - d.x) > 3) {
      d.moved = true
      const span = d.view.x1 - d.view.x0
      const delta = ((d.x - e.clientX) / width) * span
      const x0 = Math.max(0, Math.min(total - span, d.view.x0 + delta))
      setView({ x0, x1: x0 + span })
      setHover(null)
      return
    }
    const i = at(e.clientX, e.clientY)
    const box = wrapRef.current!.getBoundingClientRect()
    setHover(i >= 0 ? { i, x: e.clientX - box.left, y: e.clientY - box.top } : null)
  }
  const onUp = (e: ReactPointerEvent) => {
    const d = drag.current
    drag.current = null
    if (d?.moved) return
    const i = at(e.clientX, e.clientY)
    if (i >= 0) focusRect(i)
  }
  const focusRect = useCallback(
    (i: number) => {
      const r = layout.rects[i]
      setSelected(i)
      setView({ x0: r.x, x1: r.x + r.w })
    },
    [layout],
  )
  const zoom = useCallback(
    (factor: number, anchor = 0.5) =>
      setView((v) => {
        const span = v.x1 - v.x0
        const next = Math.max(total / 100_000, Math.min(total, span * factor))
        const center = v.x0 + span * anchor
        const x0 = Math.max(0, Math.min(total - next, center - next * anchor))
        return { x0, x1: x0 + next }
      }),
    [total],
  )
  useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey || e.shiftKey)) return
      e.preventDefault()
      const rect = el.getBoundingClientRect()
      if (e.shiftKey && !(e.ctrlKey || e.metaKey)) {
        setView((v) => {
          const span = v.x1 - v.x0
          const x0 = Math.max(0, Math.min(total - span, v.x0 + ((e.deltaY || e.deltaX) / rect.width) * span))
          return { x0, x1: x0 + span }
        })
        return
      }
      zoom(Math.exp(e.deltaY * 0.01), Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width)))
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [zoom, total])

  const reset = () => setView({ x0: 0, x1: total })
  useHotkeys(
    {
      '/': () => searchRef.current?.focus(),
      r: reset,
      '0': reset,
      '=': () => zoom(0.5),
      '-': () => zoom(2),
      Escape: () => {
        setSelected(-1)
        reset()
      },
    },
    [total, zoom],
  )

  const zoomed = view.x0 > 0 || view.x1 < total
  const hovered = hover ? layout.rects[hover.i] : null
  const hoveredFrame = hovered ? flamegraph.frames[hovered.frame] : null

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2" role="toolbar" aria-label="Flamegraph">
        <div className="relative w-72 max-w-full">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input ref={searchRef} value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search functions, files, packages" className="h-7 pl-7 text-[13px]" aria-label="Search frames" onKeyDown={(e) => e.key === 'Escape' && (e.target as HTMLInputElement).blur()} />
        </div>
        {search && (
          <span className="text-xs text-muted-foreground tabular" role="status">
            {matches.size} {matches.size === 1 ? 'function' : 'functions'} · {((matchW / total) * 100).toFixed(1)}% of samples
          </span>
        )}
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <ToggleGroup type="single" value={order} onValueChange={(v) => v && setOrder(v as FlameOrder)} aria-label="Order">
            <ToggleGroupItem value="time">Time order</ToggleGroupItem>
            <ToggleGroupItem value="heavy">Left heavy</ToggleGroupItem>
          </ToggleGroup>
          <ToggleGroup type="single" value={color} onValueChange={(v) => v && setColor(v as ColorMode)} aria-label="Colour by">
            <ToggleGroupItem value="application">App vs. library</ToggleGroupItem>
            <ToggleGroupItem value="package">Package</ToggleGroupItem>
          </ToggleGroup>
          <Button variant="ghost" size="xs" onClick={reset} disabled={!zoomed}>
            <ZoomOut /> Reset <Kbd className="ml-0.5">0</Kbd>
          </Button>
        </div>
      </div>
      {color === 'application' && (
        <div className="flex items-center gap-3 text-xs text-muted-foreground" aria-hidden>
          <span className="flex items-center gap-1.5">
            <span className="h-2.5 w-4 rounded-sm" style={{ background: frameColor({ name: 'a', file: null, line: null, package: null, is_application: true }, 'application', dark) }} /> Application code
          </span>
          <span className="flex items-center gap-1.5">
            <span className="h-2.5 w-4 rounded-sm" style={{ background: frameColor({ name: 'a', file: null, line: null, package: 'lib', is_application: false }, 'application', dark) }} /> Libraries and runtime
          </span>
          <span className="ml-auto hidden md:inline">Click a frame to zoom in · drag to pan · ⌘/Ctrl + scroll to zoom · Esc to reset</span>
        </div>
      )}
      <div ref={wrapRef} className="relative max-h-[540px] overflow-y-auto rounded-lg border bg-card" data-testid="flamegraph">
        <canvas
          ref={canvasRef}
          style={{ width, height }}
          className="block cursor-pointer touch-none"
          onPointerDown={onDown}
          onPointerMove={onMove}
          onPointerUp={onUp}
          onPointerLeave={() => setHover(null)}
          role="img"
          aria-label={`Flamegraph of ${flamegraph.samples} samples. Use the call tree below for a keyboard-accessible view.`}
        />
        {hovered && hoveredFrame && hover && (
          <div className="pointer-events-none absolute z-10 max-w-md rounded-md border bg-popover px-2.5 py-1.5 text-xs shadow-md" style={{ left: Math.min(hover.x + 12, width - 300), top: hover.y + 14 }}>
            <div className="font-mono font-medium break-all">{hoveredFrame.name}</div>
            {hoveredFrame.file && (
              <div className="font-mono break-all text-muted-foreground">
                {hoveredFrame.file}
                {hoveredFrame.line ? `:${hoveredFrame.line}` : ''}
              </div>
            )}
            <div className="mt-1 flex gap-3 tabular">
              <span>total {weightLabel(hovered.w, total, msPerSample)}</span>
            </div>
            <div className="tabular text-muted-foreground">self {weightLabel(hovered.node.self_weight, total, msPerSample)}</div>
          </div>
        )}
      </div>
      <div className="grid gap-3 xl:grid-cols-[minmax(0,1fr)_340px]">
        <Tabs defaultValue="tree" className="min-w-0">
          <TabsList>
            <TabsTrigger value="tree">Call tree</TabsTrigger>
            <TabsTrigger value="functions">Functions</TabsTrigger>
          </TabsList>
          <TabsContent value="tree">
            <CallTree flamegraph={flamegraph} total={total} msPerSample={msPerSample} order={order} selectedFrame={selectedFrame} matches={matches} onSelectFrame={(f) => {
              const i = layout.rects.findIndex((r) => r.frame === f)
              if (i >= 0) focusRect(i)
            }} />
          </TabsContent>
          <TabsContent value="functions">
            <FunctionTable flamegraph={flamegraph} total={total} msPerSample={msPerSample} selectedFrame={selectedFrame} onSelectFrame={(f) => {
              const i = layout.rects.reduce((best, r, idx) => (r.frame === f && (best < 0 || r.w > layout.rects[best].w) ? idx : best), -1)
              if (i >= 0) focusRect(i)
            }} />
          </TabsContent>
        </Tabs>
        <FrameDetails flamegraph={flamegraph} layout={layout} index={selected} total={total} msPerSample={msPerSample} onSearch={(t) => setSearch(t)} onSelect={focusRect} />
      </div>
    </div>
  )
}

function FrameDetails({ flamegraph, layout, index, total, msPerSample, onSearch, onSelect }: { flamegraph: Flamegraph; layout: ReturnType<typeof layoutFlamegraph>; index: number; total: number; msPerSample: number | null; onSearch: (t: string) => void; onSelect: (i: number) => void }) {
  if (index < 0 || !layout.rects[index])
    return (
      <aside className="rounded-lg border bg-card p-4 text-[13px] text-muted-foreground" aria-label="Frame details">
        Select a frame in the flamegraph or the tables to see its details, callers and call stack.
      </aside>
    )
  const r = layout.rects[index]
  const f = flamegraph.frames[r.frame]
  const callers = callersOf(layout, r.frame).slice(0, 6)
  const stack = stackOf(layout, index)
  return (
    <aside className="min-w-0 space-y-3 rounded-lg border bg-card p-4" aria-label="Frame details" data-testid="frame-details">
      <div>
        <div className="flex flex-wrap items-center gap-1.5">
          <Badge variant={f?.is_application ? 'brand' : 'secondary'}>{f?.is_application ? 'application' : 'library'}</Badge>
          {f?.package && <Badge variant="outline" className="font-mono">{f.package}</Badge>}
        </div>
        <h3 className="mt-1.5 font-mono text-[13px] font-semibold break-all">{f?.name}</h3>
        {f?.file && (
          <p className="font-mono text-xs break-all text-muted-foreground">
            {f.file}
            {f.line ? `:${f.line}` : ''}
          </p>
        )}
      </div>
      <dl className="grid grid-cols-2 gap-2 text-[13px]">
        <div>
          <dt className="text-[11px] text-muted-foreground uppercase">Total</dt>
          <dd className="font-semibold tabular">{((r.w / total) * 100).toFixed(1)}%</dd>
          <dd className="text-xs text-muted-foreground tabular">{msPerSample ? ms(r.w * msPerSample) : `${r.w} samples`}</dd>
        </div>
        <div>
          <dt className="text-[11px] text-muted-foreground uppercase">Self</dt>
          <dd className="font-semibold tabular">{((r.node.self_weight / total) * 100).toFixed(1)}%</dd>
          <dd className="text-xs text-muted-foreground tabular">{msPerSample ? ms(r.node.self_weight * msPerSample) : `${r.node.self_weight} samples`}</dd>
        </div>
      </dl>
      {callers.length > 0 && (
        <div>
          <h4 className="mb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">Called from</h4>
          <ul className="space-y-0.5 text-[12.5px]">
            {callers.map((c) => (
              <li key={c.frame} className="flex items-center gap-2 font-mono">
                <span className="min-w-0 flex-1 truncate">{c.frame >= 0 ? flamegraph.frames[c.frame]?.name : '(root)'}</span>
                <span className="text-xs text-muted-foreground tabular">{((c.weight / total) * 100).toFixed(1)}%</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      <div>
        <h4 className="mb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">Stack</h4>
        <ol className="max-h-48 space-y-0.5 overflow-y-auto font-mono text-[12px]">
          {stack.map((fi, d) => (
            <li key={d} className={cn('truncate', fi === r.frame && 'font-semibold', flamegraph.frames[fi]?.is_application ? 'text-foreground' : 'text-muted-foreground')} style={{ paddingLeft: Math.min(d, 12) * 6 }}>
              {d === stack.length - 1 ? (
                flamegraph.frames[fi]?.name
              ) : (
                <button type="button" className="hover:underline" onClick={() => {
                  let i = index
                  for (let k = stack.length - 1; k > d; k--) i = layout.rects[i].parent
                  onSelect(i)
                }}>
                  {flamegraph.frames[fi]?.name}
                </button>
              )}
            </li>
          ))}
        </ol>
      </div>
      <Button variant="outline" size="xs" onClick={() => onSearch(f?.name ?? '')}>
        <Search /> Highlight everywhere
      </Button>
    </aside>
  )
}

interface TreeRow {
  node: CallTreeNode
  depth: number
  key: string
  hasChildren: boolean
  open: boolean
}

function CallTree({ flamegraph, total, msPerSample, order, selectedFrame, matches, onSelectFrame }: { flamegraph: Flamegraph; total: number; msPerSample: number | null; order: FlameOrder; selectedFrame: number; matches: Set<number>; onSelectFrame: (f: number) => void }) {
  // Expanded paths; initially every node above 4% of the samples.
  const [open, setOpen] = useState<Set<string>>(() => {
    const s = new Set<string>()
    const visit = (n: CallTreeNode, key: string, depth: number) => {
      if (depth > 30 || n.total_weight / (flamegraph.root.total_weight || 1) < 0.04) return
      s.add(key)
      n.children.forEach((c, i) => visit(c, `${key}/${i}`, depth + 1))
    }
    flamegraph.root.children.forEach((c, i) => visit(c, String(i), 0))
    return s
  })
  const rows = useMemo(() => {
    const out: TreeRow[] = []
    const sortKids = (n: CallTreeNode) => (order === 'heavy' ? n.children.map((c, i) => [c, i] as const).sort((a, b) => b[0].total_weight - a[0].total_weight) : n.children.map((c, i) => [c, i] as const))
    const stack: { node: CallTreeNode; depth: number; key: string }[] = sortKids(flamegraph.root).reverse().map(([c, i]) => ({ node: c, depth: 0, key: String(i) }))
    while (stack.length) {
      const { node, depth, key } = stack.pop()!
      const isOpen = open.has(key)
      out.push({ node, depth, key, hasChildren: node.children.length > 0, open: isOpen })
      if (isOpen) for (const [c, i] of sortKids(node).reverse()) stack.push({ node: c, depth: depth + 1, key: `${key}/${i}` })
    }
    return out
  }, [flamegraph, open, order])
  const scrollRef = useRef<HTMLDivElement>(null)
  const v = useVirtualizer({ count: rows.length, getScrollElement: () => scrollRef.current, estimateSize: () => 26, initialRect: { width: 900, height: 400 }, overscan: 10 })
  const toggle = (key: string) => setOpen((s) => {
    const n = new Set(s)
    if (n.has(key)) n.delete(key)
    else n.add(key)
    return n
  })
  const pct = (w: number) => `${((w / total) * 100).toFixed(1)}%`
  return (
    <div className="overflow-hidden rounded-lg border bg-card">
      <div className="grid grid-cols-[88px_88px_minmax(0,1fr)] border-b bg-muted/40 px-2 py-1.5 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
        <span className="text-right">Self</span>
        <span className="text-right">Total</span>
        <span className="pl-3">Function</span>
      </div>
      <div ref={scrollRef} className="max-h-[420px] overflow-y-auto">
        <div role="treegrid" aria-label="Call tree" className="relative" style={{ height: v.getTotalSize() }}>
          {v.getVirtualItems().map((it) => {
            const r = rows[it.index]
            const f = r.node.frame_index !== null ? flamegraph.frames[r.node.frame_index] : undefined
            const hit = r.node.frame_index !== null && matches.has(r.node.frame_index)
            return (
              <div
                key={r.key}
                role="row"
                aria-level={r.depth + 1}
                aria-expanded={r.hasChildren ? r.open : undefined}
                aria-selected={r.node.frame_index === selectedFrame}
                tabIndex={0}
                className={cn('absolute inset-x-0 grid h-[26px] cursor-default grid-cols-[88px_88px_minmax(0,1fr)] items-center px-2 text-[12.5px] hover:bg-row-hover focus-visible:bg-row-hover focus-visible:outline-none', r.node.frame_index === selectedFrame && 'bg-row-selected', hit && 'bg-brand-soft/60')}
                style={{ transform: `translateY(${it.start}px)` }}
                onClick={() => r.node.frame_index !== null && onSelectFrame(r.node.frame_index)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault()
                    if (r.node.frame_index !== null) onSelectFrame(r.node.frame_index)
                  }
                  if (e.key === 'ArrowRight' && r.hasChildren && !r.open) toggle(r.key)
                  if (e.key === 'ArrowLeft' && r.open) toggle(r.key)
                  if (e.key === 'ArrowDown') {
                    e.preventDefault()
                    ;(e.currentTarget.nextElementSibling as HTMLElement | null)?.focus()
                  }
                  if (e.key === 'ArrowUp') {
                    e.preventDefault()
                    ;(e.currentTarget.previousElementSibling as HTMLElement | null)?.focus()
                  }
                }}
              >
                <span className="text-right text-muted-foreground tabular" title={msPerSample ? ms(r.node.self_weight * msPerSample) : `${r.node.self_weight} samples`}>
                  {pct(r.node.self_weight)}
                </span>
                <span className="text-right font-medium tabular" title={msPerSample ? ms(r.node.total_weight * msPerSample) : `${r.node.total_weight} samples`}>
                  {pct(r.node.total_weight)}
                </span>
                <span className="flex min-w-0 items-center gap-1 pl-3 font-mono" style={{ paddingLeft: 12 + Math.min(r.depth, 40) * 12 }}>
                  {r.hasChildren ? (
                    <button type="button" tabIndex={-1} aria-label={r.open ? 'Collapse' : 'Expand'} className="flex size-4 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:bg-accent" onClick={(e) => {
                        e.stopPropagation()
                        toggle(r.key)
                      }}>
                      <ChevronRight className={cn('size-3 transition-transform', r.open && 'rotate-90')} />
                    </button>
                  ) : (
                    <span className="size-4 shrink-0" />
                  )}
                  <span className={cn('truncate', !f?.is_application && 'text-muted-foreground')} title={f ? `${f.name} — ${f.file ?? ''}${f.line ? `:${f.line}` : ''}` : undefined}>
                    {f?.name ?? '(root)'}
                  </span>
                  {f?.package && <span className="hidden shrink-0 text-[11px] text-muted-foreground sm:inline">{f.package}</span>}
                </span>
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}

function FunctionTable({ flamegraph, total, msPerSample, selectedFrame, onSelectFrame }: { flamegraph: Flamegraph; total: number; msPerSample: number | null; selectedFrame: number; onSelectFrame: (f: number) => void }) {
  const [sort, setSort] = useState<'self' | 'total'>('self')
  const rows = useMemo(() => {
    const t = functionTable(flamegraph)
    return sort === 'total' ? [...t].sort((a, b) => b.total - a.total) : t
  }, [flamegraph, sort])
  const pct = (w: number) => `${((w / total) * 100).toFixed(1)}%`
  return (
    <div className="max-h-[460px] overflow-auto rounded-lg border bg-card">
      <table className="w-full text-[12.5px]" aria-label="Functions">
        <thead className="sticky top-0 bg-muted/90 text-[11px] tracking-wide text-muted-foreground uppercase backdrop-blur">
          <tr>
            <th className="px-2 py-1.5 text-right font-medium">
              <button type="button" className={cn('hover:text-foreground', sort === 'self' && 'text-foreground')} onClick={() => setSort('self')}>
                Self
              </button>
            </th>
            <th className="px-2 py-1.5 text-right font-medium">
              <button type="button" className={cn('hover:text-foreground', sort === 'total' && 'text-foreground')} onClick={() => setSort('total')}>
                Total
              </button>
            </th>
            <th className="px-2 py-1.5 text-left font-medium">Function</th>
            <th className="hidden px-2 py-1.5 text-left font-medium md:table-cell">Package</th>
            <th className="px-2 py-1.5 text-right font-medium">Calls</th>
          </tr>
        </thead>
        <tbody className="divide-y">
          {rows.slice(0, 300).map((r) => {
            const f = flamegraph.frames[r.frame]
            return (
              <tr key={r.frame} className={cn('cursor-default hover:bg-row-hover', r.frame === selectedFrame && 'bg-row-selected')} onClick={() => onSelectFrame(r.frame)}>
                <td className="px-2 py-1 text-right font-medium tabular" title={msPerSample ? ms(r.self * msPerSample) : `${r.self} samples`}>
                  {pct(r.self)}
                </td>
                <td className="px-2 py-1 text-right text-muted-foreground tabular">{pct(r.total)}</td>
                <td className={cn('max-w-0 truncate px-2 py-1 font-mono', !f?.is_application && 'text-muted-foreground')} title={`${f?.file ?? ''}${f?.line ? `:${f.line}` : ''}`}>
                  {f?.name}
                </td>
                <td className="hidden px-2 py-1 font-mono text-muted-foreground md:table-cell">{f?.package ?? (f?.is_application ? 'app' : '')}</td>
                <td className="px-2 py-1 text-right text-muted-foreground tabular">{r.calls}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
