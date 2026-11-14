// Log search query: same grammar as the issue search (lib/search/parse.ts).
//
//   level:error            level:[warn,error]        !level:debug
//   trace:<trace id>       span:<span id>
//   environment:production release:1.4.2
//   http.status_code:>=500 user.id:42                 any attribute key
//   "connection reset"     timeout                    text in the body
//
// The server evaluates the query; this module parses it for highlighting,
// autocomplete, facets, live-tail filtering and the mock API.

import type { LogLevel, LogRow } from '@/api/telemetry-types'
import { formatFilter, tokenize, type FilterToken, type Operator } from '@/lib/search/parse'
import type { Suggestion } from '@/lib/search/autocomplete'
import { quoteValue } from '@/lib/search/parse'

export const LOG_LEVELS: LogLevel[] = ['trace', 'debug', 'info', 'warn', 'error', 'fatal']

/** Keys with a meaning of their own; anything else is an attribute. */
export const LOG_KEYS: { key: string; description: string; values?: string[] }[] = [
  { key: 'level', description: 'Severity', values: LOG_LEVELS },
  { key: 'trace', description: 'Trace id' },
  { key: 'span', description: 'Span id' },
  { key: 'environment', description: 'Environment' },
  { key: 'release', description: 'Release version' },
  { key: 'sdk', description: 'SDK name' },
]
const BUILTIN = new Set(LOG_KEYS.map((k) => k.key))

export interface LogFilter {
  key: string
  values: string[]
  op: Operator
  negated: boolean
}

export interface LogQuery {
  levels: LogLevel[]
  excludedLevels: LogLevel[]
  trace: string | null
  span: string | null
  /** environment, release, sdk and attribute filters. */
  filters: LogFilter[]
  text: string[]
}

const LEVEL_ALIASES: Record<string, LogLevel> = { warning: 'warn', critical: 'fatal', err: 'error' }

function toLevel(v: string): LogLevel | null {
  const s = v.toLowerCase()
  if ((LOG_LEVELS as string[]).includes(s)) return s as LogLevel
  return LEVEL_ALIASES[s] ?? null
}

export function parseLogQuery(input: string): LogQuery {
  const q: LogQuery = { levels: [], excludedLevels: [], trace: null, span: null, filters: [], text: [] }
  for (const t of tokenize(input)) {
    if (t.type === 'text') {
      if (t.value) q.text.push(t.value)
      continue
    }
    const f = t as FilterToken
    if (f.incomplete) continue
    if (f.key === 'level') {
      const levels = f.values.map(toLevel).filter((l): l is LogLevel => l !== null)
      if (f.negated) q.excludedLevels.push(...levels)
      else if (f.op === '>=' || f.op === '>') {
        // level:>=warn → warn, error, fatal
        const min = levels[0] ? LOG_LEVELS.indexOf(levels[0]) + (f.op === '>' ? 1 : 0) : 0
        q.levels.push(...LOG_LEVELS.slice(min))
      } else q.levels.push(...levels)
    } else if (f.key === 'trace' && !f.negated) q.trace = f.values[0]?.toLowerCase().replace(/-/g, '') ?? null
    else if (f.key === 'span' && !f.negated) q.span = f.values[0]?.toLowerCase() ?? null
    else q.filters.push({ key: f.key, values: f.values, op: f.op, negated: f.negated })
  }
  return q
}

function fieldValue(row: LogRow, key: string): unknown {
  if (key === 'environment') return row.environment
  if (key === 'release') return row.release
  if (key === 'sdk') return row.sdk
  if (key === 'body' || key === 'message') return row.body
  return row.attributes[key]
}

function compare(actual: unknown, op: Operator, expected: string): boolean {
  if (op === '=') {
    if (actual === null || actual === undefined) return expected === '' || expected === 'null'
    const a = String(actual)
    if (expected.includes('*')) {
      const re = new RegExp(`^${expected.split('*').map(escapeRe).join('.*')}$`, 'i')
      return re.test(a)
    }
    return a.toLowerCase() === expected.toLowerCase()
  }
  const a = typeof actual === 'number' ? actual : Number(actual)
  const b = Number(expected)
  if (!Number.isFinite(a) || !Number.isFinite(b)) return false
  return op === '>' ? a > b : op === '>=' ? a >= b : op === '<' ? a < b : a <= b
}

function escapeRe(s: string) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/** Whether a row matches the query (client-side, as the server would). */
export function matchesLog(row: LogRow, q: LogQuery): boolean {
  if (q.levels.length && !q.levels.includes(row.level)) return false
  if (q.excludedLevels.includes(row.level)) return false
  if (q.trace && (row.trace_id ?? '').toLowerCase() !== q.trace) return false
  if (q.span && (row.span_id ?? '').toLowerCase() !== q.span) return false
  for (const f of q.filters) {
    const v = fieldValue(row, f.key)
    const hit = f.values.some((x) => compare(v, f.op, x))
    if (hit === f.negated) return false
  }
  if (q.text.length) {
    const body = row.body.toLowerCase()
    if (!q.text.every((t) => body.includes(t.toLowerCase()))) return false
  }
  return true
}

/** Adds `key:value` to the query, replacing an existing filter for the key (or removing it with `exclude`). */
export function toggleLogFilter(input: string, key: string, value: string, mode: 'include' | 'exclude' = 'include'): string {
  const tokens = tokenize(input)
  const negated = mode === 'exclude'
  const existing = tokens.find((t) => t.type === 'filter' && t.key === key && t.negated === negated && t.values.length === 1 && t.values[0] === value)
  const kept = tokens.filter((t) => !(t.type === 'filter' && t.key === key && (t.negated === negated || (t.values.length === 1 && t.values[0] === value))))
  const parts = kept.map((t) => t.raw)
  if (!existing) parts.push(formatFilter(key, value, { negated }))
  return parts.join(' ')
}

export interface Facet {
  key: string
  /** Rows that have the attribute. */
  count: number
  values: { value: string; count: number }[]
}

const FACET_SKIP = new Set(['sentry.message.template', 'sentry.message.parameter.0', 'sentry.message.parameter.1', 'sentry.message.parameter.2'])

/** Top values per attribute over the given rows, most common keys first. */
export function computeFacets(rows: LogRow[], opts: { maxKeys?: number; maxValues?: number } = {}): Facet[] {
  const maxKeys = opts.maxKeys ?? 24
  const maxValues = opts.maxValues ?? 6
  const byKey = new Map<string, Map<string, number>>()
  const add = (k: string, v: unknown) => {
    if (v === null || v === undefined || v === '' || typeof v === 'object') return
    const s = String(v)
    if (s.length > 120) return
    let m = byKey.get(k)
    if (!m) byKey.set(k, (m = new Map()))
    m.set(s, (m.get(s) ?? 0) + 1)
  }
  for (const r of rows) {
    add('level', r.level)
    add('environment', r.environment)
    add('release', r.release)
    for (const [k, v] of Object.entries(r.attributes)) if (!FACET_SKIP.has(k)) add(k, v)
  }
  const facets: Facet[] = []
  for (const [key, m] of byKey) {
    const values = [...m.entries()].map(([value, count]) => ({ value, count })).sort((a, b) => b.count - a.count)
    const count = values.reduce((s, v) => s + v.count, 0)
    // Values unique per row (ids, timestamps) make poor facets.
    if (key !== 'level' && values.length > 0.6 * count && count > 20) continue
    facets.push({ key, count, values: values.slice(0, maxValues) })
  }
  const pinned = ['level', 'environment', 'release']
  return facets
    .sort((a, b) => {
      const pa = pinned.indexOf(a.key)
      const pb = pinned.indexOf(b.key)
      if (pa !== -1 || pb !== -1) return (pa === -1 ? 99 : pa) - (pb === -1 ? 99 : pb)
      return b.count - a.count
    })
    .slice(0, maxKeys)
}

function score(candidate: string, partial: string): number {
  if (!partial) return 1
  const c = candidate.toLowerCase()
  const p = partial.toLowerCase()
  if (c.startsWith(p)) return 3
  if (c.split(/[._-]/).some((part) => part.startsWith(p))) return 2
  return c.includes(p) ? 1 : 0
}

/** Key suggestions: built-in keys plus attribute keys seen in the data. */
export function suggestLogKeys(partial: string, attributeKeys: string[], limit = 12): Suggestion[] {
  const all = [
    ...LOG_KEYS.map((k) => ({ key: k.key, description: k.description })),
    ...attributeKeys.filter((k) => !BUILTIN.has(k)).map((k) => ({ key: k, description: 'Attribute' })),
  ]
  return all
    .map((k) => ({ k, s: score(k.key, partial) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s)
    .slice(0, limit)
    .map(({ k }) => ({ kind: 'key' as const, label: k.key, description: k.description, insert: `${k.key}:`, key: k.key }))
}

/** Value suggestions: fixed levels, or values seen in the data. */
export function suggestLogValues(key: string, partial: string, seen: string[], limit = 12): Suggestion[] {
  const fixed = LOG_KEYS.find((k) => k.key === key)?.values ?? []
  const values = [...fixed, ...seen.filter((v) => !fixed.includes(v))]
  return values
    .map((v) => ({ v, s: score(v, partial) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s)
    .slice(0, limit)
    .map(({ v }) => ({ kind: 'value' as const, label: v, insert: quoteValue(v), key }))
}
