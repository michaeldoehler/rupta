import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { getWorld } from '@/mocks/handlers'
import { renderApp } from './render'

describe('organization icon', () => {
  it('shows the favicon in the organization menu and falls back to the initial', async () => {
    const user = userEvent.setup()
    renderApp('/acme/issues')
    const trigger = await screen.findByRole('button', { name: 'Organization: Acme' })
    expect(trigger.querySelector('img')).toHaveAttribute('src', getWorld().orgs[0].icon_url)
    await user.click(trigger)
    const studio = await screen.findByRole('menuitem', { name: /Side Studio/ })
    expect(studio.querySelector('img')).toBeNull()
    expect(studio).toHaveTextContent('S')
  })

  it('sets, refuses and removes the website domain', async () => {
    const user = userEvent.setup()
    renderApp('/acme/settings')
    const input = await screen.findByLabelText('Website')
    expect(input).toHaveValue('acme.dev')
    const save = screen.getByRole('button', { name: 'Save' })
    expect(save).toBeDisabled()

    await user.clear(input)
    await user.type(input, 'https://www.example.org/about')
    await user.click(save)
    await waitFor(() => expect(getWorld().orgs[0].domain).toBe('www.example.org'))
    expect(getWorld().orgs[0].icon_url).toMatch(/^data:image\/svg\+xml,/)

    await user.clear(input)
    await user.type(input, 'nothing.invalid')
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(await screen.findByText(/could not fetch an icon from nothing\.invalid/)).toBeInTheDocument()
    expect(getWorld().orgs[0].domain).toBe('www.example.org')

    await user.clear(input)
    await user.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(getWorld().orgs[0].icon_url).toBeNull())
    const menu = screen.getByRole('button', { name: 'Organization: Acme' })
    await waitFor(() => expect(within(menu).queryByRole('img')).toBeNull())
    expect(menu.querySelector('img')).toBeNull()
  })
})
