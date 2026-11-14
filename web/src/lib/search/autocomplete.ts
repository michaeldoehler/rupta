import { quoteValue, tokenize, type FilterToken, type Token } from './parse'
import { SEARCH_KEY_MAP, SEARCH_KEYS, type SearchKey } from './keys'

export type SuggestionKind = 'key' | 'value'

export interface Suggestion {
  kind: SuggestionKind
  /** What is shown (key name or value). */
  label: string
  description?: string
  /** Text that replaces the current token. */
  insert: string
  /** Cursor position inside the new query after applying. */
  key?: string
  section?: string
}

export interface CompletionContext {
  /** Nothing typed at the cursor yet, or a bare word: complete keys. */
  mode: 'key' | 'value'
  /** The token under the cursor, if any. */
  token: Token | null
  /** Partial text being completed (key prefix or value prefix). */
  partial: string
  /** The key whose values are being completed (mode = value). */
  key?: string
  negated?: boolean
  /** Range of the query to replace when a suggestion is applied. */
  replaceStart: number
  replaceEnd: number
}

/** Works out what is being typed at `cursor`. */
export function completionContext(query: string, cursor: number): CompletionContext {
  const tokens = tokenize(query)
  const token = tokens.find((t) => cursor >= t.start && cursor <= t.end) ?? null
  if (!token) return { mode: 'key', token: null, partial: '', replaceStart: cursor, replaceEnd: cursor }
  if (token.type === 'filter' && cursor >= token.valueStart) {
    const f = token as FilterToken
    let partial = query.slice(f.valueStart, cursor)
    if (partial.startsWith('"')) partial = partial.slice(1)
    // inside a list: complete the item after the last comma
    let replaceStart = f.valueStart
    if (partial.startsWith('[')) {
      const lastComma = partial.lastIndexOf(',')
      replaceStart = f.valueStart + (lastComma === -1 ? 1 : lastComma + 1)
      partial = partial.slice(lastComma === -1 ? 1 : lastComma + 1)
    }
    return {
      mode: 'value',
      token,
      partial,
      key: f.key,
      negated: f.negated,
      replaceStart,
      replaceEnd: f.end,
    }
  }
  const raw = token.raw
  const negated = raw.startsWith('!')
  return {
    mode: 'key',
    token,
    partial: query.slice(token.start + (negated ? 1 : 0), cursor),
    negated,
    replaceStart: token.start,
    replaceEnd: token.end,
  }
}

function matches(candidate: string, partial: string): number {
  if (!partial) return 1
  const c = candidate.toLowerCase()
  const p = partial.toLowerCase()
  if (c.startsWith(p)) return 3
  if (c.split(/[._-]/).some((part) => part.startsWith(p))) return 2
  if (c.includes(p)) return 1
  return 0
}

/** Key suggestions: built-in keys plus custom tag keys from the server. */
export function suggestKeys(partial: string, tagKeys: string[] = [], limit = 12): Suggestion[] {
  const all: { key: string; description: string; section: string }[] = [
    ...SEARCH_KEYS.map((k) => ({ key: k.key, description: k.description, section: k.section })),
    ...tagKeys.filter((k) => !SEARCH_KEY_MAP.has(k)).map((k) => ({ key: k, description: 'Tag', section: 'tag' })),
  ]
  return all
    .map((k) => ({ k, score: matches(k.key, partial) }))
    .filter((x) => x.score > 0)
    .sort((a, b) => b.score - a.score)
    .slice(0, limit)
    .map(({ k }) => ({ kind: 'key' as const, label: k.key, description: k.description, insert: `${k.key}:`, key: k.key, section: k.section }))
}

/** Value suggestions for a key: fixed values or values fetched for the tag. */
export function suggestValues(key: string, partial: string, dynamicValues: string[] = [], limit = 12): Suggestion[] {
  const def: SearchKey | undefined = SEARCH_KEY_MAP.get(key)
  const fixed = def?.values ?? []
  const values: { value: string; description?: string }[] = [
    ...fixed,
    ...dynamicValues.filter((v) => !fixed.some((f) => f.value === v)).map((value) => ({ value })),
  ]
  return values
    .map((v) => ({ v, score: matches(v.value, partial) }))
    .filter((x) => x.score > 0)
    .sort((a, b) => b.score - a.score)
    .slice(0, limit)
    .map(({ v }) => ({ kind: 'value' as const, label: v.value, description: v.description, insert: quoteValue(v.value), key }))
}

/** Whether the key's values must be fetched from the server. */
export function dynamicTagFor(key: string): string | null {
  const def = SEARCH_KEY_MAP.get(key)
  if (def) return def.dynamic ? (def.tag ?? key) : null
  return key // unknown keys are custom tags
}

/**
 * Applies a suggestion at the context's range. Keys leave the cursor after
 * the colon; values add a trailing space so typing continues with a new term.
 */
export function applySuggestion(query: string, ctx: CompletionContext, s: Suggestion): { query: string; cursor: number } {
  const before = query.slice(0, ctx.replaceStart)
  let after = query.slice(ctx.replaceEnd)
  let insert: string
  if (s.kind === 'key') {
    insert = `${ctx.negated ? '!' : ''}${s.insert}`
    // If the token already had a value (editing the key), keep nothing of the old token.
  } else {
    const inList = ctx.token?.type === 'filter' && query.slice((ctx.token as FilterToken).valueStart).startsWith('[')
    if (inList) {
      // keep the rest of the list
      const listEnd = query.indexOf(']', ctx.replaceStart)
      const nextComma = query.indexOf(',', ctx.replaceStart)
      const stop = nextComma !== -1 && (listEnd === -1 || nextComma < listEnd) ? nextComma : listEnd === -1 ? ctx.replaceEnd : listEnd
      after = query.slice(stop)
      insert = s.insert
    } else {
      insert = s.insert
      if (!after.startsWith(' ')) insert += ' '
    }
  }
  const next = before + insert + after
  const cursor = before.length + insert.length + (s.kind === 'value' && after.startsWith(' ') ? 1 : 0)
  return { query: next, cursor: Math.min(cursor, next.length) }
}
