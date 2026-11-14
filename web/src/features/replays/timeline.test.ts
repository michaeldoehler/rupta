import { describe, expect, it } from 'vitest'
import type { RecordingEvent } from '@/api/telemetry-types'
import { normRecording, normReplay } from '@/api/telemetry'
import { RECORDING } from '@/mocks/fixtures/telemetry'
import { buildMarkers, clock, countInteractions, extractBreadcrumbs, extractConsole, extractNetwork, recordingDuration, urlAt } from './timeline'

const T0 = 1_700_000_000_000
const crumb = (at: number, category: string, message = '', data = {}): RecordingEvent => ({ type: 5, timestamp: T0 + at, data: { tag: 'breadcrumb', payload: { timestamp: (T0 + at) / 1000, category, message, data } } })
const perf = (at: number, op: string, description: string, dur = 100, data = {}): RecordingEvent => ({
  type: 5,
  timestamp: T0 + at,
  data: { tag: 'performanceSpan', payload: { op, description, startTimestamp: (T0 + at) / 1000, endTimestamp: (T0 + at + dur) / 1000, data } },
})

const events: RecordingEvent[] = [
  { type: 4, timestamp: T0, data: { href: 'https://shop.test/', width: 1280, height: 720 } },
  { type: 2, timestamp: T0 + 5, data: {} },
  perf(10, 'navigation.navigate', 'https://shop.test/'),
  crumb(1000, 'ui.click', 'button.add "Add to cart"'),
  { type: 3, timestamp: T0 + 1001, data: { source: 2, type: 2, id: 9 } },
  perf(1010, 'resource.fetch', 'https://shop.test/api/cart', 40, { method: 'POST', statusCode: 201, response: { size: 12 } }),
  crumb(2000, 'console', 'Checkout failed', {}),
  crumb(2100, 'replay.mutations', 'internal'),
  perf(3000, 'navigation.push', 'https://shop.test/cart'),
  { type: 3, timestamp: T0 + 9000, data: { source: 5, text: 'x' } },
]

describe('replay timeline', () => {
  it('builds error, click and navigation markers in time order', () => {
    const markers = buildMarkers(events, [{ event_id: 'e1', issue_id: '4', short_id: 'WEB-4', title: 'TypeError', timestamp: T0 + 2500 }])
    expect(markers.map((m) => [m.kind, m.offset])).toEqual([
      ['navigation', 10],
      ['click', 1000],
      ['error', 2500],
      ['navigation', 3000],
    ])
    expect(markers.find((m) => m.kind === 'error')?.ref).toBe('e1')
    expect(urlAt(markers, 2000)).toBe('https://shop.test/')
    expect(urlAt(markers, 3500)).toBe('https://shop.test/cart')
  })

  it('falls back to rrweb clicks and meta events without SDK events', () => {
    const plain = events.filter((e) => e.type !== 5)
    const markers = buildMarkers(plain)
    expect(markers.map((m) => m.kind)).toEqual(['navigation', 'click'])
    expect(markers[1].offset).toBe(1001)
  })

  it('marks console errors when the errors have no times', () => {
    const withConsoleError = [...events.slice(0, 6), { type: 5, timestamp: T0 + 2200, data: { tag: 'breadcrumb', payload: { timestamp: (T0 + 2200) / 1000, category: 'console', level: 'error', message: 'Checkout failed (502)' } } }, ...events.slice(6)]
    const markers = buildMarkers(withConsoleError, [{ event_id: 'e1', issue_id: '4', short_id: 'WEB-4', title: 'TypeError', timestamp: 0 }])
    expect(markers.filter((m) => m.kind === 'error').map((m) => [m.offset, m.label])).toEqual([[2200, 'Checkout failed (502)']])
  })

  it('clamps markers into the recording', () => {
    const markers = buildMarkers(events, [{ event_id: 'late', issue_id: null, short_id: null, title: 'x', timestamp: T0 + 60_000 }])
    expect(markers.at(-1)?.offset).toBe(recordingDuration(events))
  })

  it('extracts breadcrumbs without internal ones, console messages and network requests', () => {
    expect(extractBreadcrumbs(events).map((c) => c.category)).toEqual(['ui.click', 'console'])
    expect(extractConsole(events)[0].message).toBe('Checkout failed')
    const net = extractNetwork(events)
    expect(net).toEqual([{ offset: 1010, timestamp: T0 + 1010, duration: 40, op: 'resource.fetch', method: 'POST', url: 'https://shop.test/api/cart', status: 201, size: 12 }])
    expect(countInteractions(events)).toEqual({ clicks: 1, inputs: 1 })
  })

  it('formats playback time', () => {
    expect(clock(0)).toBe('0:00')
    expect(clock(61_500)).toBe('1:01')
    expect(clock(3_725_000)).toBe('1:02:05')
  })

  it('reads the recorded fixture: real rrweb snapshot plus SDK events', () => {
    const rec = normRecording([RECORDING.events.slice(0, 40), RECORDING.events.slice(40)])
    expect(rec.length).toBe(RECORDING.events.length)
    expect(rec.some((e) => e.type === 2)).toBe(true)
    const markers = buildMarkers(rec)
    expect(markers.filter((m) => m.kind === 'click').length).toBeGreaterThan(5)
    expect(markers.filter((m) => m.kind === 'navigation').length).toBeGreaterThanOrEqual(3)
    expect(extractNetwork(rec).some((r) => r.status === 502)).toBe(true)
  })

  it('normalises replay rows with durations in seconds', () => {
    const r = normReplay({ id: 'r1', project_id: 3, started: '2026-01-01T00:00:00Z', finished: '2026-01-01T00:01:00Z', duration: 60, user_email: 'a@b.c', error_ids: ['x'] })
    expect(r.duration_ms).toBe(60_000)
    expect(r.user.email).toBe('a@b.c')
    expect(r.count_errors).toBe(1)
  })
})
