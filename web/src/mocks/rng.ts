// Deterministic pseudo-random numbers, so fixture data is stable across
// reloads, tests and screenshots.

export function mulberry32(seed: number) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

export function hashString(s: string): number {
  let h = 2166136261
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 16777619)
  }
  return h >>> 0
}

export class Rng {
  private next: () => number
  constructor(seed: number | string) {
    this.next = mulberry32(typeof seed === 'string' ? hashString(seed) : seed)
  }
  float() {
    return this.next()
  }
  int(min: number, max: number) {
    return Math.floor(this.next() * (max - min + 1)) + min
  }
  pick<T>(arr: readonly T[]): T {
    return arr[Math.floor(this.next() * arr.length)]
  }
  chance(p: number) {
    return this.next() < p
  }
  /** Weighted pick: [[value, weight], ...]. */
  weighted<T>(items: readonly (readonly [T, number])[]): T {
    const total = items.reduce((s, [, w]) => s + w, 0)
    let r = this.next() * total
    for (const [v, w] of items) {
      r -= w
      if (r <= 0) return v
    }
    return items[items.length - 1][0]
  }
  /** Heavy-tailed positive integer (most small, a few huge). */
  powerLaw(min: number, max: number, alpha = 2.2) {
    const u = this.next()
    const v = Math.pow((Math.pow(max, 1 - alpha) - Math.pow(min, 1 - alpha)) * u + Math.pow(min, 1 - alpha), 1 / (1 - alpha))
    return Math.round(v)
  }
  hex(len: number) {
    let s = ''
    for (let i = 0; i < len; i++) s += Math.floor(this.next() * 16).toString(16)
    return s
  }
  uuid() {
    return this.hex(32)
  }
  shuffle<T>(arr: T[]): T[] {
    const a = [...arr]
    for (let i = a.length - 1; i > 0; i--) {
      const j = Math.floor(this.next() * (i + 1))
      ;[a[i], a[j]] = [a[j], a[i]]
    }
    return a
  }
}
