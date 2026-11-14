// Formatting and colours shared by the performance, logs, metrics,
// profiling and replay pages.

import type { LogLevel } from '@/api/telemetry-types'
import { bytes, compact, duration } from '@/lib/format'

export const SERIES_COLORS = ['var(--series-1)', 'var(--series-2)', 'var(--series-3)', 'var(--series-4)', 'var(--series-5)']
/** Charts show at most this many series; the rest are listed in the table. */
export const MAX_SERIES = SERIES_COLORS.length

/** Compact duration for dense tables: 0.42 ms, 12 ms, 1.24 s, 2m 5s. */
export function ms(v: number | null | undefined): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return '—'
  if (v === 0) return '0 ms'
  if (v < 1) return `${v.toFixed(2)} ms`
  if (v < 10) return `${v.toFixed(1)} ms`
  return duration(v)
}

export function pct(v: number | null | undefined, digits = 1): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return '—'
  const p = v * 100
  if (p > 0 && p < 0.01) return '<0.01%'
  return `${p.toFixed(p < 1 ? 2 : digits)}%`
}

export function rate(v: number): string {
  if (v >= 100) return compact(Math.round(v))
  if (v >= 1) return v.toFixed(1)
  return v.toFixed(2)
}

/** A metric value with its unit (as the SDKs name units). */
export function withUnit(v: number | null | undefined, unit: string | null | undefined): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return '—'
  switch (unit) {
    case 'millisecond':
      return ms(v)
    case 'second':
      return ms(v * 1000)
    case 'microsecond':
      return ms(v / 1000)
    case 'nanosecond':
      return ms(v / 1e6)
    case 'byte':
      return bytes(Math.round(v))
    case 'kilobyte':
      return bytes(Math.round(v * 1000))
    case 'megabyte':
      return bytes(Math.round(v * 1e6))
    case 'percent':
      return `${v.toFixed(1)}%`
    case 'ratio':
      return pct(v)
    case 'euro':
      return `€${v.toLocaleString('en-US', { maximumFractionDigits: 2 })}`
    case 'dollar':
    case 'usd':
      return `$${v.toLocaleString('en-US', { maximumFractionDigits: 2 })}`
    default: {
      const abs = Math.abs(v)
      const s = abs >= 1000 ? compact(Math.round(v)) : abs >= 10 ? String(Math.round(v * 10) / 10) : String(Math.round(v * 100) / 100)
      return unit && unit !== 'none' ? `${s} ${unit}` : s
    }
  }
}

export const LOG_LEVEL_DOT: Record<LogLevel, string> = {
  trace: 'bg-level-debug/60',
  debug: 'bg-level-debug',
  info: 'bg-level-info',
  warn: 'bg-level-warning',
  error: 'bg-level-error',
  fatal: 'bg-level-fatal',
}

export const LOG_LEVEL_TEXT: Record<LogLevel, string> = {
  trace: 'text-muted-foreground',
  debug: 'text-muted-foreground',
  info: 'text-level-info',
  warn: 'text-warning',
  error: 'text-level-error',
  fatal: 'text-level-fatal',
}

export const LOG_LEVEL_VAR: Record<LogLevel, string> = {
  trace: 'color-mix(in oklch, var(--level-debug) 55%, transparent)',
  debug: 'var(--level-debug)',
  info: 'var(--level-info)',
  warn: 'var(--level-warning)',
  error: 'var(--level-error)',
  fatal: 'var(--level-fatal)',
}

/** Bar colour of a span by op category; the op text is always shown next to it. */
export const OP_COLOR: Record<string, string> = {
  http: 'var(--series-2)',
  db: 'var(--series-5)',
  resource: 'var(--series-3)',
  browser: 'var(--series-3)',
  ui: 'var(--series-4)',
  queue: 'var(--palm)',
  task: 'var(--palm)',
  pageload: 'var(--series-other)',
  navigation: 'var(--series-other)',
  default: 'var(--series-other)',
}

export function opColor(category: string): string {
  return OP_COLOR[category] ?? 'var(--series-other)'
}

/** Apdex and failure-rate grading for table cells: good, meh, poor. */
export function apdexGrade(v: number): 'good' | 'meh' | 'poor' {
  return v >= 0.94 ? 'good' : v >= 0.85 ? 'meh' : 'poor'
}

export const GRADE_TEXT = { good: 'text-success', meh: 'text-warning', poor: 'text-destructive' } as const
