import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import * as marks from '@/components/brand/platform-marks'
import { PlatformIcon, platformLabel } from './platform-icon'

describe('PlatformIcon', () => {
  it.each([
    ['javascript-react', 'React', { ...marks.react, hex: '#087EA4' }],
    ['php-laravel', 'Laravel', marks.laravel],
    ['node', 'Node.js', marks.nodejs],
    ['java', 'Java', marks.openjdk],
  ])('draws the mark of %s', (platform, label, mark) => {
    const { container } = render(<PlatformIcon platform={platform} />)
    const tile = container.firstElementChild!
    expect(tile).toHaveAttribute('title', label)
    expect(tile).toHaveAttribute('aria-hidden', 'true')
    expect(tile.querySelector('path')).toHaveAttribute('d', mark.path)
    expect(tile.querySelector('svg')).toHaveAttribute('fill', mark.hex)
  })

  it('falls back to the base platform, and to a letter for unknown ones', () => {
    const { container, rerender } = render(<PlatformIcon platform="python-tornado" />)
    expect(container.querySelector('path')).toHaveAttribute('d', marks.python.path)
    rerender(<PlatformIcon platform="elixir" />)
    expect(container.querySelector('svg')).toBeNull()
    expect(container.firstElementChild).toHaveTextContent('E')
    rerender(<PlatformIcon platform={null} />)
    expect(container.firstElementChild).toHaveAttribute('title', 'Other')
  })

  it('names platforms', () => {
    expect(platformLabel('javascript-nextjs')).toBe('Next.js')
    expect(platformLabel('python-tornado')).toBe('Python')
    expect(platformLabel('elixir')).toBe('elixir')
  })
})
