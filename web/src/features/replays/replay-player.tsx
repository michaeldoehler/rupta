// The session replay player: rrweb's Replayer with our own controls,
// timeline markers, speed, skip inactivity and fullscreen. Loaded lazily
// (rrweb is large), only on the replay page.

import { FastForward, Maximize, Minimize, MousePointerClick, Navigation, Pause, Play, RotateCcw, TriangleAlert } from 'lucide-react'
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Replayer } from 'rrweb'
import 'rrweb/dist/style.css'
import type { RecordingEvent } from '@/api/telemetry-types'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Tip } from '@/components/ui/tooltip'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { cn } from '@/lib/utils'
import { clock, urlAt, type Marker } from './timeline'

const SPEEDS = [0.5, 1, 2, 4, 8]
const MARKER_STYLE: Record<Marker['kind'], { cls: string; icon: typeof Play; label: string }> = {
  error: { cls: 'bg-destructive', icon: TriangleAlert, label: 'Error' },
  click: { cls: 'bg-series-2', icon: MousePointerClick, label: 'Click' },
  navigation: { cls: 'bg-series-5', icon: Navigation, label: 'Navigation' },
}

export interface PlayerProps {
  events: RecordingEvent[]
  markers: Marker[]
  /** Called about 10 times a second with the playback offset (ms). */
  onTime: (ms: number) => void
  /** Seek requests from the side panels. */
  seek: { at: number; nonce: number } | null
}

export default function ReplayPlayer({ events, markers, onTime, seek }: PlayerProps) {
  const frameRef = useRef<HTMLDivElement>(null)
  const stageRef = useRef<HTMLDivElement>(null)
  const shellRef = useRef<HTMLDivElement>(null)
  const replayer = useRef<Replayer | null>(null)
  const [playing, setPlaying] = useState(false)
  const [time, setTime] = useState(0)
  const [total, setTotal] = useState(0)
  const [speed, setSpeed] = useState(1)
  const [skip, setSkip] = useState(true)
  const [skipping, setSkipping] = useState(false)
  const [size, setSize] = useState({ width: 1280, height: 720 })
  const [scale, setScale] = useState(1)
  const [fullscreen, setFullscreen] = useState(false)
  const [failed, setFailed] = useState<string | null>(null)

  // Create the replayer once per recording.
  useEffect(() => {
    const root = frameRef.current
    if (!root || events.length < 2) return
    root.innerHTML = ''
    let r: Replayer
    try {
      r = new Replayer(events as never, {
        root,
        skipInactive: skip,
        speed,
        showWarning: false,
        triggerFocus: false,
        mouseTail: { strokeStyle: 'rgba(228, 74, 8, 0.6)', lineWidth: 3, duration: 600 },
        UNSAFE_replayCanvas: false,
      })
    } catch (e) {
      setFailed(e instanceof Error ? e.message : 'The recording could not be played.')
      return
    }
    replayer.current = r
    const meta = r.getMetaData()
    setTotal(meta.totalTime)
    const meta0 = events.find((e) => e.type === 4)?.data as { width?: number; height?: number } | undefined
    if (meta0?.width && meta0?.height) setSize({ width: meta0.width, height: meta0.height })
    r.on('resize', (d) => {
      const dim = d as { width: number; height: number }
      if (dim.width && dim.height) setSize({ width: dim.width, height: dim.height })
    })
    r.on('finish', () => setPlaying(false))
    r.on('skip-start', () => setSkipping(true))
    r.on('skip-end', () => setSkipping(false))
    r.pause(0)
    return () => {
      r.pause()
      r.destroy?.()
      replayer.current = null
    }
    // speed and skip are applied with setConfig below
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [events])

  useEffect(() => {
    replayer.current?.setConfig({ speed, skipInactive: skip })
  }, [speed, skip])

  // Clock: poll the replayer while playing.
  useEffect(() => {
    if (!playing) return
    let raf = 0
    let last = 0
    const tick = (t: number) => {
      const r = replayer.current
      if (r && t - last > 90) {
        last = t
        const now = Math.min(total, r.getCurrentTime())
        setTime(now)
        onTime(now)
      }
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [playing, total, onTime])

  // Fit the recorded viewport into the stage.
  useLayoutEffect(() => {
    const el = stageRef.current
    if (!el) return
    const fit = () => {
      const w = el.clientWidth || 800
      const h = el.clientHeight || 450
      setScale(Math.min(w / size.width, h / size.height, 1.5))
    }
    fit()
    const ro = new ResizeObserver(fit)
    ro.observe(el)
    return () => ro.disconnect()
  }, [size, fullscreen])

  const goTo = useCallback(
    (ms: number, play = playing) => {
      const r = replayer.current
      const t = Math.max(0, Math.min(total, ms))
      setTime(t)
      onTime(t)
      if (!r) return
      if (play) r.play(t)
      else r.pause(t)
    },
    [playing, total, onTime],
  )

  useEffect(() => {
    if (seek) goTo(seek.at)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seek])

  const toggle = () => {
    const r = replayer.current
    if (!r) return
    if (playing) {
      r.pause()
      setPlaying(false)
      setTime(r.getCurrentTime())
    } else {
      const from = time >= total - 50 ? 0 : time
      r.play(from)
      setPlaying(true)
    }
  }

  useEffect(() => {
    const on = () => setFullscreen(document.fullscreenElement === shellRef.current)
    document.addEventListener('fullscreenchange', on)
    return () => document.removeEventListener('fullscreenchange', on)
  }, [])
  const toggleFullscreen = () => {
    if (document.fullscreenElement) void document.exitFullscreen()
    else void shellRef.current?.requestFullscreen?.()
  }

  useHotkeys(
    {
      ' ': toggle,
      k: toggle,
      ArrowLeft: () => goTo(time - 5000),
      ArrowRight: () => goTo(time + 5000),
      ',': () => setSpeed((s) => SPEEDS[Math.max(0, SPEEDS.indexOf(s) - 1)]),
      '.': () => setSpeed((s) => SPEEDS[Math.min(SPEEDS.length - 1, SPEEDS.indexOf(s) + 1)]),
      f: toggleFullscreen,
    },
    [time, playing, total, goTo],
  )

  const url = urlAt(markers, time)
  const scrub = (clientX: number, el: HTMLElement) => {
    const rect = el.getBoundingClientRect()
    goTo(((clientX - rect.left) / rect.width) * total)
  }
  const [hoverMarker, setHoverMarker] = useState<Marker | null>(null)

  return (
    <div ref={shellRef} className={cn('flex min-w-0 flex-col overflow-hidden rounded-lg border bg-card', fullscreen && 'h-full rounded-none border-0')} data-testid="replay-player">
      <div className="flex h-8 items-center gap-2 border-b bg-muted/40 px-3 text-xs">
        <span className="flex gap-1" aria-hidden>
          <span className="size-2 rounded-full bg-border" />
          <span className="size-2 rounded-full bg-border" />
          <span className="size-2 rounded-full bg-border" />
        </span>
        <span className="min-w-0 flex-1 truncate rounded bg-background px-2 py-0.5 font-mono text-[11.5px] text-muted-foreground" title={url ?? undefined}>
          {url ?? '—'}
        </span>
        <span className="text-muted-foreground tabular">
          {size.width}×{size.height}
        </span>
      </div>
      <div ref={stageRef} className={cn('relative flex items-center justify-center overflow-hidden bg-[repeating-conic-gradient(var(--muted)_0%_25%,transparent_0%_50%)] [background-size:16px_16px]', fullscreen ? 'flex-1' : 'aspect-video')}>
        {failed ? (
          <div className="flex flex-col items-center gap-2 p-6 text-center text-[13px]">
            <TriangleAlert className="size-5 text-destructive" />
            <p className="font-medium">This recording cannot be played</p>
            <p className="text-muted-foreground">{failed}</p>
          </div>
        ) : (
          <div style={{ width: size.width * scale, height: size.height * scale }} className="relative shadow-lg">
            <div ref={frameRef} className="replay-frame origin-top-left overflow-hidden bg-white" style={{ width: size.width, height: size.height, transform: `scale(${scale})` }} />
            <button type="button" className="absolute inset-0" onClick={toggle} aria-label={playing ? 'Pause' : 'Play'} />
            {!playing && time === 0 && (
              <span className="pointer-events-none absolute inset-0 flex items-center justify-center" aria-hidden>
                <span className="flex size-14 items-center justify-center rounded-full bg-foreground/80 text-background shadow-lg">
                  <Play className="ml-1 size-6" />
                </span>
              </span>
            )}
            {skipping && <span className="absolute top-2 right-2 flex items-center gap-1 rounded-md bg-foreground/80 px-2 py-1 text-xs text-background"><FastForward className="size-3.5" /> Skipping inactivity</span>}
          </div>
        )}
      </div>
      <div className="space-y-1.5 border-t px-3 pt-2 pb-2">
        <div
          className="group relative h-5 cursor-pointer"
          role="slider"
          tabIndex={0}
          aria-label="Playback position"
          aria-valuemin={0}
          aria-valuemax={Math.round(total / 1000)}
          aria-valuenow={Math.round(time / 1000)}
          aria-valuetext={`${clock(time)} of ${clock(total)}`}
          onPointerDown={(e) => {
            const el = e.currentTarget
            scrub(e.clientX, el)
            const move = (ev: PointerEvent) => scrub(ev.clientX, el)
            const up = () => {
              window.removeEventListener('pointermove', move)
              window.removeEventListener('pointerup', up)
            }
            window.addEventListener('pointermove', move)
            window.addEventListener('pointerup', up)
          }}
          onKeyDown={(e) => {
            if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
            e.preventDefault()
            e.stopPropagation()
            goTo(time + (e.key === 'ArrowLeft' ? -5000 : 5000))
          }}
        >
          <div className="absolute inset-x-0 top-1/2 h-1.5 -translate-y-1/2 overflow-hidden rounded-full bg-muted">
            <div className="h-full bg-brand" style={{ width: `${total ? (time / total) * 100 : 0}%` }} />
          </div>
          {markers.map((m, i) => {
            const s = MARKER_STYLE[m.kind]
            return (
              <span
                key={`${m.kind}-${i}`}
                className={cn('absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full ring-2 ring-card', s.cls, m.kind === 'error' ? 'size-2.5' : 'size-1.5')}
                style={{ left: `${total ? (m.offset / total) * 100 : 0}%` }}
                onPointerEnter={() => setHoverMarker(m)}
                onPointerLeave={() => setHoverMarker(null)}
                data-marker={m.kind}
              />
            )
          })}
          <span className="absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-card bg-brand shadow" style={{ left: `${total ? (time / total) * 100 : 0}%` }} />
          {hoverMarker && (
            <span className="pointer-events-none absolute bottom-full z-10 mb-1 max-w-72 -translate-x-1/2 truncate rounded-md border bg-popover px-2 py-1 text-xs shadow-md" style={{ left: `${(hoverMarker.offset / (total || 1)) * 100}%` }}>
              {MARKER_STYLE[hoverMarker.kind].label} · {clock(hoverMarker.offset)} · {hoverMarker.label}
            </span>
          )}
        </div>
        <div className="flex items-center gap-1">
          <Tip content={playing ? 'Pause (space)' : 'Play (space)'}>
            <Button variant="ghost" size="icon-sm" onClick={toggle} aria-label={playing ? 'Pause' : 'Play'} disabled={Boolean(failed)}>
              {playing ? <Pause /> : <Play />}
            </Button>
          </Tip>
          <Tip content="Back 5 s (←)">
            <Button variant="ghost" size="icon-sm" onClick={() => goTo(time - 5000)} aria-label="Back 5 seconds">
              <RotateCcw />
            </Button>
          </Tip>
          <span className="ml-1 text-xs tabular">
            {clock(time)} <span className="text-muted-foreground">/ {clock(total)}</span>
          </span>
          <div className="ml-auto flex items-center gap-1">
            <span className="mr-2 hidden items-center gap-3 text-[11px] text-muted-foreground md:flex" aria-hidden>
              {(['error', 'click', 'navigation'] as const).map((k) => (
                <span key={k} className="flex items-center gap-1">
                  <span className={cn('size-2 rounded-full', MARKER_STYLE[k].cls)} /> {MARKER_STYLE[k].label}
                </span>
              ))}
            </span>
            <Tip content="Skip periods without activity">
              <Button variant={skip ? 'secondary' : 'ghost'} size="xs" aria-pressed={skip} onClick={() => setSkip((s) => !s)}>
                <FastForward /> Skip inactivity
              </Button>
            </Tip>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="xs" aria-label={`Playback speed ${speed}×`} className="tabular">
                  {speed}×
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuRadioGroup value={String(speed)} onValueChange={(v) => setSpeed(Number(v))}>
                  {SPEEDS.map((s) => (
                    <DropdownMenuRadioItem key={s} value={String(s)}>
                      {s}×
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
            <Tip content="Fullscreen (f)">
              <Button variant="ghost" size="icon-sm" onClick={toggleFullscreen} aria-label={fullscreen ? 'Exit fullscreen' : 'Fullscreen'}>
                {fullscreen ? <Minimize /> : <Maximize />}
              </Button>
            </Tip>
          </div>
        </div>
      </div>
    </div>
  )
}
