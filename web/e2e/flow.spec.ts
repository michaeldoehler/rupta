import { expect, test } from '@playwright/test'

test('setup, sign out and in, triage an issue, read its details and change settings', async ({ page }) => {
  // First run: the setup wizard.
  await page.goto('/?mock=fresh')
  await expect(page).toHaveURL(/\/setup$/)
  await page.getByLabel('Name').fill('Ada Admin')
  await page.getByLabel('Email').fill('ada@example.com')
  await page.getByLabel('Password').fill('correct-horse-battery')
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByLabel('Organization name').fill('Initech')
  await page.getByRole('button', { name: 'Create account' }).click()
  await expect(page).toHaveURL(/\/acme\/issues/)
  await expect(page.getByRole('grid', { name: 'Issues' }).getByRole('row').first()).toBeVisible()

  // Sign out and back in.
  await page.getByRole('button', { name: /ada@example.com/ }).click()
  await page.getByRole('menuitem', { name: 'Sign out' }).click()
  await expect(page).toHaveURL(/\/login/)
  await page.getByLabel('Email').fill('ada@example.com')
  await page.getByLabel('Password').fill('wrong-password')
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByRole('alert')).toContainText('incorrect')
  await page.getByLabel('Password').fill('correct-horse-battery')
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page).toHaveURL(/\/acme\/issues/)

  // Triage: resolve the first issue from the keyboard.
  const rows = page.getByRole('grid', { name: 'Issues' }).getByRole('row')
  await expect(rows.first()).toBeVisible()
  const title = (await rows.first().locator('[data-issue-link]').textContent()) ?? ''
  await page.keyboard.press('j')
  await page.keyboard.press('e')
  await expect(page.getByText('Issue resolved')).toBeVisible()

  // Search with autocomplete.
  const search = page.getByRole('combobox', { name: 'Search issues' })
  await search.fill('')
  await search.pressSequentially('is:unresolved lev')
  await expect(page.getByRole('option', { name: /^level:/ })).toBeVisible()
  await page.keyboard.press('Tab')
  await search.pressSequentially('fat')
  await page.keyboard.press('Enter')
  await page.keyboard.press('Enter')
  await expect(page).toHaveURL(/level%3Afatal/)
  await expect(rows.first()).toBeVisible()

  // Open an issue and read its details.
  await rows.first().locator('[data-issue-link]').click()
  await expect(page).toHaveURL(/\/acme\/issues\/\d+/)
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible()
  await expect(page.locator('#exception')).toBeVisible()
  await expect(page.locator('#breadcrumbs')).toBeVisible()
  const previous = page.getByRole('button', { name: 'Previous event' })
  if (await previous.isEnabled()) {
    const before = await page.getByRole('button', { name: 'Copy event id' }).locator('..').textContent()
    await previous.click()
    await expect(page.getByRole('button', { name: 'Copy event id' }).locator('..')).not.toHaveText(before ?? '')
  }
  const sections = page.getByRole('navigation', { name: 'Issue sections' })
  await sections.getByRole('link', { name: 'Tags' }).click()
  await expect(page.getByRole('link', { name: 'environment' })).toBeVisible()
  await sections.getByRole('link', { name: /^Activity/ }).click()
  await page.getByLabel('Comment', { exact: true }).fill('Looking into this now.')
  await page.getByRole('button', { name: 'Comment' }).click()
  await expect(page.locator('ol').getByText('Looking into this now.')).toBeVisible()
  await expect(page.getByLabel('Comment', { exact: true })).toHaveValue('')
  expect(title.length).toBeGreaterThan(0)

  // Settings: DSN and single sign-on.
  await page.getByRole('banner', { name: 'Top bar' }).getByRole('link', { name: 'Settings' }).click()
  await page.getByRole('link', { name: 'web-app' }).click()
  await page.getByRole('link', { name: 'Client keys & SDK' }).click()
  await expect(page.getByText(/https:\/\/[0-9a-f]+@rupta\.acme\.dev\/1/).first()).toBeVisible()
  await page.getByRole('tab', { name: /Django/ }).click()
  await expect(page.getByText('pip install --upgrade "sentry-sdk[django]"')).toBeVisible()
  await page.getByRole('link', { name: 'Single sign-on' }).click()
  await page.getByRole('radio', { name: 'Zitadel' }).click()
  await page.getByLabel('Issuer URL').fill('https://acme.zitadel.cloud')
  await page.getByRole('button', { name: 'Test connection' }).click()
  await expect(page.getByText('Connection works')).toBeVisible()
})

test('command palette, shortcuts help and the top bar', async ({ page }) => {
  await page.goto('/acme/issues')
  await expect(page.getByRole('grid', { name: 'Issues' }).getByRole('row').first()).toBeVisible()
  await page.keyboard.press('Shift+?')
  await expect(page.getByRole('dialog', { name: 'Keyboard shortcuts' })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog')).toBeHidden()
  // One dark theme: no switch anywhere.
  await expect(page.locator('html')).toHaveClass(/dark/)
  await page.keyboard.press('ControlOrMeta+k')
  await page.getByPlaceholder(/Type a command/).fill('theme')
  await expect(page.getByRole('option', { name: /theme/i })).toHaveCount(0)
  await page.keyboard.press('Escape')
  // The Explore menu in the top bar reaches the telemetry pages.
  await page
    .getByRole('navigation', { name: 'Main' })
    .getByRole('button', { name: /Explore/ })
    .click()
  await page.getByRole('menuitem', { name: /Logs/ }).click()
  await expect(page).toHaveURL(/\/acme\/logs/)
  await expect(page.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: /Logs/ })).toHaveAttribute('aria-current', 'page')
  await page.keyboard.press('ControlOrMeta+k')
  await page.getByPlaceholder(/Type a command/).fill('releases')
  await page.keyboard.press('Enter')
  await expect(page).toHaveURL(/\/acme\/releases/)
})

test('works on a phone-sized screen', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/acme/issues')
  await expect(page.getByRole('grid', { name: 'Issues' }).getByRole('row').first()).toBeVisible()
  await page.getByRole('button', { name: 'Open navigation' }).click()
  await page.getByRole('link', { name: 'Crons' }).click()
  await expect(page).toHaveURL(/\/acme\/crons/)
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)
  expect(overflow).toBeLessThanOrEqual(0)
})
