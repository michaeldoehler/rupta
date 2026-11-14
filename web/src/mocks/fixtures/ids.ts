// Deterministic ids shared by the error events and the telemetry fixtures,
// so an event's trace and replay resolve to matching mock data.

import { hashString, Rng } from '../rng'

const MARKER = 'e7e7'

/** Trace id of an error event: issue id (8 hex), a marker, then random hex. */
export function traceIdForEvent(issueId: string, index: number): string {
  const head = Number(issueId).toString(16).padStart(8, '0').slice(-8)
  return `${head}${MARKER}${new Rng(`trace:${issueId}:${index}`).hex(20)}`
}

/** The issue an event trace id was made for, if any. */
export function issueFromTraceId(traceId: string): string | null {
  if (traceId.length !== 32 || traceId.slice(8, 12) !== MARKER) return null
  const n = parseInt(traceId.slice(0, 8), 16)
  return Number.isFinite(n) ? String(n) : null
}

/** Span id of the span the error happened in. */
export function errorSpanId(traceId: string): string {
  return new Rng(`error-span:${traceId}`).hex(16)
}

export const REPLAY_COUNT = 140

export function replayIdAt(i: number): string {
  return new Rng(`replay:${i}`).hex(32)
}

/** Indexes of the replays that captured an issue's errors (browser issues only). */
export function replaysForIssue(issueId: string): number[] {
  const h = hashString(`issue-replays:${issueId}`)
  const n = 2 + (h % 4)
  const first = h % REPLAY_COUNT
  return Array.from({ length: n }, (_, k) => (first + k * 17) % REPLAY_COUNT)
}
