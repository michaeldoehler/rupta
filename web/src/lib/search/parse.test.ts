import { describe, expect, it } from 'vitest'
import { formatFilter, getFilterValues, normalizeQuery, parseQuery, parseRelative, quoteValue, setFilter, tokenize } from './parse'

describe('tokenize', () => {
  it('splits filters and free text with positions', () => {
    const t = tokenize('is:unresolved level:error  TypeError')
    expect(t.map((x) => x.type)).toEqual(['filter', 'filter', 'text'])
    expect(t[0]).toMatchObject({ key: 'is', values: ['unresolved'], start: 0, end: 13, valueStart: 3 })
    expect(t[2]).toMatchObject({ type: 'text', value: 'TypeError', start: 27 })
  })

  it('handles quoted values, quoted text and escapes', () => {
    const t = tokenize('browser:"Mobile Safari 18.1" "cannot read \\"x\\""')
    expect(t[0]).toMatchObject({ type: 'filter', key: 'browser', values: ['Mobile Safari 18.1'], quoted: true })
    expect(t[1]).toMatchObject({ type: 'text', value: 'cannot read "x"', quoted: true })
  })

  it('parses negation, operators, lists and tag keys', () => {
    const [neg, op, list, tag] = tokenize('!level:info times_seen:>=100 level:[error,fatal] tags[my key]:on')
    expect(neg).toMatchObject({ key: 'level', negated: true, values: ['info'] })
    expect(op).toMatchObject({ key: 'times_seen', op: '>=', values: ['100'] })
    expect(list).toMatchObject({ key: 'level', values: ['error', 'fatal'] })
    expect(tag).toMatchObject({ key: 'tags[my key]', values: ['on'] })
  })

  it('keeps colons inside values such as URLs', () => {
    const [t] = tokenize('url:https://app.example.com/a?b=1')
    expect(t).toMatchObject({ key: 'url', values: ['https://app.example.com/a?b=1'] })
  })

  it('marks a key without value as incomplete', () => {
    const [t] = tokenize('level:')
    expect(t).toMatchObject({ type: 'filter', key: 'level', incomplete: true, values: [] })
    expect(parseQuery('level:').filters).toHaveLength(0)
  })
})

describe('query helpers', () => {
  it('parseQuery separates filters from text', () => {
    const q = parseQuery('is:unresolved checkout "failed to fetch"')
    expect(q.filters.map((f) => f.key)).toEqual(['is'])
    expect(q.text).toEqual(['checkout', 'failed to fetch'])
  })

  it('quotes values only when needed', () => {
    expect(quoteValue('error')).toBe('error')
    expect(quoteValue('Safari 18')).toBe('"Safari 18"')
    expect(quoteValue('')).toBe('""')
    expect(formatFilter('level', ['error', 'fatal'])).toBe('level:[error,fatal]')
    expect(formatFilter('times_seen', '10', { op: '>' })).toBe('times_seen:>10')
    expect(formatFilter('level', 'info', { negated: true })).toBe('!level:info')
  })

  it('setFilter replaces, adds and removes a filter', () => {
    expect(setFilter('is:unresolved level:error', 'level', 'fatal')).toBe('level:fatal is:unresolved')
    expect(setFilter('is:unresolved', 'environment', 'production')).toBe('environment:production is:unresolved')
    expect(setFilter('is:unresolved level:error', 'level', null)).toBe('is:unresolved')
    expect(getFilterValues('level:[a,b] x', 'level')).toEqual(['a', 'b'])
  })

  it('normalizes whitespace', () => {
    expect(normalizeQuery('  is:unresolved    level:error ')).toBe('is:unresolved level:error')
  })

  it('parses relative times', () => {
    expect(parseRelative('-24h')).toBe(86_400_000)
    expect(parseRelative('7d')).toBe(7 * 86_400_000)
    expect(parseRelative('soon')).toBeNull()
  })
})
