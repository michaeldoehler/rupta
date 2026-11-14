import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { server } from '@/mocks/node'
import { api } from './endpoints'

describe('user preferences', () => {
  it('reads preferences from the server and sends them in its shape', async () => {
    let sent: unknown = null
    server.use(
      http.get('/api/v1/user', () =>
        HttpResponse.json({ id: '1', email: 'a@b.c', name: 'A', avatar_url: null, is_superuser: false, preferences: { timezone: null, clock: '12h', theme: 'dark', notifications: { issue_alerts: false, workflow: 'all', weekly_report: true } } }),
      ),
      http.patch('/api/v1/user', async ({ request }) => {
        sent = await request.json()
        return HttpResponse.json({ id: '1', email: 'a@b.c', name: 'A', avatar_url: null, is_superuser: false, preferences: { timezone: 'Europe/Berlin', clock: '24h', theme: 'dark', notifications: {} } })
      }),
    )
    const u = await api.user.get()
    expect(u.preferences).toMatchObject({ timezone: 'browser', clock_24h: false, theme: 'dark', notifications: { issue_alerts: false, workflow: 'all', weekly_report: true } })
    const next = await api.user.update({ preferences: { ...u.preferences, timezone: 'Europe/Berlin', clock_24h: true } })
    expect(sent).toEqual({ preferences: { timezone: 'Europe/Berlin', clock: '24h', theme: 'dark', notifications: { issue_alerts: false, workflow: 'all', weekly_report: true } } })
    expect(next.preferences.timezone).toBe('Europe/Berlin')
  })
})
