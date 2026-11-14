// Search keys the issue stream understands, with descriptions and fixed
// value lists. Keys with `dynamic: true` take values from the tag values API.

export interface SearchKey {
  key: string
  description: string
  values?: { value: string; description?: string }[]
  /** Values come from the tag values endpoint (tag key in `tag`). */
  dynamic?: boolean
  tag?: string
  /** Shown as an example value. */
  example?: string
  section: 'status' | 'issue' | 'event' | 'tag'
}

export const SEARCH_KEYS: SearchKey[] = [
  {
    key: 'is',
    section: 'status',
    description: 'Issue status or state',
    values: [
      { value: 'unresolved', description: 'Open issues' },
      { value: 'resolved', description: 'Marked as fixed' },
      { value: 'ignored', description: 'Archived issues' },
      { value: 'new', description: 'First seen in the last 7 days, not reviewed' },
      { value: 'ongoing', description: 'Seen before, still happening' },
      { value: 'regressed', description: 'Came back after being resolved' },
      { value: 'escalating', description: 'Event volume rising sharply' },
      { value: 'assigned', description: 'Has an assignee' },
      { value: 'unassigned', description: 'Has no assignee' },
      { value: 'bookmarked', description: 'Bookmarked by you' },
    ],
  },
  {
    key: 'level',
    section: 'issue',
    description: 'Severity of the event',
    values: [{ value: 'fatal' }, { value: 'error' }, { value: 'warning' }, { value: 'info' }, { value: 'debug' }],
  },
  {
    key: 'assigned',
    section: 'issue',
    description: 'Assigned to a user or team',
    values: [
      { value: 'me', description: 'Assigned to you' },
      { value: 'my_teams', description: 'Assigned to one of your teams' },
      { value: 'none', description: 'Nobody' },
    ],
    example: 'me, #backend, jane@example.com',
  },
  {
    key: 'priority',
    section: 'issue',
    description: 'Issue priority',
    values: [{ value: 'high' }, { value: 'medium' }, { value: 'low' }],
  },
  { key: 'release', section: 'event', description: 'Release version', dynamic: true, tag: 'release', example: 'latest, 1.4.2' },
  { key: 'first_release', section: 'issue', description: 'Release the issue first appeared in', dynamic: true, tag: 'release' },
  { key: 'environment', section: 'event', description: 'Deployment environment', dynamic: true, tag: 'environment' },
  { key: 'browser', section: 'event', description: 'Browser name', dynamic: true, tag: 'browser' },
  { key: 'os', section: 'event', description: 'Operating system', dynamic: true, tag: 'os' },
  { key: 'device', section: 'event', description: 'Device model', dynamic: true, tag: 'device' },
  { key: 'runtime', section: 'event', description: 'Runtime name and version', dynamic: true, tag: 'runtime' },
  { key: 'url', section: 'event', description: 'Request URL', dynamic: true, tag: 'url' },
  { key: 'transaction', section: 'event', description: 'Route or transaction name', dynamic: true, tag: 'transaction' },
  { key: 'server_name', section: 'event', description: 'Host that sent the event', dynamic: true, tag: 'server_name' },
  { key: 'user.email', section: 'event', description: 'Email of the affected user', dynamic: true, tag: 'user.email' },
  { key: 'user.id', section: 'event', description: 'Id of the affected user', dynamic: true, tag: 'user.id' },
  { key: 'error.type', section: 'event', description: 'Exception type', dynamic: true, tag: 'error.type', example: 'TypeError' },
  { key: 'error.value', section: 'event', description: 'Exception message' },
  {
    key: 'error.handled',
    section: 'event',
    description: 'Whether the error was caught',
    values: [{ value: 'yes' }, { value: 'no' }],
  },
  { key: 'platform', section: 'issue', description: 'SDK platform', dynamic: true, tag: 'platform' },
  { key: 'sdk.name', section: 'event', description: 'SDK name', dynamic: true, tag: 'sdk.name' },
  {
    key: 'age',
    section: 'issue',
    description: 'Time since first seen',
    values: [{ value: '-1h' }, { value: '-24h' }, { value: '-7d' }, { value: '-30d' }],
    example: '-24h',
  },
  { key: 'last_seen', section: 'issue', description: 'Time since last seen', values: [{ value: '-1h' }, { value: '-24h' }, { value: '-7d' }], example: '-24h' },
  { key: 'times_seen', section: 'issue', description: 'Number of events', values: [{ value: '>10' }, { value: '>100' }, { value: '>1000' }], example: '>100' },
  { key: 'users', section: 'issue', description: 'Number of affected users', values: [{ value: '>1' }, { value: '>10' }, { value: '>100' }], example: '>10' },
]

export const SEARCH_KEY_MAP = new Map(SEARCH_KEYS.map((k) => [k.key, k]))
