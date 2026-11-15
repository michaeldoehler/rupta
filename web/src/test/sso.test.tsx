import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import type { SsoConfig } from '@/api/types'
import { getWorld, resetMock } from '@/mocks/handlers'
import { renderApp } from './render'

const orgProvider: SsoConfig = {
  enabled: true,
  provider_id: 'org-acme',
  provider_name: 'Acme Keycloak',
  issuer: 'https://keycloak.acme.dev/realms/acme',
  client_id: 'rupta',
  has_client_secret: true,
  scopes: 'openid email profile',
  role_claim: 'groups',
  role_mapping: { admins: 'admin' },
  default_role: 'member',
  jit_provisioning: true,
  enforce: false,
  owner_local_login: true,
  redirect_uri: 'https://errors.acme.dev/auth/sso/org-acme/callback',
}

function withoutInstance() {
  getWorld().instanceSso = { local_login: true, provider: null }
}

const OPTIONAL = 'Use a different identity provider for this organization (optional)'

describe('single sign-on settings', () => {
  it('shows the instance provider read-only and keeps the organization form collapsed', async () => {
    const user = userEvent.setup()
    renderApp('/acme/settings/sso')
    const card = await screen.findByRole('region', { name: 'Single sign-on for this instance' })
    expect(within(card).getByText('Configured by environment variables of the deployment.')).toBeInTheDocument()
    expect(within(card).getByText('https://auth.acme.dev')).toBeInTheDocument()
    expect(within(card).getByText('urn:zitadel:iam:org:project:roles')).toBeInTheDocument()
    expect(within(card).getByText('Set')).toBeInTheDocument()
    expect(within(card).getByText('https://errors.acme.dev/auth/sso/oidc/callback')).toBeInTheDocument()
    expect(within(card).getByRole('link', { name: /Single sign-on documentation/ })).toHaveAttribute('href', expect.stringContaining('docs/sso.md'))
    expect(screen.queryByText('Not configured')).not.toBeInTheDocument()
    expect(screen.getByText(OPTIONAL)).toBeInTheDocument()
    expect(screen.queryByLabelText('Issuer URL')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: /Set up organization provider/ }))
    expect(await screen.findByLabelText('Issuer URL')).toBeInTheDocument()
  })

  it('shows the organization form directly without an instance provider', async () => {
    withoutInstance()
    renderApp('/acme/settings/sso')
    expect(await screen.findByLabelText('Issuer URL')).toBeInTheDocument()
    expect(screen.getByText('Not configured')).toBeInTheDocument()
    expect(screen.queryByRole('region', { name: 'Single sign-on for this instance' })).not.toBeInTheDocument()
    expect(screen.queryByText(OPTIONAL)).not.toBeInTheDocument()
  })

  it('shows an organization provider without an instance provider', async () => {
    withoutInstance()
    getWorld().sso.set('1', orgProvider)
    renderApp('/acme/settings/sso')
    expect(await screen.findByDisplayValue('https://keycloak.acme.dev/realms/acme')).toBeInTheDocument()
    expect(screen.getByText('Configured')).toBeInTheDocument()
    expect(screen.queryByText(OPTIONAL)).not.toBeInTheDocument()
  })

  it('shows both, with the organization provider expanded', async () => {
    getWorld().sso.set('1', orgProvider)
    renderApp('/acme/settings/sso')
    expect(await screen.findByRole('region', { name: 'Single sign-on for this instance' })).toBeInTheDocument()
    expect(screen.getByText(OPTIONAL)).toBeInTheDocument()
    expect(await screen.findByDisplayValue('https://keycloak.acme.dev/realms/acme')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Hide/ })).toBeInTheDocument()
  })

  it('saves the default role "none" and requires a role mapping for it', async () => {
    withoutInstance()
    const user = userEvent.setup()
    renderApp('/acme/settings/sso')
    await user.type(await screen.findByLabelText('Issuer URL'), 'https://idp.acme.dev')
    await user.type(screen.getByLabelText('Client ID'), 'rupta')
    await user.type(screen.getByLabelText('Client secret'), 'secret')
    await user.click(screen.getByRole('combobox', { name: 'Default role' }))
    await user.click(await screen.findByRole('option', { name: 'No access (refuse sign-in)' }))
    expect(screen.getByText(/Owners can always sign in/)).toBeInTheDocument()
    // The Zitadel preset brings a role claim and a mapping; remove the mapping.
    for (let i = 3; i >= 1; i--) await user.click(screen.getByRole('button', { name: `Remove mapping ${i}` }))
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(await screen.findByText('Add at least one mapping when users without a matching role are refused.')).toBeInTheDocument()
    expect(getWorld().sso.get('1')).toBeUndefined()
    await user.click(screen.getByRole('button', { name: 'Add mapping' }))
    await user.type(screen.getByLabelText('Token value 1'), 'rupta-member')
    await user.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(getWorld().sso.get('1')?.default_role).toBe('none'))
  })

  it('uses the Zitadel scope that matches the role claim', async () => {
    withoutInstance()
    renderApp('/acme/settings/sso')
    expect(await screen.findByDisplayValue('openid email profile urn:zitadel:iam:org:project:roles')).toBeInTheDocument()
    expect(screen.getByDisplayValue('urn:zitadel:iam:org:project:roles')).toBeInTheDocument()
  })
})

describe('sign-in errors', () => {
  it('explains a refused sign-in without a role', async () => {
    resetMock({ auth: { setupDone: true, userId: null } })
    renderApp('/login?error=sso_no_access')
    expect(await screen.findByText(/has no role that gives access to rupta/)).toBeInTheDocument()
  })
})
