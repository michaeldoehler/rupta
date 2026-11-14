import type { AlertAction, AlertCondition, AlertFilter, Channel, Team } from '@/api/types'

export const INTERVALS = [
  ['1m', '1 minute'],
  ['5m', '5 minutes'],
  ['15m', '15 minutes'],
  ['1h', 'one hour'],
  ['1d', 'one day'],
  ['1w', 'one week'],
] as const

export const FREQUENCIES: [number, string][] = [
  [5, '5 minutes'],
  [10, '10 minutes'],
  [30, '30 minutes'],
  [60, '1 hour'],
  [180, '3 hours'],
  [720, '12 hours'],
  [1440, '24 hours'],
  [10080, '1 week'],
]

const intervalLabel = (i: string) => INTERVALS.find(([k]) => k === i)?.[1] ?? i

export function describeCondition(c: AlertCondition): string {
  switch (c.type) {
    case 'first_seen':
      return 'A new issue is created'
    case 'regression':
      return 'A resolved issue comes back'
    case 'reappeared':
      return 'An ignored issue reappears'
    case 'event_frequency':
      return `An issue is seen more than ${c.count.toLocaleString()} times in ${intervalLabel(c.interval)}`
    case 'user_frequency':
      return `An issue affects more than ${c.count.toLocaleString()} users in ${intervalLabel(c.interval)}`
  }
}

export function describeFilter(f: AlertFilter): string {
  switch (f.type) {
    case 'level':
      return `level ${f.match === 'eq' ? 'is' : f.match === 'gte' ? 'is at least' : 'is at most'} ${f.level}`
    case 'tag':
      return f.match === 'is_set'
        ? `tag ${f.key} is set`
        : `tag ${f.key} ${{ eq: 'equals', ne: 'does not equal', contains: 'contains', starts_with: 'starts with', is_set: '' }[f.match]} “${f.value ?? ''}”`
    case 'environment':
      return `environment is ${f.value}`
    case 'issue_age':
      return `issue is older than ${f.older_than_minutes} minutes`
  }
}

export function describeAction(a: AlertAction, channels: Channel[], teams: Team[]): string {
  if (a.type === 'channel') {
    const c = channels.find((x) => x.id === a.channel_id)
    return c ? `Notify ${c.name} (${c.kind})` : 'Notify a deleted channel'
  }
  if (a.target === 'owners') return 'Email the project team'
  if (a.target === 'members') return 'Email all members'
  if (a.target === 'team') return `Email #${teams.find((t) => t.id === a.target_id)?.slug ?? 'team'}`
  return 'Email a member'
}
