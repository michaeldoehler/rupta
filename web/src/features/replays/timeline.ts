// Reads a session recording (rrweb events plus the custom events the
// browser SDK's replay integration adds) into what the player shows:
// timeline markers, breadcrumbs, console messages and network requests.
// Pure functions, unit tested.

import type { RecordingEvent, ReplayError } from '@/api/telemetry-types'

// rrweb event and incremental source numbers (rrweb's EventType / IncrementalSource).
export const EV = { DomContentLoaded: 0, Load: 1, FullSnapshot: 2, Incremental: 3, Meta: 4, Custom: 5 } as const
const SOURCE_MOUSE_INTERACTION = 2
const SOURCE_INPUT = 5
const MOUSE_CLICK = 2

export type MarkerKind = 'error' | 'click' | 'navigation'

export interface Marker {
  kind: MarkerKind
  /** Milliseconds from the start of the recording. */
  offset: number
  label: string
  /** For errors: event id; for navigation: URL. */
  ref?: string
}

export interface Crumb {
  offset: number
  timestamp: number
  category: string
  level: string
  message: string
  data: Record<string, unknown>
}

export interface NetworkRequest {
  offset: number
  timestamp: number
  duration: number
  op: string
  method: string
  url: string
  status: number | null
  size: number | null
}

type Raw = Record<string, unknown>
const obj = (v: unknown): Raw => (v && typeof v === 'object' ? (v as Raw) : {})
/** SDK timestamps are seconds; rrweb's are milliseconds. */
const sdkMs = (v: unknown) => {
  const n = Number(v)
  if (!Number.isFinite(n)) return 0
  return n < 1e11 ? n * 1000 : n
}

export function recordingStart(events: RecordingEvent[]): number {
  return events.length ? events[0].timestamp : 0
}

export function recordingDuration(events: RecordingEvent[]): number {
  return events.length ? events[events.length - 1].timestamp - events[0].timestamp : 0
}

function custom(e: RecordingEvent): { tag: string; payload: Raw } | null {
  if (e.type !== EV.Custom) return null
  const d = obj(e.data)
  return { tag: String(d.tag ?? ''), payload: obj(d.payload) }
}

/** Breadcrumbs the SDK recorded (custom events tagged "breadcrumb"). */
export function extractBreadcrumbs(events: RecordingEvent[]): Crumb[] {
  const start = recordingStart(events)
  const out: Crumb[] = []
  for (const e of events) {
    const c = custom(e)
    if (!c || c.tag !== 'breadcrumb') continue
    const p = c.payload
    const category = String(p.category ?? 'default')
    // Internal bookkeeping crumbs are noise for people.
    if (category.startsWith('replay.') || category === 'sentry.transaction') continue
    const ts = p.timestamp !== undefined ? sdkMs(p.timestamp) : e.timestamp
    out.push({
      offset: Math.max(0, ts - start),
      timestamp: ts,
      category,
      level: String(p.level ?? 'info'),
      message: String(p.message ?? ''),
      data: obj(p.data),
    })
  }
  return out.sort((a, b) => a.timestamp - b.timestamp)
}

export function extractConsole(events: RecordingEvent[]): Crumb[] {
  return extractBreadcrumbs(events).filter((c) => c.category === 'console')
}

/** Network requests from performance spans (fetch, XHR and resources). */
export function extractNetwork(events: RecordingEvent[]): NetworkRequest[] {
  const start = recordingStart(events)
  const out: NetworkRequest[] = []
  for (const e of events) {
    const c = custom(e)
    if (!c || c.tag !== 'performanceSpan') continue
    const p = c.payload
    const op = String(p.op ?? '')
    if (!op.startsWith('resource.')) continue
    const s = sdkMs(p.startTimestamp)
    const end = sdkMs(p.endTimestamp)
    const d = obj(p.data)
    const response = obj(d.response)
    const size = Number(response.size ?? d.responseBodySize ?? d.encodedBodySize ?? NaN)
    out.push({
      offset: Math.max(0, s - start),
      timestamp: s,
      duration: Math.max(0, end - s),
      op,
      method: String(d.method ?? (op === 'resource.fetch' || op === 'resource.xhr' ? 'GET' : '')),
      url: String(p.description ?? ''),
      status: d.statusCode !== undefined ? Number(d.statusCode) : null,
      size: Number.isFinite(size) ? size : null,
    })
  }
  return out.sort((a, b) => a.timestamp - b.timestamp)
}

/**
 * Timeline markers: errors from the replay's linked events, clicks (the
 * SDK's ui.click breadcrumbs, or rrweb click interactions if there are
 * none) and navigations (navigation spans or breadcrumbs, or page loads).
 */
export function buildMarkers(events: RecordingEvent[], errors: ReplayError[] = []): Marker[] {
  const start = recordingStart(events)
  const duration = recordingDuration(events)
  const clamp = (t: number) => Math.min(duration, Math.max(0, t - start))
  const markers: Marker[] = []
  const crumbs = extractBreadcrumbs(events)

  const clickCrumbs = crumbs.filter((c) => c.category === 'ui.click')
  if (clickCrumbs.length) {
    for (const c of clickCrumbs) markers.push({ kind: 'click', offset: clamp(c.timestamp), label: c.message || 'Click' })
  } else {
    for (const e of events) {
      const d = obj(e.data)
      if (e.type === EV.Incremental && d.source === SOURCE_MOUSE_INTERACTION && d.type === MOUSE_CLICK)
        markers.push({ kind: 'click', offset: clamp(e.timestamp), label: 'Click' })
    }
  }

  const navs: Marker[] = []
  for (const e of events) {
    const c = custom(e)
    if (c?.tag === 'performanceSpan' && String(c.payload.op ?? '').startsWith('navigation.')) {
      const url = String(c.payload.description ?? '')
      navs.push({ kind: 'navigation', offset: clamp(sdkMs(c.payload.startTimestamp)), label: url, ref: url })
    }
  }
  if (!navs.length) {
    for (const c of crumbs)
      if (c.category === 'navigation')
        navs.push({ kind: 'navigation', offset: clamp(c.timestamp), label: String(c.data.to ?? c.message), ref: String(c.data.to ?? '') })
  }
  if (!navs.length) {
    for (const e of events) {
      if (e.type === EV.Meta) {
        const href = String(obj(e.data).href ?? '')
        navs.push({ kind: 'navigation', offset: clamp(e.timestamp), label: href, ref: href })
      }
    }
  }
  // The same URL reported twice within 50 ms is one navigation.
  for (const n of navs) if (!markers.some((m) => m.kind === 'navigation' && m.ref === n.ref && Math.abs(m.offset - n.offset) < 50)) markers.push(n)

  const timed = errors.filter((e) => e.timestamp)
  for (const err of timed) markers.push({ kind: 'error', offset: clamp(err.timestamp), label: err.title, ref: err.event_id })
  // Without error times, console errors the SDK recorded mark where things failed.
  if (!timed.length)
    for (const c of crumbs)
      if (c.category === 'console' && c.level === 'error') markers.push({ kind: 'error', offset: clamp(c.timestamp), label: c.message || 'Console error' })
  return markers.sort((a, b) => a.offset - b.offset)
}

/** Counts used in the list and header when the server did not count them. */
export function countInteractions(events: RecordingEvent[]): { clicks: number; inputs: number } {
  let clicks = 0
  let inputs = 0
  for (const e of events) {
    if (e.type !== EV.Incremental) continue
    const d = obj(e.data)
    if (d.source === SOURCE_MOUSE_INTERACTION && d.type === MOUSE_CLICK) clicks += 1
    else if (d.source === SOURCE_INPUT) inputs += 1
  }
  return { clicks, inputs }
}

/** The URL shown at an offset: the last navigation at or before it. */
export function urlAt(markers: Marker[], offset: number): string | null {
  let url: string | null = null
  for (const m of markers) {
    if (m.kind !== 'navigation') continue
    if (m.offset > offset) break
    url = m.ref ?? m.label
  }
  return url
}

/** Formats a playback offset as m:ss (or h:mm:ss). */
export function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  return h ? `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}` : `${m}:${String(s).padStart(2, '0')}`
}
