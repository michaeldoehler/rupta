// Which mark each service gets and how its tile is drawn. See NOTICE.md for
// where the marks come from.

import { Mail, Plug, Webhook, type LucideIcon } from 'lucide-react'
import { github, gitlab, jira, linear, opsgenie, pagerduty, type Mark } from './marks'

/** Services the UI refers to: issue trackers and notification channels. */
export type BrandKind = 'jira_cloud' | 'jira_server' | 'linear' | 'github' | 'gitlab' | 'pagerduty' | 'opsgenie' | 'slack' | 'teams' | 'webhook' | 'email'

/**
 * How a service is drawn:
 * - `light`: the brand-coloured glyph on a light tile,
 * - `solid`: a white glyph on a tile in the brand colour,
 * - `monogram`: a letter on a tile in the brand colour, for brands whose
 *   terms do not allow their logo here (see NOTICE.md),
 * - `neutral`: a generic icon on a neutral tile.
 */
export type Brand = { label: string } & (
  { tile: 'light' | 'solid'; mark: Mark } | { tile: 'monogram'; letter: string; color: string } | { tile: 'neutral'; icon: LucideIcon }
)

export const BRANDS: Record<BrandKind, Brand> = {
  jira_cloud: { label: 'Jira Cloud', tile: 'light', mark: jira },
  jira_server: { label: 'Jira Data Center', tile: 'light', mark: jira },
  linear: { label: 'Linear', tile: 'solid', mark: linear },
  github: { label: 'GitHub', tile: 'light', mark: github },
  gitlab: { label: 'GitLab', tile: 'light', mark: gitlab },
  pagerduty: { label: 'PagerDuty', tile: 'solid', mark: pagerduty },
  opsgenie: { label: 'Opsgenie', tile: 'light', mark: opsgenie },
  // Slack's brand terms and Microsoft's trademark guidelines do not permit
  // their logos here, so both get a monogram in the brand colour.
  slack: { label: 'Slack', tile: 'monogram', letter: 'S', color: '#4A154B' },
  teams: { label: 'Microsoft Teams', tile: 'monogram', letter: 'T', color: '#5059C9' },
  webhook: { label: 'Webhook', tile: 'neutral', icon: Webhook },
  email: { label: 'Email', tile: 'neutral', icon: Mail },
}

/** The brand of a kind, or a generic one named after the kind for anything unknown. */
export function brandOf(kind: string | null | undefined): Brand {
  if (kind && Object.hasOwn(BRANDS, kind)) return BRANDS[kind as BrandKind]
  return { label: kind ? kind.replace(/_/g, ' ') : 'Integration', tile: 'neutral', icon: Plug }
}

/** Whether an SSO provider's configured name is Zitadel (case-insensitive). */
export function isZitadel(name: string | null | undefined): boolean {
  return (name ?? '').trim().toLowerCase() === 'zitadel'
}
