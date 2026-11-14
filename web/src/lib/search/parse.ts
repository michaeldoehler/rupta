// Issue search query language, client side: tokenising for highlighting,
// autocomplete and the mock server. The server has its own parser
// (rupta-core); both follow the same grammar:
//
//   query    := (term ws)*
//   term     := filter | text
//   filter   := ["!"] key ":" [op] value
//   op       := ">" | ">=" | "<" | "<="
//   value    := word | quoted | "[" value ("," value)* "]"
//   text     := word | quoted
//
// Keys are letters, digits, "_", ".", "-" and "[...]" (tags[my key]).

export type Operator = '=' | '>' | '>=' | '<' | '<='

export interface FilterToken {
  type: 'filter'
  key: string
  op: Operator
  /** One value, or several for `key:[a,b]`. */
  values: string[]
  negated: boolean
  /** `key:` with nothing after it yet (while typing). */
  incomplete: boolean
  quoted: boolean
  start: number
  end: number
  raw: string
  /** Offset where the value part starts (after `key:` and the operator). */
  valueStart: number
}

export interface TextToken {
  type: 'text'
  value: string
  quoted: boolean
  start: number
  end: number
  raw: string
}

export type Token = FilterToken | TextToken

const KEY_RE = /^!?(?:[A-Za-z_][\w.-]*|tags\[[^\]]+\])$/

function readQuoted(input: string, i: number): { value: string; end: number; closed: boolean } {
  // input[i] === '"'
  let j = i + 1
  let value = ''
  while (j < input.length && input[j] !== '"') {
    if (input[j] === '\\' && j + 1 < input.length) {
      value += input[j + 1]
      j += 2
    } else {
      value += input[j]
      j += 1
    }
  }
  const closed = j < input.length
  return { value, end: closed ? j + 1 : j, closed }
}

function readBareWord(input: string, i: number): number {
  let j = i
  while (j < input.length && !/\s/.test(input[j])) j += 1
  return j
}

function parseValue(input: string, start: number): { values: string[]; end: number; quoted: boolean } {
  if (start >= input.length || /\s/.test(input[start])) return { values: [], end: start, quoted: false }
  if (input[start] === '"') {
    const q = readQuoted(input, start)
    return { values: [q.value], end: q.end, quoted: true }
  }
  if (input[start] === '[') {
    const close = input.indexOf(']', start)
    const end = close === -1 ? readBareWord(input, start) : close + 1
    const inner = input.slice(start + 1, close === -1 ? end : close)
    const values = inner
      .split(',')
      .map((v) => v.trim().replace(/^"(.*)"$/, '$1'))
      .filter((v) => v !== '')
    return { values, end, quoted: false }
  }
  const end = readBareWord(input, start)
  return { values: [input.slice(start, end)], end, quoted: false }
}

export function tokenize(input: string): Token[] {
  const tokens: Token[] = []
  let i = 0
  while (i < input.length) {
    if (/\s/.test(input[i])) {
      i += 1
      continue
    }
    const start = i
    if (input[i] === '"') {
      const q = readQuoted(input, i)
      tokens.push({ type: 'text', value: q.value, quoted: true, start, end: q.end, raw: input.slice(start, q.end) })
      i = q.end
      continue
    }
    // Find a key followed by ':' before whitespace.
    let j = i
    let inBracket = false
    while (j < input.length && (inBracket || !/[\s:"]/.test(input[j]))) {
      if (input[j] === '[') inBracket = true
      else if (input[j] === ']') inBracket = false
      j += 1
    }
    const candidate = input.slice(i, j)
    if (input[j] === ':' && KEY_RE.test(candidate)) {
      const negated = candidate.startsWith('!')
      const key = negated ? candidate.slice(1) : candidate
      let k = j + 1
      let op: Operator = '='
      const opMatch = /^(>=|<=|>|<)/.exec(input.slice(k))
      if (opMatch) {
        op = opMatch[1] as Operator
        k += opMatch[1].length
      }
      const valueStart = k
      const { values, end, quoted } = parseValue(input, k)
      tokens.push({
        type: 'filter',
        key,
        op,
        values,
        negated,
        incomplete: values.length === 0,
        quoted,
        start,
        end,
        raw: input.slice(start, end),
        valueStart,
      })
      i = Math.max(end, j + 1)
      continue
    }
    const end = readBareWord(input, i)
    tokens.push({ type: 'text', value: input.slice(i, end), quoted: false, start, end, raw: input.slice(start, end) })
    i = end
  }
  return tokens
}

export interface ParsedQuery {
  filters: FilterToken[]
  text: string[]
}

export function parseQuery(input: string): ParsedQuery {
  const tokens = tokenize(input)
  return {
    filters: tokens.filter((t): t is FilterToken => t.type === 'filter' && !t.incomplete),
    text: tokens.filter((t): t is TextToken => t.type === 'text').map((t) => t.value),
  }
}

export function quoteValue(v: string): string {
  return /[\s",[\]]/.test(v) || v === '' ? `"${v.replace(/(["\\])/g, '\\$1')}"` : v
}

export function formatFilter(key: string, value: string | string[], opts: { negated?: boolean; op?: Operator } = {}): string {
  const op = opts.op && opts.op !== '=' ? opts.op : ''
  const v = Array.isArray(value) ? `[${value.map(quoteValue).join(',')}]` : quoteValue(value)
  return `${opts.negated ? '!' : ''}${key}:${op}${v}`
}

/** The values of the first non-negated filter with this key. */
export function getFilterValues(input: string, key: string): string[] {
  const f = parseQuery(input).filters.find((t) => t.key === key && !t.negated)
  return f ? f.values : []
}

/** Replaces (or adds/removes, with `null`) a filter in the query string. */
export function setFilter(input: string, key: string, value: string | string[] | null): string {
  const tokens = tokenize(input)
  const kept = tokens.filter((t) => !(t.type === 'filter' && t.key === key && !t.negated)).map((t) => t.raw)
  if (value !== null && !(Array.isArray(value) && value.length === 0)) kept.unshift(formatFilter(key, value))
  return kept.join(' ')
}

/** Normalises whitespace without changing meaning. */
export function normalizeQuery(input: string): string {
  return tokenize(input)
    .map((t) => t.raw)
    .join(' ')
}

/** Parses relative times like `-24h`, `-7d`, `-30m`, `-2w` into milliseconds. */
export function parseRelative(v: string): number | null {
  const m = /^[-+]?(\d+)([smhdw])$/.exec(v.trim())
  if (!m) return null
  const n = Number(m[1])
  const unit = { s: 1e3, m: 6e4, h: 3.6e6, d: 8.64e7, w: 6.048e8 }[m[2] as 's' | 'm' | 'h' | 'd' | 'w']
  return n * unit
}
