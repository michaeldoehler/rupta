import { fireEvent, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import type { Breadcrumb } from '@/api/types'
import { renderUi } from '@/test/render'
import { Breadcrumbs } from './breadcrumbs'
import { RequestSection, toCurl } from './request'

const crumbs: Breadcrumb[] = Array.from({ length: 12 }, (_, i) => ({
  timestamp: new Date(Date.UTC(2026, 10, 14, 12, 0, i)).toISOString(),
  type: i % 2 ? 'http' : 'navigation',
  category: i % 2 ? 'fetch' : 'navigation',
  level: i === 5 ? 'error' : 'info',
  message: null,
  data: i % 2 ? { method: 'GET', url: `/api/item/${i}`, status_code: i === 5 ? 500 : 200 } : { from: '/a', to: `/page-${i}` },
}))

describe('Breadcrumbs', () => {
  it('shows the newest first, collapses older ones and filters by text', async () => {
    renderUi(<Breadcrumbs crumbs={crumbs} eventTime={new Date(Date.UTC(2026, 10, 14, 12, 0, 12)).toISOString()} />)
    expect(await screen.findByText('/api/item/11')).toBeInTheDocument()
    expect(screen.queryByText('/api/item/1')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Show 4 older breadcrumbs/ }))
    expect(screen.getByText('/api/item/1')).toBeInTheDocument()
    fireEvent.change(screen.getByRole('textbox', { name: 'Search breadcrumbs' }), { target: { value: 'item/5' } })
    expect(screen.getAllByRole('listitem')).toHaveLength(1)
    expect(screen.getByText('[500]')).toBeInTheDocument()
  })

  it('switches between relative and absolute time', async () => {
    const user = userEvent.setup()
    renderUi(<Breadcrumbs crumbs={crumbs} eventTime={new Date(Date.UTC(2026, 10, 14, 12, 0, 12)).toISOString()} />)
    expect(await screen.findByText('−1.0s')).toBeInTheDocument()
    await user.click(screen.getByRole('radio', { name: 'Absolute' }))
    expect(screen.queryByText('−1.0s')).not.toBeInTheDocument()
  })
})

describe('Request', () => {
  const req = { url: 'https://api.example.com/orders', method: 'POST', headers: [['Content-Type', 'application/json']] as [string, string][], query_string: [['page', '2']] as [string, string][], data: { a: "it's" } }
  it('builds a cURL command', () => {
    expect(toCurl(req)).toBe(`curl -X POST 'https://api.example.com/orders?page=2' \\\n  -H 'Content-Type: application/json' \\\n  --data '{"a":"it'\\''s"}'`)
  })
  it('renders method, url, query, body and headers', async () => {
    renderUi(<RequestSection req={req} />)
    expect(await screen.findByText('POST')).toBeInTheDocument()
    expect(screen.getByText('https://api.example.com/orders')).toBeInTheDocument()
    expect(screen.getByRole('rowheader', { name: 'page' })).toBeInTheDocument()
    expect(screen.getByText(/"a": "it's"/)).toBeInTheDocument()
  })
})
