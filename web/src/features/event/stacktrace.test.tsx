import { fireEvent, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import type { ExceptionValue, Frame } from '@/api/types'
import { renderUi } from '@/test/render'
import { displayFrame, ExceptionBlock, rawStacktrace, type StacktraceOptions } from './stacktrace'

const lib = (n: number): Frame => ({ filename: `node_modules/lib/file${n}.js`, function: `libFn${n}`, lineno: 10 + n, colno: 1, in_app: false })
const crash: Frame = {
  filename: './src/lib/money.ts',
  abs_path: 'webpack:///src/lib/money.ts',
  function: 'formatPrice',
  lineno: 6,
  colno: 32,
  in_app: true,
  pre_context: ['const f = new Intl.NumberFormat()', '', 'export function formatPrice(item) {'],
  context_line: '  return f.format(item.price.toFixed(2))',
  post_context: ['}'],
  vars: { item: { sku: 'A-1', price: '19.90' } },
  symbolicated: true,
  raw: { filename: '/assets/index-abc.js', function: 'p', lineno: 1, colno: 4242, in_app: true },
}
const caller: Frame = {
  filename: './src/components/Cart.tsx',
  function: 'CartLine',
  lineno: 21,
  in_app: true,
  context_line: '<span>{formatPrice(item)}</span>',
}
const ex: ExceptionValue = {
  type: 'TypeError',
  value: 'item.price.toFixed is not a function',
  mechanism: { type: 'onerror', handled: false },
  stacktrace: { frames: [lib(1), lib(2), lib(3), caller, crash] },
}
const opts: StacktraceOptions = { inAppOnly: true, newestFirst: true, view: 'symbolicated', expandAll: false }

describe('ExceptionBlock', () => {
  it('shows the exception header with mechanism and handled state', async () => {
    renderUi(<ExceptionBlock ex={ex} index={0} platform="javascript" opts={opts} />)
    expect(await screen.findByRole('heading', { name: 'TypeError' })).toBeInTheDocument()
    expect(screen.getByText('item.price.toFixed is not a function')).toBeInTheDocument()
    expect(screen.getByText('unhandled')).toBeInTheDocument()
    expect(screen.getByText('mechanism: onerror')).toBeInTheDocument()
  })

  it('hides library frames behind an expander when in-app only is on', async () => {
    renderUi(<ExceptionBlock ex={ex} index={0} platform="javascript" opts={opts} />)
    await screen.findByText('./src/lib/money.ts')
    expect(screen.queryByText('node_modules/lib/file1.js')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /3 library frames hidden/ }))
    expect(screen.getByText('node_modules/lib/file1.js')).toBeInTheDocument()
  })

  it('expands the crashing frame with source context, highlighted line and variables', async () => {
    renderUi(<ExceptionBlock ex={ex} index={0} platform="javascript" opts={opts} />)
    const current = await screen.findByRole('row', { current: true })
    expect(within(current).getByText('6')).toBeInTheDocument()
    expect(current).toHaveTextContent('return f.format(item.price.toFixed(2))')
    expect(screen.getByText('crashed here')).toBeInTheDocument()
    expect(screen.getByText('source mapped')).toBeInTheDocument()
    expect(screen.getByRole('rowheader', { name: 'item' })).toBeInTheDocument()
    // the caller frame is collapsed
    expect(screen.getByRole('button', { name: 'Expand frame ./src/components/Cart.tsx' })).toHaveAttribute('aria-expanded', 'false')
  })

  it('orders frames newest first or oldest first', async () => {
    const names = () => screen.getAllByRole('listitem').map((li) => li.textContent ?? '')
    const first = renderUi(<ExceptionBlock ex={ex} index={0} platform="javascript" opts={{ ...opts, inAppOnly: false }} />)
    await screen.findByText('./src/lib/money.ts')
    expect(names()[0]).toContain('money.ts')
    first.unmount()
    renderUi(<ExceptionBlock ex={ex} index={0} platform="javascript" opts={{ ...opts, inAppOnly: false, newestFirst: false }} />)
    await screen.findByText('./src/lib/money.ts')
    expect(names()[0]).toContain('file1.js')
    expect(names().at(-1)).toContain('money.ts')
  })

  it('shows the minified location in minified view', async () => {
    renderUi(<ExceptionBlock ex={ex} index={0} platform="javascript" opts={{ ...opts, view: 'minified' }} />)
    expect(await screen.findByText('/assets/index-abc.js')).toBeInTheDocument()
    expect(screen.queryByText('source mapped')).not.toBeInTheDocument()
  })
})

describe('raw stack traces', () => {
  it('uses the minified frame for the minified view', () => {
    expect(displayFrame(crash, 'minified')).toMatchObject({ filename: '/assets/index-abc.js', colno: 4242 })
    expect(displayFrame(crash, 'symbolicated')).toBe(crash)
  })

  it('renders platform-style text', () => {
    const js = rawStacktrace([ex], 'javascript')
    expect(js.split('\n')[0]).toBe('TypeError: item.price.toFixed is not a function')
    expect(js.split('\n')[1]).toBe('    at formatPrice (webpack:///src/lib/money.ts:6:32)')
    const py = rawStacktrace(
      [
        {
          type: 'KeyError',
          value: "'id'",
          stacktrace: { frames: [{ filename: 'app.py', abs_path: '/app/app.py', function: 'view', lineno: 3, context_line: 'x = d["id"]' }] },
        },
      ],
      'python',
    )
    expect(py).toBe(`Traceback (most recent call last):\n  File "/app/app.py", line 3, in view\n    x = d["id"]\nKeyError: 'id'`)
  })
})
