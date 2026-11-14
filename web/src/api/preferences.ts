import type { UserPreferences } from './types'

// A copy of the user's preferences in this browser. The server stores them
// with the account (see `profile` in endpoints.ts); this copy holds deploy
// notifications, which the server does not store, and is the fallback for
// servers without preferences.
const KEY = 'rupta-preferences'

export const DEFAULT_PREFERENCES: UserPreferences = {
  timezone: 'browser',
  clock_24h: true,
  notifications: { issue_alerts: true, workflow: 'participating', weekly_report: true, deploys: false },
}

export function readPreferences(): UserPreferences {
  try {
    const raw = localStorage.getItem(KEY)
    return raw ? { ...DEFAULT_PREFERENCES, ...(JSON.parse(raw) as UserPreferences) } : DEFAULT_PREFERENCES
  } catch {
    return DEFAULT_PREFERENCES
  }
}

export function writePreferences(p: UserPreferences) {
  const next = { ...readPreferences(), ...p, notifications: { ...readPreferences().notifications, ...p.notifications } }
  try {
    localStorage.setItem(KEY, JSON.stringify(next))
  } catch {
    /* storage unavailable */
  }
}
