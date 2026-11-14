import { describe, expect, it } from 'vitest'
import { describeAction } from '@/features/alerts/describe'
import { normRule } from './adapt'
import { normIntegration, normLink, normProjectIntegration, normTarget } from './integrations'

describe('integration adapters', () => {
  it('keeps the one-time webhook secret and the write-only token hint', () => {
    const i = normIntegration({
      id: 4,
      kind: 'jira_cloud',
      name: 'Jira',
      config: { base_url: 'https://acme.atlassian.net', email: 'bot@acme.dev' },
      token_hint: '****abcd',
      has_webhook_secret: true,
      webhook_url: 'https://rupta.example/api/integrations/jira_cloud/4/webhook',
      account: { id: '5f1c' },
      created_at: '2026-11-01T00:00:00Z',
      webhook_secret: 'whsec_1',
    })
    expect(i.id).toBe('4')
    expect(i.enabled).toBe(true)
    expect(i.account).toEqual({ id: '5f1c', name: '5f1c' })
    expect(i.updated_at).toBe('2026-11-01T00:00:00Z')
    expect(i.webhook_secret).toBe('whsec_1')
    expect(i.last_error).toBeNull()
    expect(normIntegration({ id: 5, kind: 'linear', name: 'L', account: null }).account).toBeNull()
  })

  it('reads targets with missing lists', () => {
    const t = normTarget({ id: 'acme/api', name: 'acme/api', states: [{ id: 's', name: 'Done', category: 'completed' }] })
    expect(t.issue_types).toEqual([])
    expect(t.key).toBeNull()
    expect(t.states[0].category).toBe('completed')
  })

  it('reads project defaults and links', () => {
    const d = normProjectIntegration({ integration: { id: 9, kind: 'github', name: 'GitHub' }, target: 'acme/api', labels: ['bug'], incoming_sync: true })
    expect(d.integration).toEqual({ id: '9', kind: 'github', name: 'GitHub', enabled: true })
    expect(d.outgoing_comment).toBe(false)
    const l = normLink({
      id: 1,
      issue_id: 7,
      integration: { id: 9, kind: 'github', name: 'GitHub' },
      external_id: 'acme/api#5',
      url: 'u',
      status_category: 'weird',
    })
    expect(l.external_key).toBe('acme/api#5')
    expect(l.status_category).toBe('open')
    expect(l.synced_at).toBeNull()
  })

  it('reads and describes a create_ticket alert action', () => {
    const project = { id: '3', slug: 'web', name: 'Web', platform: 'javascript' }
    const r = normRule(
      {
        id: 1,
        name: 'Tickets',
        conditions: [{ type: 'first_seen' }],
        filters: [],
        actions: [{ type: 'create_ticket', integration_id: 9, target: 'OPS', labels: ['rupta'] }],
      },
      project,
    )
    expect(r.actions[0]).toEqual({ type: 'create_ticket', integration_id: '9', target: 'OPS', labels: ['rupta'] })
    expect(describeAction(r.actions[0], [], [], [], [{ id: '9', name: 'Jira' }])).toBe('Create a ticket in Jira (OPS)')
    expect(describeAction(r.actions[0], [], [])).toBe('Create a ticket in an issue tracker (OPS)')
  })
})
