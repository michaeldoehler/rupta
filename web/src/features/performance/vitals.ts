import type { VitalName } from '@/api/telemetry-types'

// Thresholds of the Core Web Vitals (and FCP/TTFB) at the 75th percentile:
// up to `good` is good, up to `poor` needs improvement, above is poor.
export const VITALS: { key: VitalName; label: string; name: string; good: number; poor: number; unit: 'ms' | 'score' }[] = [
  { key: 'lcp', label: 'LCP', name: 'Largest contentful paint', good: 2500, poor: 4000, unit: 'ms' },
  { key: 'inp', label: 'INP', name: 'Interaction to next paint', good: 200, poor: 500, unit: 'ms' },
  { key: 'cls', label: 'CLS', name: 'Cumulative layout shift', good: 0.1, poor: 0.25, unit: 'score' },
  { key: 'fcp', label: 'FCP', name: 'First contentful paint', good: 1800, poor: 3000, unit: 'ms' },
  { key: 'ttfb', label: 'TTFB', name: 'Time to first byte', good: 800, poor: 1800, unit: 'ms' },
]

export type VitalRating = 'good' | 'needs-improvement' | 'poor'

export function rateVital(key: VitalName, value: number): VitalRating {
  const v = VITALS.find((x) => x.key === key)!
  return value <= v.good ? 'good' : value <= v.poor ? 'needs-improvement' : 'poor'
}

export const RATING_LABEL: Record<VitalRating, string> = { good: 'Good', 'needs-improvement': 'Needs improvement', poor: 'Poor' }
