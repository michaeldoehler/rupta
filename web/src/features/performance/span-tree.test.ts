import { describe, expect, it } from 'vitest'
import type { TraceSpan } from '@/api/telemetry-types'
import { normTrace } from '@/api/telemetry'
import { buildSpanTree, criticalPath, flattenTree, opBreakdown, selfTime, timeTicks, traceBounds } from './span-tree'

function span(id: string, parent: string | null, start: number, duration: number, op = 'function'): TraceSpan {
  return {
    span_id: id,
    parent_span_id: parent,
    trace_id: 't',
    project_id: '1',
    segment_id: null,
    is_segment: parent === null,
    transaction: null,
    op,
    description: id,
    status: 'ok',
    start,
    duration_ms: duration,
    data: {},
    tags: {},
    profile_id: null,
  }
}

describe('buildSpanTree', () => {
  it('nests spans under their parents and orders siblings by start', () => {
    const roots = buildSpanTree([span('c', 'a', 30, 10), span('a', null, 0, 100), span('b', 'a', 10, 10), span('d', 'b', 12, 2)])
    expect(roots).toHaveLength(1)
    const [a] = roots
    expect(a.children.map((c) => c.span.span_id)).toEqual(['b', 'c'])
    expect(a.children[0].children[0].span.span_id).toBe('d')
    expect(a.children[0].children[0].depth).toBe(2)
    expect(a.descendants).toBe(3)
  })

  it('makes orphans roots and survives parent cycles and duplicates', () => {
    const roots = buildSpanTree([span('a', null, 0, 10), span('x', 'missing', 5, 1), span('p', 'q', 1, 1), span('q', 'p', 2, 1), span('a', null, 3, 3)])
    const ids = roots.map((r) => r.span.span_id)
    expect(ids).toContain('a')
    expect(ids).toContain('x')
    // p and q point at each other: one of them must become a root, nothing is lost
    let count = 0
    flattenTree(roots, new Set()).forEach(() => (count += 1))
    expect(count).toBe(4)
  })

  it('flattens depth-first and skips collapsed subtrees', () => {
    const roots = buildSpanTree([span('a', null, 0, 100), span('b', 'a', 10, 10), span('d', 'b', 12, 2), span('c', 'a', 30, 10)])
    expect(flattenTree(roots, new Set()).map((r) => r.node.span.span_id)).toEqual(['a', 'b', 'd', 'c'])
    const collapsed = flattenTree(roots, new Set(['b']))
    expect(collapsed.map((r) => r.node.span.span_id)).toEqual(['a', 'b', 'c'])
    expect(collapsed[1].collapsed).toBe(true)
  })

  it('handles deep traces without recursion limits', () => {
    const spans = Array.from({ length: 20_000 }, (_, i) => span(`s${i}`, i ? `s${i - 1}` : null, i, 20_000 - i))
    const roots = buildSpanTree(spans)
    expect(roots[0].descendants).toBe(19_999)
    expect(flattenTree(roots, new Set())).toHaveLength(20_000)
  })
})

describe('critical path and self time', () => {
  it('follows the latest-ending child chain', () => {
    // a: 0..100; b: 0..40, c: 45..95 (critical), c1 inside c ends last; e overlaps b
    const roots = buildSpanTree([
      span('a', null, 0, 100),
      span('b', 'a', 0, 40),
      span('e', 'a', 5, 20),
      span('c', 'a', 45, 50),
      span('c1', 'c', 50, 44),
    ])
    const path = criticalPath(roots)
    expect([...path].sort()).toEqual(['a', 'b', 'c', 'c1'])
    expect(path.has('e')).toBe(false)
  })

  it('computes self time from the union of child intervals', () => {
    const roots = buildSpanTree([span('a', null, 0, 100), span('b', 'a', 10, 30), span('c', 'a', 20, 30), span('d', 'a', 90, 20)])
    // children cover 10..50 and 90..100 → 50 ms covered
    expect(selfTime(roots[0])).toBe(50)
    const breakdown = opBreakdown(roots)
    expect(breakdown.reduce((s, b) => s + b.ms, 0)).toBe(50 + 30 + 30 + 20)
  })

  it('finds bounds and nice ticks', () => {
    expect(traceBounds([span('a', null, 100, 50), span('b', 'a', 90, 100)])).toEqual({ start: 90, end: 190 })
    expect(timeTicks(1000, 5)).toEqual([0, 200, 400, 600, 800, 1000])
    expect(timeTicks(0)).toEqual([0])
  })
})

describe('normTrace', () => {
  it('reads nested spans, epoch-second timestamps and transactions', () => {
    const t = normTrace(
      {
        transactions: [{ span_id: 'root', name: 'GET /', op: 'http.server', start_timestamp: 1700000000.5, duration_ms: 120 }],
        spans: [{ span_id: 'a', parent_span_id: 'root', start_timestamp: 1700000000.6, timestamp: 1700000000.7, children: [{ span_id: 'b', parent_span_id: 'a', start: '2023-11-14T22:13:20.650Z', duration: 5 }] }],
        errors: [{ event_id: 'e1', issue_id: 7, span_id: 'b', timestamp: '2023-11-14T22:13:20.652Z', title: 'Boom' }],
        logs_count: 3,
      },
      'abc',
    )
    expect(t.spans.map((s) => s.span_id).sort()).toEqual(['a', 'b', 'root'])
    const root = t.spans.find((s) => s.span_id === 'root')!
    expect(root.is_segment).toBe(true)
    expect(root.transaction).toBe('GET /')
    expect(root.start).toBe(1700000000500)
    expect(t.spans.find((s) => s.span_id === 'b')!.duration_ms).toBe(5)
    expect(t.errors[0].issue_id).toBe('7')
    expect(t.logs_count).toBe(3)
  })
})
