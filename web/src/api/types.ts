// Types of the REST API v1 (see the design brief). 64-bit ids are strings,
// timestamps ISO-8601 UTC strings.

export type Id = string
export type Iso = string

export interface Page<T> {
  data: T[]
  next_cursor: string | null
}

export type Role = 'owner' | 'admin' | 'member' | 'viewer'
export type Level = 'fatal' | 'error' | 'warning' | 'info' | 'debug'
export type IssueStatus = 'unresolved' | 'resolved' | 'ignored'
export type IssueSubstatus =
  'new' | 'ongoing' | 'regressed' | 'escalating' | 'archived_until_escalating' | 'archived_until_condition_met' | 'archived_forever' | null
export type Priority = 'high' | 'medium' | 'low'

// --- auth -----------------------------------------------------------------

export interface AuthConfig {
  local_login: boolean
  sso: { id: string; name: string }[]
  setup_required: boolean
}

export interface User {
  id: Id
  email: string
  name: string
  avatar_url: string | null
  is_superuser: boolean
}

export interface OrgRef {
  id: Id
  slug: string
  name: string
  role: Role
}

export interface Session {
  user: User
  csrf_token: string | null
  orgs: OrgRef[]
}

export interface UserPreferences {
  theme?: 'light' | 'dark' | 'system'
  timezone?: string
  clock_24h?: boolean
  notifications?: {
    issue_alerts?: boolean
    workflow?: 'all' | 'participating' | 'none'
    weekly_report?: boolean
    deploys?: boolean
  }
}

export interface UserProfile extends User {
  preferences: UserPreferences
  created_at: Iso
  last_login_at: Iso | null
  has_password: boolean
  identities: { provider: string; email: string | null }[]
}

export interface Invitation {
  org: { slug: string; name: string }
  email: string
  role: Role
  invited_by: string | null
  expires_at: Iso
  user_exists: boolean
}

// --- orgs -------------------------------------------------------------------

export interface Org {
  id: Id
  slug: string
  name: string
  created_at: Iso
  settings: Record<string, unknown>
}

export interface Member {
  user: User
  role: Role
  created_at: Iso
  teams: string[]
  sso: boolean
}

export interface PendingInvite {
  id: Id
  email: string
  role: Role
  created_at: Iso
  expires_at: Iso
  /** Link to share when no email could be sent. */
  accept_url?: string | null
  email_sent?: boolean | null
}

export interface Team {
  id: Id
  slug: string
  name: string
  created_at: Iso
  members: User[]
  projects: { id: Id; slug: string }[]
}

export interface AuditEntry {
  id: Id
  actor: { type: 'user' | 'token' | 'system'; id: Id | null; name: string }
  action: string
  target_type: string | null
  target_id: string | null
  data: Record<string, unknown>
  ip: string | null
  created_at: Iso
}

export interface SsoConfig {
  enabled: boolean
  provider_name: string
  issuer: string
  client_id: string
  /** Never returned; send to change. */
  client_secret?: string
  has_client_secret: boolean
  scopes: string
  role_claim: string | null
  role_mapping: Record<string, Role>
  default_role: SsoDefaultRole
  jit_provisioning: boolean
  enforce: boolean
  redirect_uri: string
  provider_id?: string
  /** Owners may still use their password while SSO is enforced. */
  owner_local_login?: boolean
}

/** Role for users whose claims map to no role; `none` refuses their sign-in. */
export type SsoDefaultRole = Exclude<Role, 'owner'> | 'none'

/** The instance-wide provider from the deployment's RUPTA_OIDC_* variables (read-only). */
export interface InstanceSso {
  provider_id: string
  provider_name: string
  issuer: string
  client_id: string
  has_client_secret: boolean
  scopes: string
  role_claim: string | null
  role_mapping: Record<string, Role>
  default_role: SsoDefaultRole | 'owner'
  jit_provisioning: boolean
  allowed_audiences: string[]
  redirect_uri: string
  /** Organization that users of the instance provider join. */
  default_org: string
}

export interface InstanceSsoInfo {
  /** Email and password login is enabled for the instance. */
  local_login: boolean
  provider: InstanceSso | null
}

export interface SsoTestResult {
  ok: boolean
  message: string
  issuer?: string
  authorization_endpoint?: string
  problems?: string[]
}

export type TokenScope = string

export interface ApiToken {
  id: Id
  name: string
  token_hint: string
  scopes: TokenScope[]
  created_at: Iso
  last_used_at: Iso | null
  expires_at: Iso | null
  /** Present only in the response that created the token. */
  token?: string
}

export type ChannelKind = 'email' | 'slack' | 'webhook' | 'teams' | 'pagerduty' | 'opsgenie'

export interface Channel {
  id: Id
  kind: ChannelKind
  name: string
  config: Record<string, unknown>
  created_at: Iso
}

// --- projects ---------------------------------------------------------------

export interface ProjectRef {
  id: Id
  slug: string
  name: string
  platform: string | null
}

export interface InboundFilters {
  browser_extensions: boolean
  localhost: boolean
  legacy_browsers: boolean
  web_crawlers: boolean
  error_messages: string[]
  releases: string[]
}

export interface Scrubbing {
  enabled: boolean
  extra_fields: string[]
  safe_fields: string[]
  scrub_ip_addresses: boolean
}

export interface ProjectSettings {
  allowed_origins: string[]
  rate_limit_per_min: number | null
  retention_days: number | null
  filters: InboundFilters
  scrubbing: Scrubbing
  in_app_include: string[]
  in_app_exclude: string[]
  retention?: Partial<{ logs: number; spans: number; metrics: number; profiles: number; replays: number }>
  sampling?: Partial<{ transactions: number; spans: number; logs: number }>
  data_types?: Partial<{ traces: boolean; logs: boolean; metrics: boolean; profiles: boolean; replays: boolean }>
  [key: string]: unknown
}

export interface Project extends ProjectRef {
  team: { id: Id; slug: string; name: string } | null
  created_at: Iso
  settings: ProjectSettings
  /** Only in the list: events received over the last 24h, hourly. */
  stats?: [number, number][]
}

export interface ProjectKey {
  id: Id
  label: string
  public_key: string
  secret_key: string
  is_active: boolean
  rate_limit_per_min: number | null
  created_at: Iso
  dsn: string
}

export interface ArtifactBundle {
  id: Id
  checksum: string
  release: string | null
  dist: string | null
  file_count: number
  size: number
  created_at: Iso
  download_url?: string | null
}

// --- issues -----------------------------------------------------------------

export interface Assignee {
  type: 'user' | 'team'
  id: Id
  name: string
  email?: string
}

export interface IssueMetadata {
  type?: string
  value?: string
  filename?: string
  function?: string
}

export interface Issue {
  id: Id
  short_id: string
  title: string
  culprit: string | null
  level: Level
  status: IssueStatus
  substatus: IssueSubstatus
  type: string
  platform: string | null
  project: ProjectRef
  first_seen: Iso
  last_seen: Iso
  count: number
  user_count: number
  assignee: Assignee | null
  metadata: IssueMetadata
  stats: [number, number][]
  priority: Priority
  is_bookmarked?: boolean
  has_seen?: boolean
}

export interface ReleaseRef {
  version: string
  date: Iso | null
}

export interface IssueDetail extends Omit<Issue, 'stats'> {
  first_release: ReleaseRef | null
  last_release: ReleaseRef | null
  resolved_in_release: string | null
  ignore_until: Iso | null
  ignore_count: number | null
  permalink?: string
  /** Counts over the current period, for the header. */
  counts?: { '24h': number; '30d': number }
}

export interface IssueUpdate {
  status?: IssueStatus
  substatus?: IssueSubstatus
  assignee?: { type: 'user' | 'team'; id: Id } | null
  ignore_duration?: number
  ignore_count?: number
  ignore_user_count?: number
  /** Minutes over which `ignore_user_count` users are counted (default: since ignoring). */
  ignore_user_window?: number
  resolved_in_release?: string
  resolved_in_next_release?: boolean
  is_bookmarked?: boolean
  has_seen?: boolean
  priority?: Priority
}

export interface IssueQueryParams {
  project?: string[]
  query?: string
  sort?: IssueSort
  period?: StatsPeriod
  environment?: string[]
  cursor?: string
  limit?: number
}

export type IssueSort = 'last_seen' | 'first_seen' | 'freq' | 'users' | 'priority'
export type StatsPeriod = '1h' | '24h' | '7d' | '14d' | '30d' | '90d'

export interface TagValue {
  value: string
  name?: string
  count: number
  first_seen?: Iso
  last_seen?: Iso
}

export interface TagDistribution {
  key: string
  name: string
  total_values: number
  unique_values: number
  top_values: TagValue[]
}

export interface Activity {
  id: Id
  kind:
    | 'comment'
    | 'set_resolved'
    | 'set_resolved_in_release'
    | 'set_unresolved'
    | 'set_ignored'
    | 'set_regression'
    | 'set_escalating'
    | 'first_seen'
    | 'assigned'
    | 'unassigned'
    | 'set_priority'
    | string
  user: { id: Id; name: string; email: string } | null
  data: Record<string, unknown> & { text?: string }
  created_at: Iso
}

export interface Feedback {
  id: Id
  project: ProjectRef
  event_id: string | null
  issue: { id: Id; short_id: string; title: string } | null
  name: string | null
  email: string | null
  message: string
  url: string | null
  created_at: Iso
}

export interface Attachment {
  id: Id
  event_id: string
  name: string
  content_type: string | null
  size: number
  created_at: Iso
  download_url?: string | null
}

export interface SavedSearch {
  id: Id
  name: string
  query: string
  sort: IssueSort
  is_shared: boolean
  created_at: Iso
}

export interface TagKey {
  key: string
  name: string
  total_values: number
}

// --- events -----------------------------------------------------------------

export interface Frame {
  filename?: string | null
  abs_path?: string | null
  module?: string | null
  function?: string | null
  lineno?: number | null
  colno?: number | null
  in_app?: boolean
  context_line?: string | null
  pre_context?: string[]
  post_context?: string[]
  vars?: Record<string, unknown> | null
  platform?: string | null
  symbolicated?: boolean
  /** The frame before symbolication (minified location). */
  raw?: Omit<Frame, 'raw'> | null
  package?: string | null
  instruction_addr?: string | null
}

export interface Stacktrace {
  frames: Frame[]
  registers?: Record<string, string>
}

export interface Mechanism {
  type: string
  handled?: boolean
  synthetic?: boolean
  description?: string
  help_link?: string
  data?: Record<string, unknown>
  exception_id?: number
  parent_id?: number
  source?: string
  is_exception_group?: boolean
}

export interface ExceptionValue {
  type?: string | null
  value?: string | null
  module?: string | null
  mechanism?: Mechanism | null
  stacktrace?: Stacktrace | null
  raw_stacktrace?: Stacktrace | null
  thread_id?: number | string
}

export interface Breadcrumb {
  timestamp: Iso | number
  type?: string
  category?: string | null
  level?: Level | 'log' | string
  message?: string | null
  data?: Record<string, unknown> | null
}

export interface EventUser {
  id?: string | null
  email?: string | null
  username?: string | null
  ip_address?: string | null
  name?: string | null
  geo?: { country_code?: string; city?: string; region?: string } | null
  data?: Record<string, unknown> | null
}

export interface EventRequest {
  url?: string | null
  method?: string | null
  headers?: [string, string][]
  query_string?: string | [string, string][] | null
  data?: unknown
  cookies?: string | [string, string][] | Record<string, string> | null
  env?: Record<string, string> | null
  fragment?: string | null
}

export interface Thread {
  id: number | string
  name?: string | null
  crashed?: boolean
  current?: boolean
  main?: boolean
  stacktrace?: Stacktrace | null
}

export interface RuptaEvent {
  event_id: string
  project_id: Id
  issue_id: Id
  timestamp: Iso
  received: Iso
  level: Level
  platform: string
  message: string | null
  title: string
  culprit: string | null
  logger: string | null
  environment: string | null
  release: string | null
  dist: string | null
  server_name: string | null
  transaction: string | null
  fingerprint: string[]
  tags: [string, string][]
  user: EventUser | null
  request: EventRequest | null
  contexts: Record<string, Record<string, unknown>>
  sdk: { name: string; version: string; integrations?: string[]; packages?: { name: string; version: string }[] } | null
  exception: { values: ExceptionValue[] } | null
  threads: { values: Thread[] } | null
  breadcrumbs: { values: Breadcrumb[] } | null
  extra: Record<string, unknown> | null
  modules: Record<string, string> | null
  debug_meta: { images?: Record<string, unknown>[] } | null
  errors: { type: string; name?: string; value?: unknown }[] | null
  previous_event_id: string | null
  next_event_id: string | null
  /** Trace and session replay the event belongs to (also in contexts). */
  trace_id?: string | null
  span_id?: string | null
  replay_id?: string | null
}

export interface EventSummary {
  event_id: string
  timestamp: Iso
  title: string
  message: string | null
  level: Level
  environment: string | null
  release: string | null
  user: { id?: string | null; email?: string | null; ip_address?: string | null } | null
  browser: string | null
  os: string | null
  url: string | null
  transaction: string | null
}

// --- releases ---------------------------------------------------------------

export interface Deploy {
  id: Id
  environment: string
  name: string | null
  url: string | null
  date_started: Iso | null
  date_finished: Iso
}

export interface Release {
  version: string
  short_version?: string | null
  ref: string | null
  url: string | null
  date_created: Iso
  date_released: Iso | null
  first_event: Iso | null
  last_event: Iso | null
  projects: (ProjectRef & { new_issues: number })[]
  new_issues: number
  crash_free_sessions: number | null
  crash_free_users: number | null
  adoption: number | null
  sessions: number
  last_deploy: Deploy | null
  /** Sessions per hour/day over the period, for the sparkline. */
  stats?: [number, number][]
}

export interface ReleaseHealthPoint {
  ts: number
  total: number
  errored: number
  crashed: number
  abnormal: number
}

export interface ReleaseDetail extends Release {
  deploys: Deploy[]
  health: ReleaseHealthPoint[]
  users: number | null
  commit_count?: number
}

// --- crons ------------------------------------------------------------------

export type MonitorHealth = 'ok' | 'missed' | 'error' | 'timeout' | 'unknown'
export type CheckinStatus = 'in_progress' | 'ok' | 'error' | 'missed' | 'timeout'

export interface Checkin {
  id: string
  status: CheckinStatus
  environment: string | null
  duration_ms: number | null
  created_at: Iso
}

export interface Monitor {
  id: Id
  slug: string
  name: string
  project: ProjectRef
  schedule: { type: 'crontab' | 'interval'; value: string | number; unit?: string }
  timezone: string
  checkin_margin: number | null
  max_runtime: number | null
  status: 'active' | 'disabled'
  health: MonitorHealth
  last_checkin: Iso | null
  next_checkin: Iso | null
  created_at: Iso
  recent_checkins: Checkin[]
}

// --- alerts -----------------------------------------------------------------

export type AlertCondition =
  | { type: 'first_seen' }
  | { type: 'regression' }
  | { type: 'reappeared' }
  | { type: 'event_frequency'; count: number; interval: string }
  | { type: 'user_frequency'; count: number; interval: string }
  | { type: 'escalating' }

export type AlertFilter =
  { type: 'level'; match: 'eq' | 'gte'; level: Level } | { type: 'tag'; key: string; value: string } | { type: 'environment'; value: string }

export type AlertAction =
  | { type: 'email'; user_ids: Id[]; team_ids: Id[] }
  | { type: 'channel'; channel_id: Id }
  | { type: 'create_ticket'; integration_id: Id; target?: string; issue_type?: string; project?: string; labels?: string[] }

export interface AlertRule {
  id: Id
  project: ProjectRef
  name: string
  enabled: boolean
  /** The rule fires when any condition holds and all filters match. */
  conditions: AlertCondition[]
  filters: AlertFilter[]
  actions: AlertAction[]
  frequency_minutes: number
  environment: string | null
  created_by: string | null
  created_at: Iso
  updated_at: Iso
  last_fired?: Iso | null
}

export type AlertRuleInput = Omit<AlertRule, 'id' | 'project' | 'created_by' | 'created_at' | 'updated_at' | 'last_fired'>

// --- errors -----------------------------------------------------------------

export interface ApiErrorBody {
  error: { code: string; message: string; details?: Record<string, unknown> }
}
