import { slack as slackMark, teams as teamsMark } from './platform-marks'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { describe, expect, it } from 'vitest'
import { BrandIcon } from './brand-icon'
import { BrandPicker } from './brand-picker'
import { BRANDS, isZitadel, type BrandKind } from './brands'
import { github, jira, pagerduty } from './marks'
import { SsoProviderIcon } from './sso-icon'

describe('BrandIcon', () => {
  it.each<[BrandKind, string, string | null]>([
    ['jira_cloud', 'Jira Cloud', 'Jira'],
    ['jira_server', 'Jira Data Center', 'Jira'],
    ['linear', 'Linear', 'Linear'],
    ['github', 'GitHub', 'GitHub'],
    ['gitlab', 'GitLab', 'GitLab'],
    ['pagerduty', 'PagerDuty', 'PagerDuty'],
    ['opsgenie', 'Opsgenie', 'Opsgenie'],
    ['slack', 'Slack', 'Slack'],
    ['teams', 'Microsoft Teams', 'Microsoft Teams'],
    ['webhook', 'Webhook', null],
    ['email', 'Email', null],
  ])('draws %s with its name', (kind, name, mark) => {
    render(<BrandIcon kind={kind} />)
    const tile = screen.getByRole('img', { name })
    expect(tile).toHaveAttribute('title', name)
    const glyph = tile.querySelector('[data-mark]')
    if (mark) expect(glyph).toHaveAttribute('data-mark', mark)
    else expect(glyph).toBeNull()
  })

  it('draws the simple-icons path in the brand colour on a light tile', () => {
    render(<BrandIcon kind="github" />)
    const tile = screen.getByRole('img', { name: 'GitHub' })
    expect(tile).toHaveAttribute('data-brand-tile', 'light')
    expect(tile.querySelector('svg')).toHaveAttribute('fill', github.hex)
    expect(tile.querySelector('path')).toHaveAttribute('d', github.path)
  })

  it('draws every service mark in its brand colour on a light tile', () => {
    render(<BrandIcon kind="pagerduty" />)
    const tile = screen.getByRole('img', { name: 'PagerDuty' })
    expect(tile).toHaveAttribute('data-brand-tile', 'light')
    expect(tile.querySelector('svg')).toHaveAttribute('fill', pagerduty.hex)
    expect(tile.querySelector('path')).toHaveAttribute('d', pagerduty.path)
  })

  it('uses one Jira mark for Cloud and Data Center', () => {
    render(
      <>
        <BrandIcon kind="jira_cloud" />
        <BrandIcon kind="jira_server" />
      </>,
    )
    const paths = screen.getAllByRole('img').map((t) => t.querySelector('path')?.getAttribute('d'))
    expect(paths).toEqual([jira.path, jira.path])
  })

  it('draws the Slack and Microsoft Teams marks', () => {
    render(
      <>
        <BrandIcon kind="slack" />
        <BrandIcon kind="teams" />
      </>,
    )
    const slack = screen.getByRole('img', { name: 'Slack' })
    expect(slack.querySelector('[data-monogram]')).toBeNull()
    expect(slack.querySelector('path')).toHaveAttribute('d', slackMark.path)
    expect(slack).toHaveAttribute('data-brand-tile', 'light')
    expect(slack.querySelector('svg')).toHaveAttribute('fill', slackMark.hex)
    expect(screen.getByRole('img', { name: 'Microsoft Teams' }).querySelector('path')).toHaveAttribute('d', teamsMark.path)
  })

  it('falls back to a generic icon named after an unknown kind', () => {
    render(
      <>
        <BrandIcon kind="carrier_pigeon" />
        <BrandIcon kind={null} />
      </>,
    )
    expect(screen.getByRole('img', { name: 'carrier pigeon' })).toHaveAttribute('data-brand-tile', 'neutral')
    expect(screen.getByRole('img', { name: 'Integration' })).toHaveAttribute('data-brand-tile', 'neutral')
    // Inherited object keys are not kinds.
    render(<BrandIcon kind="toString" />)
    expect(screen.getByRole('img', { name: 'toString' })).toHaveAttribute('data-brand-tile', 'neutral')
  })

  it('takes another accessible name, or none next to text that names the service', () => {
    render(
      <>
        <BrandIcon kind="linear" label="Linear (product)" />
        <BrandIcon kind="gitlab" decorative />
      </>,
    )
    expect(screen.getByRole('img', { name: 'Linear (product)' })).toBeInTheDocument()
    expect(screen.queryByRole('img', { name: 'GitLab' })).toBeNull()
    expect(screen.getByTitle('GitLab')).toHaveAttribute('aria-hidden', 'true')
  })

  it('has a brand for every kind it names', () => {
    for (const b of Object.values(BRANDS)) expect(b.label).not.toBe('')
  })
})

describe('SsoProviderIcon', () => {
  it('shows the ZITADEL mark for a provider named Zitadel in any case', () => {
    expect(isZitadel(' ZITADEL ')).toBe(true)
    expect(isZitadel('Zitadel Cloud')).toBe(false)
    render(<SsoProviderIcon name="zitadel" />)
    const tile = screen.getByRole('img', { name: 'ZITADEL' })
    expect(tile.querySelectorAll('linearGradient')).toHaveLength(6)
  })

  it('keeps gradient ids unique when the mark appears twice', () => {
    const { container } = render(
      <>
        <SsoProviderIcon name="Zitadel" />
        <SsoProviderIcon name="Zitadel" />
      </>,
    )
    const ids = [...container.querySelectorAll('linearGradient')].map((g) => g.id)
    expect(new Set(ids).size).toBe(12)
  })

  it('shows a key for other providers', () => {
    render(<SsoProviderIcon name="Keycloak" />)
    const tile = screen.getByRole('img', { name: 'Keycloak' })
    expect(tile).toHaveAttribute('data-brand-tile', 'neutral')
    expect(tile.querySelector('linearGradient')).toBeNull()
  })
})

describe('BrandPicker', () => {
  function Picker() {
    const [v, setV] = useState<BrandKind>('slack')
    return (
      <BrandPicker
        label="Channel type"
        value={v}
        onChange={setV}
        options={(['slack', 'teams', 'email'] as const).map((k) => ({
          value: k,
          label: BRANDS[k].label,
          description: `About ${k}`,
          icon: <BrandIcon kind={k} decorative />,
        }))}
      />
    )
  }

  it('is a radio group of named, described options moved with the arrow keys', async () => {
    const user = userEvent.setup()
    render(<Picker />)
    expect(screen.getByRole('radiogroup', { name: 'Channel type' })).toBeInTheDocument()
    const slack = screen.getByRole('radio', { name: 'Slack' })
    expect(slack).toBeChecked()
    expect(slack).toHaveAccessibleDescription('About slack')
    // Only the selected tile is in the tab order.
    await user.tab()
    expect(slack).toHaveFocus()
    await user.keyboard('{ArrowRight}')
    expect(screen.getByRole('radio', { name: /Microsoft Teams/ })).toBeChecked()
    expect(screen.getByRole('radio', { name: /Microsoft Teams/ })).toHaveFocus()
    await user.keyboard('{ArrowLeft}{ArrowLeft}')
    expect(screen.getByRole('radio', { name: /Email/ })).toBeChecked()
    await user.keyboard('{Home}')
    expect(slack).toBeChecked()
    await user.click(screen.getByRole('radio', { name: /Email/ }))
    expect(screen.getByRole('radio', { name: /Email/ })).toBeChecked()
  })
})
