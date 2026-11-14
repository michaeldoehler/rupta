import { describe, expect, it } from 'vitest'
import type { CallTreeNode, Flamegraph } from '@/api/telemetry-types'
import { normFlamegraph } from '@/api/telemetry'
import { flamegraphOf, sampledProfile } from '@/mocks/fixtures/telemetry'
import { callersOf, frameColor, functionTable, hitTestRow, layoutFlamegraph, matchedWeight, rowsByDepth, searchFrames, stackOf } from './flamegraph-layout'

const n = (frame: number | null, self: number, children: CallTreeNode[] = []): CallTreeNode => ({
  frame_index: frame,
  self_weight: self,
  total_weight: self + children.reduce((s, c) => s + c.total_weight, 0),
  children,
})

// main → [handle → [query(6), render(1)], idle(3)]; recursion: walk → walk
const fg: Flamegraph = {
  frames: [
    { name: 'main', file: 'app.py', line: 1, package: null, is_application: true },
    { name: 'handle', file: 'app.py', line: 10, package: null, is_application: true },
    { name: 'query', file: 'psycopg/cursor.py', line: 700, package: 'psycopg', is_application: false },
    { name: 'render', file: 'jinja2/environment.py', line: 1200, package: 'jinja2', is_application: false },
    { name: 'idle', file: 'selectors.py', line: 400, package: 'python', is_application: false },
    { name: 'walk', file: 'tree.py', line: 5, package: null, is_application: true },
  ],
  root: n(null, 0, [n(0, 0, [n(1, 1, [n(2, 6), n(3, 1)]), n(4, 3), n(5, 1, [n(5, 2)])])]),
  samples: 14,
  profiles: 1,
}

describe('layoutFlamegraph', () => {
  it('places children side by side in time order under their parent', () => {
    const l = layoutFlamegraph(fg, 'time')
    expect(l.total).toBe(14)
    expect(l.maxDepth).toBe(2)
    const byFrame = (f: number) => l.rects.filter((r) => r.frame === f)
    expect(byFrame(0)[0]).toMatchObject({ x: 0, w: 14, depth: 0 })
    expect(byFrame(1)[0]).toMatchObject({ x: 0, w: 8, depth: 1 })
    expect(byFrame(2)[0]).toMatchObject({ x: 0, w: 6, depth: 2 })
    expect(byFrame(3)[0]).toMatchObject({ x: 6, w: 1, depth: 2 })
    expect(byFrame(4)[0]).toMatchObject({ x: 8, w: 3, depth: 1 })
  })

  it('orders the heaviest callee first in left-heavy mode', () => {
    const heavy: Flamegraph = { ...fg, root: n(null, 0, [n(0, 0, [n(4, 3), n(1, 1, [n(3, 1), n(2, 6)])])]) }
    const l = layoutFlamegraph(heavy, 'heavy')
    const rect = (f: number) => l.rects.find((r) => r.frame === f)!
    expect(rect(1).x).toBe(0)
    expect(rect(4).x).toBe(8)
    expect(rect(2).x).toBe(0)
    expect(rect(3).x).toBe(6)
  })

  it('hit-tests by depth row and returns the stack of a rect', () => {
    const l = layoutFlamegraph(fg)
    const rows = rowsByDepth(l)
    const i = hitTestRow(l, rows[2], 6.5)
    expect(l.rects[i].frame).toBe(3)
    expect(stackOf(l, i)).toEqual([0, 1, 3])
    expect(l.rects[hitTestRow(l, rows[2], 12.5)].frame).toBe(5)
    expect(hitTestRow(l, rows[2], 9.5)).toBe(-1)
  })

  it('searches frames and sums matched weight without double counting', () => {
    const l = layoutFlamegraph(fg)
    expect([...searchFrames(fg.frames, 'PSYCOPG')]).toEqual([2])
    expect(matchedWeight(l, searchFrames(fg.frames, 'walk'))).toBe(3)
    expect(matchedWeight(l, new Set([1, 2]))).toBe(8)
    expect(searchFrames(fg.frames, '  ').size).toBe(0)
  })

  it('aggregates functions with recursion counted once', () => {
    const rows = functionTable(fg)
    const walk = rows.find((r) => r.frame === 5)!
    expect(walk).toMatchObject({ self: 3, total: 3, calls: 2 })
    expect(rows[0].frame).toBe(2) // highest self time first
    expect(callersOf(layoutFlamegraph(fg), 5).map((c) => c.frame)).toEqual([0, 5])
  })

  it('colours application and library frames differently', () => {
    expect(frameColor(fg.frames[0], 'application', false)).not.toBe(frameColor(fg.frames[2], 'application', false))
    expect(frameColor(fg.frames[2], 'package', true)).toMatch(/^oklch\(/)
  })
})

describe('mock sampled profiles', () => {
  it('merge into a consistent call tree', () => {
    const p = { profile_id: 'p1', project_id: '3', project_slug: 'api', kind: 'transaction' as const, transaction_name: 'GET /api/products', trace_id: 't', platform: 'python', release: null, environment: 'production', start: 0, duration_ms: 900 }
    const raw = sampledProfile(p)
    const tree = normFlamegraph(flamegraphOf([raw]))
    expect(tree.samples).toBe(raw.profile.samples.length)
    expect(tree.root.total_weight).toBe(tree.samples)
    const check = (node: CallTreeNode): void => {
      expect(node.total_weight).toBe(node.self_weight + node.children.reduce((s, c) => s + c.total_weight, 0))
      node.children.forEach(check)
    }
    check(tree.root)
    expect(tree.frames.some((f) => f.is_application)).toBe(true)
  })
})
