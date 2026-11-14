import { describe, expect, it } from 'vitest'
import { applySuggestion, completionContext, dynamicTagFor, suggestKeys, suggestValues } from './autocomplete'

describe('completionContext', () => {
  it('completes keys on an empty query and after a space', () => {
    expect(completionContext('', 0)).toMatchObject({ mode: 'key', partial: '' })
    expect(completionContext('is:unresolved ', 14)).toMatchObject({ mode: 'key', partial: '', replaceStart: 14 })
  })

  it('completes a partial key', () => {
    expect(completionContext('is:unresolved lev', 17)).toMatchObject({ mode: 'key', partial: 'lev', replaceStart: 14, replaceEnd: 17 })
  })

  it('completes values after the colon', () => {
    expect(completionContext('level:', 6)).toMatchObject({ mode: 'value', key: 'level', partial: '' })
    expect(completionContext('level:er', 8)).toMatchObject({ mode: 'value', key: 'level', partial: 'er', replaceStart: 6 })
    expect(completionContext('!browser:"Chr', 13)).toMatchObject({ mode: 'value', key: 'browser', partial: 'Chr', negated: true })
  })

  it('completes the current item of a list', () => {
    expect(completionContext('level:[error,fa', 15)).toMatchObject({ mode: 'value', key: 'level', partial: 'fa', replaceStart: 13 })
  })
})

describe('suggestions', () => {
  it('ranks prefix matches first and includes custom tags', () => {
    const s = suggestKeys('re', ['region'])
    expect(s[0].label).toBe('release')
    expect(s.map((x) => x.label)).toContain('region')
    expect(s.every((x) => x.insert.endsWith(':'))).toBe(true)
  })

  it('suggests fixed values for known keys', () => {
    expect(suggestValues('level', 'f').map((s) => s.label)).toEqual(['fatal', 'info'])
    expect(suggestValues('level', 'war').map((s) => s.label)).toEqual(['warning'])
    expect(suggestValues('is', '').map((s) => s.label)).toContain('unresolved')
  })

  it('merges dynamic values and quotes ones with spaces', () => {
    const s = suggestValues('browser', 'saf', ['Chrome 131', 'Safari 18.1'])
    expect(s).toHaveLength(1)
    expect(s[0].insert).toBe('"Safari 18.1"')
  })

  it('knows which keys need server values', () => {
    expect(dynamicTagFor('level')).toBeNull()
    expect(dynamicTagFor('browser')).toBe('browser')
    expect(dynamicTagFor('customer_tier')).toBe('customer_tier')
  })
})

describe('applySuggestion', () => {
  it('replaces a partial key with key and colon', () => {
    const q = 'is:unresolved lev'
    const ctx = completionContext(q, q.length)
    const r = applySuggestion(q, ctx, suggestKeys('lev')[0])
    expect(r).toEqual({ query: 'is:unresolved level:', cursor: 20 })
  })

  it('keeps negation when completing a key', () => {
    const q = '!lev'
    const r = applySuggestion(q, completionContext(q, 4), suggestKeys('lev')[0])
    expect(r.query).toBe('!level:')
  })

  it('inserts a value with a trailing space', () => {
    const q = 'level:er'
    const r = applySuggestion(q, completionContext(q, q.length), suggestValues('level', 'er')[0])
    expect(r).toEqual({ query: 'level:error ', cursor: 12 })
  })

  it('completes a value in the middle of the query without breaking the rest', () => {
    const q = 'level:e is:unresolved'
    const r = applySuggestion(q, completionContext(q, 7), suggestValues('level', 'e')[0])
    expect(r.query).toBe('level:error is:unresolved')
  })

  it('completes an item inside a list', () => {
    const q = 'level:[error,fa]'
    const r = applySuggestion(q, completionContext(q, 15), suggestValues('level', 'fa')[0])
    expect(r.query).toBe('level:[error,fatal]')
  })
})
