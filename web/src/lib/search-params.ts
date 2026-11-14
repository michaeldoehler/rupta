import { z } from 'zod'

export const PERIODS = ['24h', '7d', '14d', '30d', '90d'] as const
export type Period = (typeof PERIODS)[number]
export const DEFAULT_PERIOD: Period = '14d'

export const PERIOD_LABEL: Record<Period, string> = {
  '24h': 'Last 24 hours',
  '7d': 'Last 7 days',
  '14d': 'Last 14 days',
  '30d': 'Last 30 days',
  '90d': 'Last 90 days',
}

/** Global selectors kept in the URL across pages: project ids, environments, period. */
export const globalSearchSchema = z.object({
  project: z.string().optional().catch(undefined),
  environment: z.string().optional().catch(undefined),
  period: z.enum(PERIODS).optional().catch(undefined),
})

export type GlobalSearch = z.infer<typeof globalSearchSchema>

export function splitList(v: string | undefined): string[] {
  return v
    ? v
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean)
    : []
}

export function joinList(v: string[]): string | undefined {
  return v.length ? v.join(',') : undefined
}
