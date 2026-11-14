import { describe, expect, it } from 'vitest'
import type { LogRow } from '@/api/telemetry-types'
import { normLog, normLogHistogram } from '@/api/telemetry'
import { computeFacets, matchesLog, parseLogQuery, suggestLogKeys, suggestLogValues, toggleLogFilter } from './query'

const row = (over: Partial<LogRow> = {}): LogRow => ({
  id: '1',
  timestamp: 0,
  project_id: '1',
  trace_id: 'ABCDEF0123456789abcdef0123456789'.toLowerCase(),
  span_id: '1122334455667788',
  level: 'error',
  severity_number: 17,
  body: 'Payment provider request failed: timeout after 10000 ms',
  attributes: { 'http.response.status_code': 504, 'payment.provider': 'stripe', 'service.name': 'api' },
  environment: 'production',
  release: '1.4.2',
  sdk: 'sentry.python',
  ...over,
})

describe('parseLogQuery', () => {
  it('splits levels, trace, attributes and text', () => {
    const q = parseLogQuery('level:[warn,error] trace:ABCDEF01-2345-6789-abcd-ef0123456789 payment.provider:stripe !environment:staging "timeout after" failed')
    expect(q.levels).toEqual(['warn', 'error'])
    expect(q.trace).toBe('abcdef0123456789abcdef0123456789')
    expect(q.filters).toEqual([
      { key: 'payment.provider', values: ['stripe'], op: '=', negated: false },
      { key: 'environment', values: ['staging'], op: '=', negated: true },
    ])
    expect(q.text).toEqual(['timeout after', 'failed'])
  })

  it('understands level aliases, minimum levels and exclusions', () => {
    expect(parseLogQuery('level:warning').levels).toEqual(['warn'])
    expect(parseLogQuery('level:>=warn').levels).toEqual(['warn', 'error', 'fatal'])
    expect(parseLogQuery('!level:debug').excludedLevels).toEqual(['debug'])
    expect(parseLogQuery('level:').levels).toEqual([])
  })
})

describe('matchesLog', () => {
  it('matches levels, attributes with operators, wildcards and body text', () => {
    const r = row()
    expect(matchesLog(r, parseLogQuery('level:error'))).toBe(true)
    expect(matchesLog(r, parseLogQuery('level:info'))).toBe(false)
    expect(matchesLog(r, parseLogQuery('http.response.status_code:>=500'))).toBe(true)
    expect(matchesLog(r, parseLogQuery('http.response.status_code:<500'))).toBe(false)
    expect(matchesLog(r, parseLogQuery('payment.provider:str*'))).toBe(true)
    expect(matchesLog(r, parseLogQuery('!payment.provider:stripe'))).toBe(false)
    expect(matchesLog(r, parseLogQuery('TIMEOUT provider'))).toBe(true)
    expect(matchesLog(r, parseLogQuery('"timeout before"'))).toBe(false)
    expect(matchesLog(r, parseLogQuery(`trace:${r.trace_id}`))).toBe(true)
    expect(matchesLog(r, parseLogQuery('trace:0000'))).toBe(false)
    expect(matchesLog(r, parseLogQuery('environment:production release:1.4.2'))).toBe(true)
    expect(matchesLog(r, parseLogQuery('missing.key:x'))).toBe(false)
  })
})

describe('toggleLogFilter', () => {
  it('adds, replaces and removes filters', () => {
    expect(toggleLogFilter('timeout', 'level', 'error')).toBe('timeout level:error')
    expect(toggleLogFilter('timeout level:error', 'level', 'error')).toBe('timeout')
    expect(toggleLogFilter('level:error', 'level', 'warn')).toBe('level:warn')
    expect(toggleLogFilter('level:error', 'level', 'error', 'exclude')).toBe('!level:error')
    expect(toggleLogFilter('', 'route', '/api/a b')).toBe('route:"/api/a b"')
  })
})

describe('facets and suggestions', () => {
  it('counts top values per attribute and skips per-row unique ones', () => {
    const rows = Array.from({ length: 40 }, (_, i) => row({ id: String(i), level: i % 4 === 0 ? 'warn' : 'info', attributes: { 'service.name': i % 2 ? 'api' : 'worker', 'request.id': `r${i}` } }))
    const facets = computeFacets(rows)
    expect(facets[0].key).toBe('level')
    expect(facets[0].values[0]).toEqual({ value: 'info', count: 30 })
    expect(facets.find((f) => f.key === 'service.name')?.values).toHaveLength(2)
    expect(facets.find((f) => f.key === 'request.id')).toBeUndefined()
  })

  it('suggests built-in and attribute keys, and level values', () => {
    expect(suggestLogKeys('le', ['service.name']).map((s) => s.label)[0]).toBe('level')
    expect(suggestLogKeys('serv', ['service.name']).map((s) => s.label)).toEqual(['service.name'])
    expect(suggestLogValues('level', 'er', []).map((s) => s.label)).toEqual(['error'])
    expect(suggestLogValues('route', '', ['/a b'])[0].insert).toBe('"/a b"')
  })
})

describe('log normalisers', () => {
  it('flattens typed attributes and maps severity numbers', () => {
    const l = normLog({ timestamp: 1700000000.25, severity_number: 13, body: 'x', attributes: { a: { type: 'integer', value: 3 }, b: 'plain' } })
    expect(l.level).toBe('warn')
    expect(l.timestamp).toBe(1700000000250)
    expect(l.attributes).toEqual({ a: 3, b: 'plain' })
  })

  it('reads histograms in several shapes', () => {
    const a = normLogHistogram({ interval: 60, data: [{ ts: '2026-01-01T00:00:00Z', counts: { error: 2, warning: 1 } }] })
    expect(a.interval_ms).toBe(60_000)
    expect(a.buckets[0].counts).toEqual({ error: 2, warn: 1 })
    const b = normLogHistogram([
      { ts: 1000, level: 'info', count: 4 },
      { ts: 1000, level: 'error', count: 1 },
      { ts: 61, level: 'info', count: 2 },
    ])
    expect(b.buckets).toHaveLength(2)
    expect(b.buckets[1].counts).toEqual({ info: 4, error: 1 })
  })
})
