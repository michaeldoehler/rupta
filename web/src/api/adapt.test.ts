import { describe, expect, it } from 'vitest'
import { issueUpdateBody, normAudit, normFeedback, normIssueDetail, normIssueStats, normMember, normRelease, normReleaseDetail, normRule } from './adapt'
import { ApiError } from './client'

const project = { id: '3', slug: 'web', name: 'Web', platform: 'javascript' }

describe('adapters for the server API', () => {
  it('normalises an issue with string releases and numeric strings', () => {
    const i = normIssueDetail({
      id: '7',
      count: '12',
      user_count: 3,
      first_release: 'web@1.0.0',
      last_release: null,
      stats: [['1700000000', '4']],
      substatus: 'new',
      is_bookmarked: true,
      metadata: {},
    })
    expect(i.count).toBe(12)
    expect(i.first_release).toEqual({ version: 'web@1.0.0', date: null })
    expect(i.last_release).toBeNull()
    expect((i as unknown as { stats: unknown }).stats).toEqual([[1700000000, 4]])
  })

  it('reads issue stats from the {events, users} object', () => {
    expect(normIssueStats({ period: '24h', resolution: 3600, events: [[1, 2]], users: [[1, 1]] })).toEqual([[1, 2]])
    expect(normIssueStats([[5, 6]])).toEqual([[5, 6]])
  })

  it('sends assignees as "user:{id}" and "team:{id}"', () => {
    expect(issueUpdateBody({ assignee: { type: 'team', id: '2' } })).toEqual({ assignee: 'team:2' })
    expect(issueUpdateBody({ assignee: null })).toEqual({ assignee: null })
    expect(issueUpdateBody({ status: 'resolved', resolved_in_next_release: true })).toEqual({ status: 'resolved' })
  })

  it('maps flat members, audit actors and feedback', () => {
    const m = normMember({ user_id: '9', email: 'a@b.c', name: 'Ada', role: 'admin', created_at: 'x', sso_linked: true, teams: ['web'] })
    expect(m).toMatchObject({ user: { id: '9', email: 'a@b.c', name: 'Ada' }, role: 'admin', sso: true })
    expect(normAudit({ id: '1', action: 'x', actor: { user_id: '9', email: 'a@b.c' }, data: {}, created_at: 'x' }).actor).toEqual({
      type: 'user',
      id: '9',
      name: 'a@b.c',
    })
    expect(normAudit({ id: '1', action: 'x', actor: { token_id: '4' }, data: {}, created_at: 'x' }).actor.type).toBe('token')
    const f = normFeedback({ id: '1', project_id: '3', issue_id: '7', message: 'hi', created_at: 'x' }, [project])
    expect(f.project.slug).toBe('web')
    expect(f.issue?.id).toBe('7')
  })

  it('reads release health from the health object and daily sessions', () => {
    const r = normRelease({
      version: 'web@2',
      date_created: 'x',
      new_issues: 1,
      projects: [{ id: '3', slug: 'web', name: 'Web', new_issues: 1 }],
      health: { total_sessions: 100, crash_free_sessions: 99.5, crash_free_users: null, adoption: 40 },
    })
    expect(r).toMatchObject({ sessions: 100, crash_free_sessions: 99.5, crash_free_users: null, adoption: 40 })
    const d = normReleaseDetail({ version: 'web@2', date_created: 'x', deploys: [], sessions: [[86400, 10, 1]] })
    expect(d.health).toEqual([{ ts: 86400, total: 10, crashed: 1, errored: 0, abnormal: 0 }])
  })

  it('maps alert rules with email user and team ids', () => {
    const r = normRule(
      {
        id: '1',
        name: 'n',
        enabled: true,
        conditions: [{ type: 'first_seen' }],
        filters: [{ type: 'tag', key: 'a', value: 'b' }],
        actions: [{ type: 'email', user_ids: [7], team_ids: [] }],
        created_by: 'Ada',
      },
      project,
    )
    expect(r.actions[0]).toEqual({ type: 'email', user_ids: ['7'], team_ids: [] })
    expect(r.project.slug).toBe('web')
  })

  it('turns details.field into a field error', () => {
    const e = new ApiError(422, 'validation_failed', 'slug: use 1 to 50 lowercase letters', { field: 'slug' })
    expect(e.fieldErrors).toEqual({ slug: 'Use 1 to 50 lowercase letters' })
  })
})
