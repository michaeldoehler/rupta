// Flamegraph layout and analysis over the call tree the server builds.
// Pure functions, unit tested; the canvas renderer only draws the result.

import type { CallTreeNode, Flamegraph, ProfileFrame } from '@/api/telemetry-types'

export type FlameOrder = 'time' | 'heavy'

export interface FlameRect {
  /** Offset from the left, in samples. */
  x: number
  /** Width in samples (the node's total weight). */
  w: number
  depth: number
  frame: number
  node: CallTreeNode
  /** Index of the parent rect, -1 for the top level. */
  parent: number
}

export interface FlameLayout {
  rects: FlameRect[]
  total: number
  maxDepth: number
}

/**
 * Lays out the tree: the root (no frame) is not drawn, its children form
 * depth 0. "time" keeps the server's order (callees as first seen, which
 * follows time); "heavy" puts the heaviest callee first (left-heavy).
 */
export function layoutFlamegraph(fg: Flamegraph, order: FlameOrder = 'time'): FlameLayout {
  const rects: FlameRect[] = []
  let maxDepth = -1
  const stack: { node: CallTreeNode; x: number; depth: number; parent: number }[] = []
  const pushChildren = (node: CallTreeNode, x: number, depth: number, parent: number) => {
    const children = order === 'heavy' ? [...node.children].sort((a, b) => b.total_weight - a.total_weight) : node.children
    let cx = x
    const items: typeof stack = []
    for (const c of children) {
      items.push({ node: c, x: cx, depth, parent })
      cx += c.total_weight
    }
    for (let i = items.length - 1; i >= 0; i--) stack.push(items[i])
  }
  pushChildren(fg.root, 0, 0, -1)
  while (stack.length) {
    const { node, x, depth, parent } = stack.pop()!
    if (node.total_weight <= 0) continue
    const idx = rects.length
    rects.push({ x, w: node.total_weight, depth, frame: node.frame_index ?? -1, node, parent })
    if (depth > maxDepth) maxDepth = depth
    pushChildren(node, x, depth + 1, idx)
  }
  const total = fg.root.total_weight || fg.root.children.reduce((s, c) => s + c.total_weight, 0)
  return { rects, total, maxDepth }
}

/** Frame indexes whose name, file or package contains `term` (case-insensitive). */
export function searchFrames(frames: ProfileFrame[], term: string): Set<number> {
  const out = new Set<number>()
  const t = term.trim().toLowerCase()
  if (!t) return out
  frames.forEach((f, i) => {
    if (f.name.toLowerCase().includes(t) || (f.file ?? '').toLowerCase().includes(t) || (f.package ?? '').toLowerCase().includes(t)) out.add(i)
  })
  return out
}

/** Sum of the widths of matching rects that are not inside another match. */
export function matchedWeight(layout: FlameLayout, matches: Set<number>): number {
  if (!matches.size) return 0
  let sum = 0
  const inside = new Uint8Array(layout.rects.length)
  layout.rects.forEach((r, i) => {
    const parentInside = r.parent >= 0 && inside[r.parent] === 1
    if (parentInside) inside[i] = 1
    else if (matches.has(r.frame)) {
      inside[i] = 1
      sum += r.w
    }
  })
  return sum
}

export interface FunctionRow {
  frame: number
  /** Samples in which the function was the innermost frame. */
  self: number
  /** Samples in which the function was on the stack (recursion counted once). */
  total: number
  /** Call sites in the tree. */
  calls: number
}

/** Flat per-function totals: self and total weight, recursion counted once. */
export function functionTable(fg: Flamegraph): FunctionRow[] {
  const rows = new Map<number, FunctionRow>()
  const onStack = new Map<number, number>()
  const visit = (node: CallTreeNode) => {
    const f = node.frame_index
    if (f !== null) {
      let r = rows.get(f)
      if (!r) rows.set(f, (r = { frame: f, self: 0, total: 0, calls: 0 }))
      r.self += node.self_weight
      r.calls += 1
      if (!onStack.get(f)) r.total += node.total_weight
      onStack.set(f, (onStack.get(f) ?? 0) + 1)
    }
    for (const c of node.children) visit(c)
    if (f !== null) onStack.set(f, onStack.get(f)! - 1)
  }
  visit(fg.root)
  return [...rows.values()].sort((a, b) => b.self - a.self || b.total - a.total)
}

/** Callers (parent frames) of a frame with their weights, heaviest first. */
export function callersOf(layout: FlameLayout, frame: number): { frame: number; weight: number }[] {
  const m = new Map<number, number>()
  for (const r of layout.rects) {
    if (r.frame !== frame) continue
    const p = r.parent >= 0 ? layout.rects[r.parent].frame : -1
    m.set(p, (m.get(p) ?? 0) + r.w)
  }
  return [...m.entries()].map(([f, weight]) => ({ frame: f, weight })).sort((a, b) => b.weight - a.weight)
}

export type ColorMode = 'application' | 'package'

/** FNV-1a, for stable per-package hues. */
function hash(s: string): number {
  let h = 2166136261
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 16777619)
  }
  return h >>> 0
}

/**
 * Fill colour of a frame. Application code is palm green (the secondary accent),
 * library code cool and muted; by package, each package gets its own hue.
 * Lightness adapts to the theme so text stays readable.
 */
export function frameColor(frame: ProfileFrame | undefined, mode: ColorMode, dark: boolean, name = ''): string {
  const key = frame?.package ?? frame?.file ?? name
  const h = hash(`${key}:${frame?.name ?? name}`)
  const l = dark ? 0.46 + (h % 7) * 0.015 : 0.8 + (h % 7) * 0.012
  if (mode === 'package') {
    const hue = hash(frame?.package ?? frame?.file ?? 'unknown') % 360
    return `oklch(${l.toFixed(3)} ${dark ? 0.09 : 0.08} ${hue})`
  }
  if (frame?.is_application) {
    // application code in the palm-leaf range, library code cool and quiet
    const hue = 112 + (h % 24)
    return `oklch(${l.toFixed(3)} ${dark ? 0.13 : 0.11} ${hue})`
  }
  const hue = 220 + (h % 40)
  return `oklch(${(l + (dark ? -0.04 : 0.03)).toFixed(3)} ${dark ? 0.035 : 0.03} ${hue})`
}

/** Index of the rect under (weightX, depth), or -1. */
export function hitTest(layout: FlameLayout, x: number, depth: number): number {
  for (let i = 0; i < layout.rects.length; i++) {
    const r = layout.rects[i]
    if (r.depth === depth && x >= r.x && x < r.x + r.w) return i
  }
  return -1
}

/** Rect indexes grouped by depth, for fast drawing and hit tests. */
export function rowsByDepth(layout: FlameLayout): number[][] {
  const rows: number[][] = Array.from({ length: layout.maxDepth + 1 }, () => [])
  layout.rects.forEach((r, i) => rows[r.depth].push(i))
  for (const row of rows) row.sort((a, b) => layout.rects[a].x - layout.rects[b].x)
  return rows
}

/** Binary search in one depth row. */
export function hitTestRow(layout: FlameLayout, row: number[], x: number): number {
  let lo = 0
  let hi = row.length - 1
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    const r = layout.rects[row[mid]]
    if (x < r.x) hi = mid - 1
    else if (x >= r.x + r.w) lo = mid + 1
    else return row[mid]
  }
  return -1
}

/** Path of frame names from the top to the rect, for the details panel. */
export function stackOf(layout: FlameLayout, index: number): number[] {
  const out: number[] = []
  let i = index
  while (i >= 0) {
    out.unshift(layout.rects[i].frame)
    i = layout.rects[i].parent
  }
  return out
}
