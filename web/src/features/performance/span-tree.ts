// Span tree of a trace: building, flattening for the waterfall, the
// critical path and self time. Pure functions, unit tested.

import type { TraceSpan } from '@/api/telemetry-types'

export interface SpanNode {
  span: TraceSpan
  children: SpanNode[]
  depth: number
  /** Number of spans below this one. */
  descendants: number
  /** End, epoch milliseconds. */
  end: number
}

const end = (s: TraceSpan) => s.start + s.duration_ms

/**
 * Builds the span tree. Spans whose parent is not in the trace (the root,
 * or spans of services that did not send data) become roots. Siblings are
 * ordered by start time; ties keep the input order.
 */
export function buildSpanTree(spans: TraceSpan[]): SpanNode[] {
  const nodes = new Map<string, SpanNode>()
  for (const s of spans) {
    // Duplicate span ids: keep the first, the others would make cycles.
    if (!nodes.has(s.span_id)) nodes.set(s.span_id, { span: s, children: [], depth: 0, descendants: 0, end: end(s) })
  }
  const roots: SpanNode[] = []
  const parentOf = new Map<SpanNode, SpanNode>()
  for (const n of nodes.values()) {
    const parentId = n.span.parent_span_id
    const parent = parentId && parentId !== n.span.span_id ? nodes.get(parentId) : undefined
    if (parent) {
      parent.children.push(n)
      parentOf.set(n, parent)
    } else roots.push(n)
  }
  const byStart = (a: SpanNode, b: SpanNode) => a.span.start - b.span.start
  // Iterative depth-first pass: sort children, set depth, collect the order.
  const order: SpanNode[] = []
  const visited = new Set<SpanNode>()
  const visit = (start: SpanNode[]) => {
    const stack: SpanNode[] = [...start].reverse()
    while (stack.length) {
      const n = stack.pop()!
      visited.add(n)
      order.push(n)
      n.children.sort(byStart)
      for (let i = n.children.length - 1; i >= 0; i--) {
        n.children[i].depth = n.depth + 1
        stack.push(n.children[i])
      }
    }
  }
  roots.sort(byStart)
  visit(roots)
  // Spans not reachable from a root form parent cycles: cut each cycle at
  // its earliest span, which becomes a root.
  if (visited.size < nodes.size) {
    const rest = [...nodes.values()].filter((n) => !visited.has(n)).sort(byStart)
    for (const n of rest) {
      if (visited.has(n)) continue
      const p = parentOf.get(n)
      if (p) p.children = p.children.filter((c) => c !== n)
      n.depth = 0
      roots.push(n)
      visit([n])
    }
    roots.sort(byStart)
  }
  for (let i = order.length - 1; i >= 0; i--) {
    const n = order[i]
    n.descendants = n.children.reduce((sum, c) => sum + 1 + c.descendants, 0)
  }
  return roots
}

export interface FlatRow {
  node: SpanNode
  depth: number
  collapsed: boolean
}

/** The rows the waterfall shows: depth-first, skipping collapsed subtrees. */
export function flattenTree(roots: SpanNode[], collapsed: ReadonlySet<string>): FlatRow[] {
  const out: FlatRow[] = []
  const stack: SpanNode[] = [...roots].reverse()
  while (stack.length) {
    const n = stack.pop()!
    const isCollapsed = collapsed.has(n.span.span_id)
    out.push({ node: n, depth: n.depth, collapsed: isCollapsed && n.children.length > 0 })
    if (!isCollapsed) for (let i = n.children.length - 1; i >= 0; i--) stack.push(n.children[i])
  }
  return out
}

/** Every node, depth-first. */
export function walk(roots: SpanNode[], fn: (n: SpanNode) => void) {
  const stack = [...roots].reverse()
  while (stack.length) {
    const n = stack.pop()!
    fn(n)
    for (let i = n.children.length - 1; i >= 0; i--) stack.push(n.children[i])
  }
}

export function traceBounds(spans: TraceSpan[]): { start: number; end: number } {
  if (!spans.length) return { start: 0, end: 0 }
  let start = Infinity
  let stop = -Infinity
  for (const s of spans) {
    if (s.start < start) start = s.start
    if (end(s) > stop) stop = end(s)
  }
  return { start, end: stop }
}

/**
 * The critical path: starting at the root that ends last, repeatedly the
 * child that finishes last before the current point in time, then the
 * latest child that finished before that one started, and so on. These
 * spans determine the end-to-end duration; making others faster does not
 * shorten the trace.
 */
export function criticalPath(roots: SpanNode[]): Set<string> {
  const out = new Set<string>()
  if (!roots.length) return out
  const root = roots.reduce((a, b) => (b.end > a.end ? b : a))
  const visit = (n: SpanNode) => {
    out.add(n.span.span_id)
    let t = n.end
    const children = [...n.children].sort((a, b) => b.end - a.end)
    for (const c of children) {
      // Small tolerance: child spans often end a fraction after the parent.
      if (c.end <= t + 0.5) {
        visit(c)
        t = c.span.start
      }
    }
  }
  visit(root)
  return out
}

/** Time in the span not covered by any child span. */
export function selfTime(n: SpanNode): number {
  if (!n.children.length) return n.span.duration_ms
  const s = n.span.start
  const e = n.end
  const intervals = n.children
    .map((c) => [Math.max(s, c.span.start), Math.min(e, c.end)] as const)
    .filter(([a, b]) => b > a)
    .sort((a, b) => a[0] - b[0])
  let covered = 0
  let curStart = -Infinity
  let curEnd = -Infinity
  for (const [a, b] of intervals) {
    if (a > curEnd) {
      if (curEnd > curStart) covered += curEnd - curStart
      curStart = a
      curEnd = b
    } else curEnd = Math.max(curEnd, b)
  }
  if (curEnd > curStart) covered += curEnd - curStart
  return Math.max(0, n.span.duration_ms - covered)
}

/** Category of an op for colouring: "db.query" → "db", "http.client" → "http". */
export function opCategory(op: string | null): string {
  if (!op) return 'default'
  const head = op.split('.')[0]
  if (
    [
      'db',
      'cache',
      'http',
      'browser',
      'resource',
      'ui',
      'queue',
      'function',
      'navigation',
      'pageload',
      'task',
      'rpc',
      'file',
      'serialize',
      'template',
      'middleware',
      'view',
    ].includes(head)
  )
    return head
  return 'default'
}

/** Total self time per op category, largest first. */
export function opBreakdown(roots: SpanNode[]): { category: string; ms: number }[] {
  const totals = new Map<string, number>()
  walk(roots, (n) => {
    const c = opCategory(n.span.op)
    totals.set(c, (totals.get(c) ?? 0) + selfTime(n))
  })
  return [...totals.entries()].map(([category, ms]) => ({ category, ms })).sort((a, b) => b.ms - a.ms)
}

/** Index of each span's row in the flat list, for jumping to a span. */
export function ancestorsOf(spanId: string, spans: Map<string, TraceSpan>): string[] {
  const out: string[] = []
  const seen = new Set<string>()
  let cur = spans.get(spanId)?.parent_span_id
  while (cur && spans.has(cur) && !seen.has(cur)) {
    seen.add(cur)
    out.push(cur)
    cur = spans.get(cur)!.parent_span_id
  }
  return out
}

/** Picks "nice" tick positions for a time axis of `durationMs`. */
export function timeTicks(durationMs: number, count = 6): number[] {
  if (durationMs <= 0) return [0]
  const raw = durationMs / count
  const mag = Math.pow(10, Math.floor(Math.log10(raw)))
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? 10 * mag
  const out: number[] = []
  for (let t = 0; t <= durationMs + 1e-9; t += step) out.push(t)
  return out
}
