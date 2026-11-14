import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { getWorld } from '@/mocks/handlers'
import { server } from '@/mocks/node'
import { renderApp } from './render'

/** The busiest unresolved issue of web-app: the mock links it to a Jira ticket. */
function linkedIssue() {
  const w = getWorld()
  const web = w.projects.find((p) => p.slug === 'web-app')!
  const top = [...w.issues]
    .filter((i) => i.status === 'unresolved')
    .sort((a, b) => b.count - a.count)
    .slice(0, 12)
  return top.find((i) => i.project_id === web.id)!
}

describe('organization integrations', () => {
  it('lists trackers with their last error and tests a connection', async () => {
    const user = userEvent.setup()
    renderApp('/acme/settings/integrations')
    const list = await screen.findByRole('list', { name: 'Integrations' })
    const github = within(list).getByRole('listitem', { name: 'GitHub' })
    expect(within(github).getByText(/403 Forbidden/)).toBeInTheDocument()
    expect(within(github).getByText('token ****wxyz')).toBeInTheDocument()
    const jira = within(list).getByRole('listitem', { name: 'Jira' })
    await user.click(within(jira).getByRole('button', { name: /Test/ }))
    expect(await within(jira).findByRole('status')).toHaveTextContent('Connection works: signed in as rupta bot.')
    await user.click(within(github).getByRole('button', { name: /Test/ }))
    expect(await within(github).findByRole('status')).toHaveTextContent('The token cannot comment on acme/api issues. (forbidden)')
  })

  it('shows a refused token and then the webhook secret once', async () => {
    const user = userEvent.setup()
    renderApp('/acme/settings/integrations')
    await screen.findByRole('list', { name: 'Integrations' })
    await user.click(screen.getByRole('button', { name: /Connect tracker/ }))
    const dialog = await screen.findByRole('dialog', { name: 'Connect an issue tracker' })
    await user.click(within(dialog).getByRole('radio', { name: /GitLab/ }))
    expect(within(dialog).getByLabelText('Name')).toHaveValue('GitLab')
    await user.type(within(dialog).getByLabelText(/Access token/), 'glpat-bad')
    await user.click(within(dialog).getByRole('button', { name: 'Connect' }))
    expect(await within(dialog).findByText(/GitLab refused the token/)).toBeInTheDocument()
    await user.clear(within(dialog).getByLabelText(/Access token/))
    await user.type(within(dialog).getByLabelText(/Access token/), 'glpat-good')
    await user.click(within(dialog).getByRole('button', { name: 'Connect' }))
    const webhook = await screen.findByRole('dialog', { name: 'Set up the webhook in GitLab' })
    expect(within(webhook).getByText(/Shown only now/)).toBeInTheDocument()
    expect(within(webhook).getByText(/^whsec_/)).toBeInTheDocument()
    await user.click(within(webhook).getByRole('button', { name: 'Done' }))
    expect(await screen.findByRole('listitem', { name: 'GitLab' })).toBeInTheDocument()
  })
})

describe('project integration defaults', () => {
  it('saves the target and sync switches', async () => {
    const user = userEvent.setup()
    let saved: Record<string, unknown> | null = null
    server.events.on('request:start', async ({ request }) => {
      if (request.method === 'PUT' && request.url.includes('/integrations/')) saved = (await request.clone().json()) as Record<string, unknown>
    })
    renderApp('/acme/settings/projects/web-app/integrations')
    expect(await screen.findByText('New tickets go to Web storefront.')).toBeInTheDocument()
    const comments = (await screen.findAllByRole('switch', { name: 'Outgoing comments' }))[0]
    await user.click(comments)
    await user.click(screen.getAllByRole('button', { name: 'Save defaults' })[0])
    expect(await screen.findByText('Defaults for Jira saved')).toBeInTheDocument()
    await waitFor(() => expect(saved).toMatchObject({ target: 'WEB', issue_type: 'Bug', labels: ['rupta'], outgoing_comment: false }))
    server.events.removeAllListeners()
  })
})

describe('linked tickets', () => {
  it('shows linked tickets and creates one from the draft', async () => {
    const user = userEvent.setup()
    const issue = linkedIssue()
    renderApp(`/acme/issues/${issue.id}`)
    const section = await screen.findByTestId('linked-tickets')
    expect(await within(section).findByRole('link', { name: /^Open WEB-\d+ in Jira$/ })).toBeInTheDocument()
    await user.click(within(section).getByRole('button', { name: /Create ticket/ }))
    const dialog = await screen.findByRole('dialog', { name: 'Create a ticket' })
    expect(await within(dialog).findByText(/Goes to/)).toHaveTextContent('Goes to Web storefront as Bug, labels rupta (project defaults).')
    await waitFor(() => expect(within(dialog).getByLabelText('Title')).toHaveValue(`[${issue.short_id}] ${issue.title}`.slice(0, 255)))
    expect((within(dialog).getByLabelText('Description') as HTMLTextAreaElement).value).toContain('**Top tags**')
    await user.click(within(dialog).getByRole('button', { name: 'Create ticket' }))
    expect(await screen.findByText(/^Created WEB-\d+$/)).toBeInTheDocument()
    await waitFor(() => expect(within(section).getAllByRole('link', { name: /^Open WEB-\d+ in Jira$/ })).toHaveLength(2))
  })

  it('explains a duplicate and an unknown reference, and unlinks', async () => {
    const user = userEvent.setup()
    const issue = linkedIssue()
    renderApp(`/acme/issues/${issue.id}`)
    const section = await screen.findByTestId('linked-tickets')
    const open = await within(section).findByRole('link', { name: /^Open WEB-\d+ in Jira$/ })
    const key = open.getAttribute('aria-label')!.split(' ')[1]
    await user.click(within(section).getByRole('button', { name: /Link existing/ }))
    const dialog = await screen.findByRole('dialog', { name: 'Link an existing ticket' })
    await user.type(await within(dialog).findByLabelText('Ticket'), 'not a ticket')
    await user.click(within(dialog).getByRole('button', { name: 'Link ticket' }))
    expect(await within(dialog).findByText(/^Not a ticket of Jira\. Use a key or URL such as OPS-42/)).toBeInTheDocument()
    await user.clear(within(dialog).getByLabelText('Ticket'))
    await user.type(within(dialog).getByLabelText('Ticket'), key)
    await user.click(within(dialog).getByRole('button', { name: 'Link ticket' }))
    expect(await within(dialog).findByText('This ticket is already linked to the issue.')).toBeInTheDocument()
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    await user.click(within(section).getByRole('button', { name: `Unlink ${key}` }))
    expect(await screen.findByText('Ticket unlinked (it stays in the tracker)')).toBeInTheDocument()
    await waitFor(() => expect(within(section).getByText('No tickets yet.')).toBeInTheDocument())
  })

  it('shows the tracker error for a ticket it cannot read', async () => {
    server.use(
      http.post('/api/v1/issues/:id/links', () =>
        HttpResponse.json(
          { error: { code: 'integration_error', message: 'Jira answered 401 Unauthorized', details: { reason: 'unauthorized' } } },
          { status: 502 },
        ),
      ),
    )
    const user = userEvent.setup()
    renderApp(`/acme/issues/${linkedIssue().id}`)
    const section = await screen.findByTestId('linked-tickets')
    await user.click(within(section).getByRole('button', { name: /Link existing/ }))
    const dialog = await screen.findByRole('dialog', { name: 'Link an existing ticket' })
    await user.type(await within(dialog).findByLabelText('Ticket'), 'OPS-1')
    await user.click(within(dialog).getByRole('button', { name: 'Link ticket' }))
    expect(await within(dialog).findByText('Jira Cloud refused the request: Jira answered 401 Unauthorized (unauthorized)')).toBeInTheDocument()
  })
})

describe('paging channels', () => {
  it('stores the PagerDuty key write-only and shows the refusal of an internal URL', async () => {
    const user = userEvent.setup()
    renderApp('/acme/settings/channels')
    await user.click(await screen.findByRole('button', { name: /Add channel/ }))
    const dialog = await screen.findByRole('dialog', { name: 'Add notification channel' })
    await user.click(within(dialog).getByRole('radio', { name: /PagerDuty/ }))
    await user.type(within(dialog).getByLabelText('Name'), 'Primary on-call')
    await user.type(within(dialog).getByLabelText(/Integration key/), 'R0UT1NGKEY0123456789abcdef012345')
    await user.click(within(dialog).getByRole('button', { name: 'Add channel' }))
    expect(await screen.findByText('Channel added')).toBeInTheDocument()
    const stored = getWorld().channels.find((c) => c.name === 'Primary on-call')!
    expect(stored.kind).toBe('pagerduty')
    expect(await screen.findByText('US region · key stored')).toBeInTheDocument()
    expect(document.body.textContent).not.toContain('R0UT1NGKEY')

    await user.click(screen.getByRole('button', { name: /Add channel/ }))
    const second = await screen.findByRole('dialog', { name: 'Add notification channel' })
    await user.click(within(second).getByRole('radio', { name: /Webhook/ }))
    await user.type(within(second).getByLabelText('Name'), 'Local hook')
    await user.type(within(second).getByLabelText(/URL/), 'http://127.0.0.1:9000/hook')
    await user.click(within(second).getByRole('button', { name: 'Add channel' }))
    expect(await within(second).findByText(/the address is internal/)).toBeInTheDocument()
  })
})
