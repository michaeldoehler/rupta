import { act, fireEvent, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { getWorld, resetMock } from '@/mocks/handlers'
import { server } from '@/mocks/node'
import { renderApp } from './render'

async function firstRows() {
  const grid = await screen.findByRole('grid', { name: 'Issues' })
  await waitFor(() => expect(within(grid).getAllByRole('row').length).toBeGreaterThan(5), { timeout: 5000 })
  return within(grid).getAllByRole('row')
}

describe('issue stream', () => {
  it('lists unresolved issues and resolves the focused one with the keyboard', async () => {
    const { router } = renderApp('/acme/issues')
    const rows = await firstRows()
    const id = rows[0].getAttribute('data-issue-id')!
    expect(router.state.location.pathname).toBe('/acme/issues')
    fireEvent.keyDown(window, { key: 'j' })
    await waitFor(() => expect(rows[0]).toHaveAttribute('data-focused', 'true'))
    fireEvent.keyDown(window, { key: 'e' })
    expect(await screen.findByText('Issue resolved')).toBeInTheDocument()
    await waitFor(() => expect(getWorld().issues.find((i) => i.id === id)?.status).toBe('resolved'))
  })

  it('resolves several selected issues from the bulk toolbar', async () => {
    const user = userEvent.setup()
    renderApp('/acme/issues')
    const rows = await firstRows()
    const ids = rows.slice(0, 3).map((r) => r.getAttribute('data-issue-id')!)
    for (const r of rows.slice(0, 3)) await user.click(within(r).getByRole('checkbox'))
    expect(screen.getByText('3 selected')).toBeInTheDocument()
    await user.click(within(screen.getByRole('toolbar', { name: 'Bulk actions' })).getByRole('button', { name: /^Resolve$/ }))
    expect(await screen.findByText('3 issues resolved')).toBeInTheDocument()
    await waitFor(() => expect(ids.every((id) => getWorld().issues.find((i) => i.id === id)?.status === 'resolved')).toBe(true))
  })

  it('rolls back an optimistic update when the server fails', async () => {
    server.use(http.patch('/api/v1/issues/:id', () => HttpResponse.json({ error: { code: 'internal', message: 'Database is read-only' } }, { status: 500 })))
    renderApp('/acme/issues')
    const rows = await firstRows()
    const id = rows[0].getAttribute('data-issue-id')!
    fireEvent.keyDown(window, { key: 'j' })
    fireEvent.keyDown(window, { key: 'b' })
    expect(await screen.findByText('Could not update the issue')).toBeInTheDocument()
    expect(screen.getByText('Database is read-only')).toBeInTheDocument()
    expect(getWorld().issues.find((i) => i.id === id)?.bookmarked_by.has('1')).toBe(false)
    await waitFor(() => expect(within(screen.getAllByRole('row')[0]).queryByLabelText('Bookmarked')).not.toBeInTheDocument())
  })

  it('switches tabs through the URL', async () => {
    const user = userEvent.setup()
    const { router } = renderApp('/acme/issues')
    await firstRows()
    await user.click(screen.getByRole('button', { name: 'Resolved' }))
    await waitFor(() => expect(router.state.location.search).toMatchObject({ query: 'is:resolved' }))
    expect(screen.getByRole('button', { name: 'Resolved' })).toHaveAttribute('aria-current', 'page')
  })

  it('shows an empty state when nothing matches', async () => {
    renderApp('/acme/issues?query=%22no%20such%20error%20anywhere%22')
    expect(await screen.findByText('No issues match')).toBeInTheDocument()
  })
})

describe('authentication', () => {
  it('redirects to the login page and signs in', async () => {
    resetMock({ auth: { setupDone: true, userId: null } })
    const user = userEvent.setup()
    const { router } = renderApp('/acme/issues')
    const email = await screen.findByLabelText('Email')
    expect(router.state.location.pathname).toBe('/login')
    expect(screen.getByRole('link', { name: 'Continue with Zitadel' })).toHaveAttribute('href', '/auth/sso/zitadel/start?next=%2Facme%2Fissues')
    await user.type(email, 'jane@acme.dev')
    await user.type(screen.getByLabelText('Password'), 'wrong')
    await user.click(screen.getByRole('button', { name: 'Sign in' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Email or password is incorrect.')
    await user.clear(screen.getByLabelText('Password'))
    await user.type(screen.getByLabelText('Password'), 'password')
    await user.click(screen.getByRole('button', { name: 'Sign in' }))
    await waitFor(() => expect(router.state.location.pathname).toBe('/acme/issues'))
  })

  it('runs the setup wizard on a fresh instance', async () => {
    resetMock({ auth: { setupDone: false, userId: null } })
    const user = userEvent.setup()
    const { router } = renderApp('/')
    await user.type(await screen.findByLabelText('Name'), 'Ada Admin')
    await waitFor(() => expect(router.state.location.pathname).toBe('/setup'))
    await user.type(screen.getByLabelText('Email'), 'ada@example.com')
    await user.type(screen.getByLabelText('Password'), 'short')
    await user.click(screen.getByRole('button', { name: 'Continue' }))
    expect(await screen.findByText('Use at least 10 characters.')).toBeInTheDocument()
    await user.clear(screen.getByLabelText('Password'))
    await user.type(screen.getByLabelText('Password'), 'a-long-password')
    await user.click(screen.getByRole('button', { name: 'Continue' }))
    await user.type(await screen.findByLabelText('Organization name'), 'Initech')
    await user.click(screen.getByRole('button', { name: 'Create account' }))
    await waitFor(() => expect(router.state.location.pathname).toBe('/acme/issues'))
    expect(getWorld().orgs[0].name).toBe('Initech')
  })
})

describe('command palette', () => {
  it('jumps to an issue by short id', async () => {
    const user = userEvent.setup()
    const { router } = renderApp('/acme/issues')
    await firstRows()
    act(() => {
      fireEvent.keyDown(window, { key: 'k', metaKey: true })
    })
    const input = await screen.findByPlaceholderText(/Type a command/)
    const target = getWorld().issues.find((i) => i.short_id === 'API-1')!
    await user.type(input, 'api-1')
    await user.click(await screen.findByRole('option', { name: /Go to issue API-1/ }))
    await waitFor(() => expect(router.state.location.pathname).toBe(`/acme/issues/${target.id}`))
  })
})
