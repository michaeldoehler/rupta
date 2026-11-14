// Issue tracker integrations (Jira Cloud, Jira Data Center, Linear, GitHub,
// GitLab): organization integrations, project defaults, issue links and the
// ticket draft. See docs/integrations.md.

import { request, requestList } from './client'
import type { Id, Iso } from './types'

export type IntegrationKind = 'jira_cloud' | 'jira_server' | 'linear' | 'github' | 'gitlab'

export interface IntegrationConfig {
  base_url?: string
  email?: string
  resolve_transition?: string
  reopen_transition?: string
  resolve_state?: string
  reopen_state?: string
}

export interface Integration {
  id: Id
  kind: IntegrationKind
  name: string
  enabled: boolean
  config: IntegrationConfig
  token_hint: string | null
  has_webhook_secret: boolean
  webhook_url: string
  account: { id: string; name: string } | null
  created_by: string | null
  created_at: Iso
  updated_at: Iso
  last_error: string | null
  last_error_at: Iso | null
}

/** A created (or rotated) integration; the secret is shown once. */
export interface IntegrationWithSecret extends Integration {
  webhook_secret?: string | null
}

export interface IntegrationRef {
  id: Id
  kind: IntegrationKind
  name: string
  enabled: boolean
}

export interface Choice {
  id: string
  name: string
}

export interface Target {
  id: string
  name: string
  key: string | null
  issue_types: Choice[]
  projects: Choice[]
  labels: Choice[]
  states: (Choice & { category: string })[]
}

export interface ProjectIntegration {
  integration: IntegrationRef
  target: string
  target_name: string | null
  issue_type: string | null
  project: string | null
  labels: string[]
  incoming_sync: boolean
  outgoing_transition: boolean
  outgoing_comment: boolean
  updated_at: Iso
}

export interface IssueLink {
  id: Id
  issue_id: Id
  integration: IntegrationRef
  external_id: string
  external_key: string
  url: string
  title: string | null
  status: string | null
  status_category: 'open' | 'done'
  assignee: string | null
  created_by: string | null
  created_at: Iso
  synced_at: Iso | null
}

export interface TestResult {
  ok: boolean
  account: { id: string; name: string } | null
  error: string | null
  reason: string | null
}

export interface IntegrationInput {
  kind?: IntegrationKind
  name?: string
  enabled?: boolean
  config?: IntegrationConfig
  token?: string
  webhook_secret?: string
  rotate_webhook_secret?: boolean
  validate?: boolean
}

export interface ProjectDefaultsInput {
  target: string
  target_name?: string | null
  issue_type?: string | null
  project?: string | null
  labels?: string[]
  incoming_sync?: boolean
  outgoing_transition?: boolean
  outgoing_comment?: boolean
}

export interface CreateLinkInput {
  integration_id: Id
  mode: 'create' | 'link'
  reference?: string
  target?: string
  issue_type?: string
  project?: string
  labels?: string[]
  title?: string
  description?: string
}

type Raw = Record<string, unknown>
const obj = (v: unknown): Raw => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Raw) : {})
const str = (v: unknown): string | null => (v === null || v === undefined || v === '' ? null : String(v))
const choices = (v: unknown): Choice[] =>
  (Array.isArray(v) ? v : []).map((c) =>
    typeof c === 'string' ? { id: c, name: c } : { id: String(obj(c).id ?? obj(c).name), name: String(obj(c).name ?? obj(c).id) },
  )

function ref(v: unknown): IntegrationRef {
  const o = obj(v)
  return { id: String(o.id), kind: o.kind as IntegrationKind, name: String(o.name ?? ''), enabled: o.enabled !== false }
}

export function normIntegration(v: unknown): IntegrationWithSecret {
  const o = obj(v)
  const acc = obj(o.account)
  return {
    id: String(o.id),
    kind: o.kind as IntegrationKind,
    name: String(o.name ?? ''),
    enabled: o.enabled !== false,
    config: obj(o.config) as IntegrationConfig,
    token_hint: str(o.token_hint),
    has_webhook_secret: Boolean(o.has_webhook_secret),
    webhook_url: String(o.webhook_url ?? ''),
    account: o.account ? { id: String(acc.id ?? ''), name: String(acc.name ?? acc.id ?? '') } : null,
    created_by: str(o.created_by),
    created_at: String(o.created_at ?? ''),
    updated_at: String(o.updated_at ?? o.created_at ?? ''),
    last_error: str(o.last_error),
    last_error_at: str(o.last_error_at),
    webhook_secret: str(o.webhook_secret),
  }
}

export function normTarget(v: unknown): Target {
  const o = obj(v)
  return {
    id: String(o.id),
    name: String(o.name ?? o.id),
    key: str(o.key),
    issue_types: choices(o.issue_types),
    projects: choices(o.projects),
    labels: choices(o.labels),
    states: (Array.isArray(o.states) ? o.states : []).map((s) => ({
      id: String(obj(s).id),
      name: String(obj(s).name ?? obj(s).id),
      category: String(obj(s).category ?? ''),
    })),
  }
}

export function normProjectIntegration(v: unknown): ProjectIntegration {
  const o = obj(v)
  return {
    integration: ref(o.integration),
    target: String(o.target ?? ''),
    target_name: str(o.target_name),
    issue_type: str(o.issue_type),
    project: str(o.project),
    labels: (Array.isArray(o.labels) ? o.labels : []).map(String),
    incoming_sync: Boolean(o.incoming_sync),
    outgoing_transition: Boolean(o.outgoing_transition),
    outgoing_comment: Boolean(o.outgoing_comment),
    updated_at: String(o.updated_at ?? ''),
  }
}

export function normLink(v: unknown): IssueLink {
  const o = obj(v)
  return {
    id: String(o.id),
    issue_id: String(o.issue_id ?? ''),
    integration: ref(o.integration),
    external_id: String(o.external_id ?? ''),
    external_key: String(o.external_key ?? o.external_id ?? ''),
    url: String(o.url ?? ''),
    title: str(o.title),
    status: str(o.status),
    status_category: o.status_category === 'done' ? 'done' : 'open',
    assignee: str(o.assignee),
    created_by: str(o.created_by),
    created_at: String(o.created_at ?? ''),
    synced_at: str(o.synced_at),
  }
}

const e = encodeURIComponent

export const integrationsApi = {
  list: (org: string) => requestList<unknown>(`/orgs/${e(org)}/integrations`).then((l) => l.map(normIntegration)),
  get: (org: string, id: Id) => request<unknown>(`/orgs/${e(org)}/integrations/${e(id)}`).then(normIntegration),
  create: (org: string, body: IntegrationInput) => request<unknown>(`/orgs/${e(org)}/integrations`, { method: 'POST', body }).then(normIntegration),
  update: (org: string, id: Id, body: IntegrationInput) =>
    request<unknown>(`/orgs/${e(org)}/integrations/${e(id)}`, { method: 'PATCH', body }).then(normIntegration),
  remove: (org: string, id: Id) => request<void>(`/orgs/${e(org)}/integrations/${e(id)}`, { method: 'DELETE' }),
  test: (org: string, id: Id) =>
    request<unknown>(`/orgs/${e(org)}/integrations/${e(id)}/test`, { method: 'POST' }).then((r): TestResult => {
      const o = obj(r)
      return {
        ok: Boolean(o.ok),
        account: o.account ? { id: String(obj(o.account).id ?? ''), name: String(obj(o.account).name ?? '') } : null,
        error: str(o.error),
        reason: str(o.reason),
      }
    }),
  targets: (org: string, id: Id) => requestList<unknown>(`/orgs/${e(org)}/integrations/${e(id)}/targets`).then((l) => l.map(normTarget)),

  projectDefaults: (org: string, project: string) =>
    requestList<unknown>(`/projects/${e(org)}/${e(project)}/integrations`).then((l) => l.map(normProjectIntegration)),
  saveProjectDefaults: (org: string, project: string, integrationId: Id, body: ProjectDefaultsInput) =>
    request<unknown>(`/projects/${e(org)}/${e(project)}/integrations/${e(integrationId)}`, { method: 'PUT', body }).then(normProjectIntegration),
  removeProjectDefaults: (org: string, project: string, integrationId: Id) =>
    request<void>(`/projects/${e(org)}/${e(project)}/integrations/${e(integrationId)}`, { method: 'DELETE' }),

  links: (issueId: Id) => requestList<unknown>(`/issues/${e(issueId)}/links`).then((l) => l.map(normLink)),
  createLink: (issueId: Id, body: CreateLinkInput) => request<unknown>(`/issues/${e(issueId)}/links`, { method: 'POST', body }).then(normLink),
  refreshLink: (issueId: Id, linkId: Id) => request<unknown>(`/issues/${e(issueId)}/links/${e(linkId)}/refresh`, { method: 'POST' }).then(normLink),
  unlink: (issueId: Id, linkId: Id) => request<void>(`/issues/${e(issueId)}/links/${e(linkId)}`, { method: 'DELETE' }),
  draft: (issueId: Id) =>
    request<{ title?: string; description?: string }>(`/issues/${e(issueId)}/ticket-draft`).then((r) => ({
      title: r?.title ?? '',
      description: r?.description ?? '',
    })),
}

export const ik = {
  list: (org: string) => ['orgs', org, 'integrations'] as const,
  targets: (org: string, id: Id) => ['orgs', org, 'integrations', id, 'targets'] as const,
  projectDefaults: (org: string, project: string) => ['projects', org, project, 'integrations'] as const,
  links: (issueId: Id) => ['issues', 'detail', issueId, 'links'] as const,
  draft: (issueId: Id) => ['issues', 'detail', issueId, 'ticket-draft'] as const,
}
