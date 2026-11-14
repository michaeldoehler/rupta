import { fireEvent, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { renderUi } from '@/test/render'
import { SearchBar } from './search-bar'

describe('SearchBar', () => {
  it('suggests keys, then values, and submits the query', async () => {
    const onSubmit = vi.fn()
    const user = userEvent.setup()
    renderUi(<SearchBar org="acme" projects={[]} value="" onSubmit={onSubmit} />)
    const input = await screen.findByRole('combobox', { name: 'Search issues' })
    await user.click(input)
    await user.type(input, 'lev')
    expect(await screen.findByRole('option', { name: /^level:Severity/ })).toHaveAttribute('aria-selected', 'true')
    await user.keyboard('{Tab}')
    expect(input).toHaveValue('level:')
    expect(await screen.findByRole('option', { name: /^fatal/ })).toBeInTheDocument()
    await user.type(input, 'err')
    await user.keyboard('{Enter}')
    expect(input).toHaveValue('level:error ')
    await user.keyboard('{Enter}')
    expect(onSubmit).toHaveBeenCalledWith('level:error')
  })

  it('loads values for tag keys from the server', async () => {
    const user = userEvent.setup()
    renderUi(<SearchBar org="acme" projects={[]} value="" onSubmit={() => {}} />)
    const input = await screen.findByRole('combobox', { name: 'Search issues' })
    await user.click(input)
    await user.type(input, 'environment:')
    expect(await screen.findByRole('option', { name: 'production' })).toBeInTheDocument()
  })

  it('navigates suggestions with the arrow keys and closes on Escape', async () => {
    const user = userEvent.setup()
    renderUi(<SearchBar org="acme" projects={[]} value="" onSubmit={() => {}} />)
    const input = await screen.findByRole('combobox')
    await user.click(input)
    await user.type(input, 'is:')
    const first = await screen.findByRole('option', { name: /unresolved/ })
    expect(first).toHaveAttribute('aria-selected', 'true')
    await user.keyboard('{ArrowDown}')
    expect(screen.getByRole('option', { name: /^resolved/ })).toHaveAttribute('aria-selected', 'true')
    fireEvent.keyDown(input, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('listbox')).not.toBeInTheDocument())
  })

  it('clears the search', async () => {
    const onSubmit = vi.fn()
    const user = userEvent.setup()
    renderUi(<SearchBar org="acme" projects={[]} value="is:resolved" onSubmit={onSubmit} />)
    await user.click(await screen.findByRole('button', { name: 'Clear search' }))
    expect(onSubmit).toHaveBeenCalledWith('')
  })
})
