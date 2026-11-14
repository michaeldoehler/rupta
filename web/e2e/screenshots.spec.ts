// Screenshots of the main screens in both themes, from the mock build.
//   pnpm screenshots                 -> docs/images (README set)
//   REVIEW_DIR=/path pnpm screenshots -> also a full review set of every screen
import { mkdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { expect, test, type Page } from '@playwright/test'

const DOCS = fileURLToPath(new URL('../../docs/images/', import.meta.url))
const REVIEW = process.env.REVIEW_DIR
const README_SHOTS = new Set(['issues', 'issue-detail', 'stacktrace', 'release-detail', 'project-keys', 'command-palette'])

type Shot = { n: string; screen: string; state: string; go: (page: Page) => Promise<void>; mobile?: boolean; readme?: string }

async function ready(page: Page) {
  await page.waitForLoadState('networkidle')
  await expect(page.locator('[aria-busy="true"]')).toHaveCount(0, { timeout: 10_000 })
  await page.waitForTimeout(400)
}

async function stream(page: Page, query = '') {
  await page.goto(`/acme/issues${query}`)
  await expect(page.getByRole('grid', { name: 'Issues' }).getByRole('row').first()).toBeVisible()
}

async function openIssue(page: Page, search = '"reading"') {
  await stream(page, `?query=${encodeURIComponent(`is:unresolved ${search}`)}`)
  await page.getByRole('grid', { name: 'Issues' }).getByRole('row').first().locator('[data-issue-link]').click()
  await expect(page.locator('#exception')).toBeVisible()
}

const shots: Shot[] = [
  { n: '01', screen: 'setup', state: 'wizard', go: async (p) => void (await p.goto('/?mock=fresh'), await p.getByLabel('Name').fill('Ada Admin')) },
  { n: '02', screen: 'login', state: 'local-and-sso', go: async (p) => void (await p.goto('/login?mock=signed-out')) },
  { n: '03', screen: 'issues', state: 'default', readme: 'issues', go: async (p) => void (await p.goto('/acme/issues?mock=demo'), await stream(p)) },
  {
    n: '04',
    screen: 'issues',
    state: 'search-autocomplete',
    go: async (p) => {
      await stream(p)
      const s = p.getByRole('combobox', { name: 'Search issues' })
      await s.click()
      await s.press('End')
      await s.pressSequentially(' browser:')
      await expect(p.getByRole('option').first()).toBeVisible()
    },
  },
  {
    n: '05',
    screen: 'issues',
    state: 'bulk-selection',
    go: async (p) => {
      await stream(p)
      const rows = p.getByRole('grid', { name: 'Issues' }).getByRole('row')
      for (const i of [0, 1, 3]) await rows.nth(i).getByRole('checkbox').click()
      await p.keyboard.press('j')
    },
  },
  {
    n: '06',
    screen: 'issues',
    state: 'empty',
    go: async (p) => void (await p.goto('/acme/issues?query=%22no%20such%20error%22'), await expect(p.getByText('No issues match')).toBeVisible()),
  },
  { n: '07', screen: 'issue-detail', state: 'top', readme: 'issue-detail', go: async (p) => openIssue(p) },
  {
    n: '08',
    screen: 'issue-detail',
    state: 'stacktrace-source-context',
    readme: 'stacktrace',
    go: async (p) => {
      await openIssue(p)
      await p.locator('#exception').scrollIntoViewIfNeeded()
      await p.evaluate(() => document.getElementById('exception')?.scrollIntoView({ block: 'start' }))
    },
  },
  {
    n: '09',
    screen: 'issue-detail',
    state: 'chained-exceptions',
    go: async (p) => {
      await openIssue(p, 'IntegrityError')
      await p.evaluate(() => document.getElementById('exception')?.scrollIntoView({ block: 'start' }))
    },
  },
  {
    n: '10',
    screen: 'issue-detail',
    state: 'breadcrumbs',
    go: async (p) => void (await openIssue(p), await p.evaluate(() => document.getElementById('breadcrumbs')?.scrollIntoView({ block: 'start' }))),
  },
  {
    n: '11',
    screen: 'issue-detail',
    state: 'request',
    go: async (p) => void (await openIssue(p, 'KeyError'), await p.evaluate(() => document.getElementById('request')?.scrollIntoView({ block: 'start' }))),
  },
  {
    n: '12',
    screen: 'issue-detail',
    state: 'contexts',
    go: async (p) => void (await openIssue(p), await p.evaluate(() => document.getElementById('contexts')?.scrollIntoView({ block: 'start' }))),
  },
  {
    n: '13',
    screen: 'issue-detail',
    state: 'tags-distribution',
    go: async (p) =>
      void (
        await openIssue(p),
        await p.getByRole('navigation', { name: 'Issue sections' }).getByRole('link', { name: 'Tags' }).click(),
        await expect(p.getByRole('link', { name: 'environment' })).toBeVisible()
      ),
  },
  {
    n: '14',
    screen: 'issue-detail',
    state: 'activity-comments',
    go: async (p) => {
      await stream(p, '?sort=last_seen')
      await p.getByRole('grid', { name: 'Issues' }).getByRole('row').first().locator('[data-issue-link]').click()
      await p
        .getByRole('navigation', { name: 'Issue sections' })
        .getByRole('link', { name: /^Activity/ })
        .click()
      await p.getByLabel('Comment', { exact: true }).fill('Reproduced on **Safari 18**. The fix is in `useProducts`; deploying with the next release.')
    },
  },
  {
    n: '15',
    screen: 'issue-detail',
    state: 'events-tab',
    go: async (p) =>
      void (
        await openIssue(p),
        await p
          .getByRole('navigation', { name: 'Issue sections' })
          .getByRole('link', { name: /^Events/ })
          .click(),
        await expect(p.getByRole('columnheader', { name: 'Environment' })).toBeVisible()
      ),
  },
  {
    n: '16',
    screen: 'issue-detail',
    state: 'raw-json',
    go: async (p) =>
      void (await openIssue(p), await p.getByRole('button', { name: 'View event JSON' }).click(), await expect(p.getByRole('dialog')).toBeVisible()),
  },
  { n: '17', screen: 'releases', state: 'list', go: async (p) => void (await p.goto('/acme/releases')) },
  {
    n: '18',
    screen: 'release-detail',
    state: 'health',
    readme: 'release-detail',
    go: async (p) =>
      void (
        await p.goto('/acme/releases'),
        await p
          .getByRole('link', { name: /1\.\d+\.\d+/ })
          .first()
          .click(),
        await expect(p.getByText('Sessions', { exact: true })).toBeVisible()
      ),
  },
  { n: '19', screen: 'crons', state: 'list', go: async (p) => void (await p.goto('/acme/crons')) },
  { n: '20', screen: 'crons', state: 'monitor-detail', go: async (p) => void (await p.goto('/acme/crons/api/reindex-search')) },
  { n: '21', screen: 'feedback', state: 'list', go: async (p) => void (await p.goto('/acme/feedback')) },
  { n: '22', screen: 'alerts', state: 'list', go: async (p) => void (await p.goto('/acme/alerts')) },
  {
    n: '23',
    screen: 'alerts',
    state: 'rule-editor',
    go: async (p) =>
      void (
        await p.goto('/acme/alerts'),
        await p.getByRole('link', { name: 'Error spike' }).click(),
        await expect(p.getByLabel('Name')).toHaveValue('Error spike')
      ),
  },
  {
    n: '24',
    screen: 'project-settings',
    state: 'keys-sdk',
    readme: 'project-keys',
    go: async (p) => void (await p.goto('/acme/settings/projects/web-app/keys'), await p.getByText('Set up the SDK').scrollIntoViewIfNeeded()),
  },
  { n: '25', screen: 'project-settings', state: 'inbound-filters', go: async (p) => void (await p.goto('/acme/settings/projects/web-app/filters')) },
  { n: '26', screen: 'project-settings', state: 'data-scrubbing', go: async (p) => void (await p.goto('/acme/settings/projects/billing/scrubbing')) },
  { n: '27', screen: 'project-settings', state: 'source-maps', go: async (p) => void (await p.goto('/acme/settings/projects/web-app/source-maps')) },
  { n: '28', screen: 'org-settings', state: 'members', go: async (p) => void (await p.goto('/acme/settings/members')) },
  { n: '29', screen: 'org-settings', state: 'teams', go: async (p) => void (await p.goto('/acme/settings/teams')) },
  {
    n: '30',
    screen: 'org-settings',
    state: 'sso-zitadel',
    go: async (p) => void (await p.goto('/acme/settings/sso'), await p.getByLabel('Issuer URL').fill('https://acme.zitadel.cloud')),
  },
  {
    n: '31',
    screen: 'org-settings',
    state: 'token-created',
    go: async (p) => {
      await p.goto('/acme/settings/tokens')
      await p.getByRole('button', { name: 'Create token' }).first().click()
      await p.getByLabel('Name').fill('CI source map upload')
      await p.getByRole('dialog').getByRole('button', { name: 'Create token' }).click()
      await expect(p.getByText('Token created')).toBeVisible()
    },
  },
  { n: '32', screen: 'org-settings', state: 'audit-log', go: async (p) => void (await p.goto('/acme/settings/audit-log')) },
  { n: '33', screen: 'org-settings', state: 'channels', go: async (p) => void (await p.goto('/acme/settings/channels')) },
  { n: '34', screen: 'user-settings', state: 'profile', go: async (p) => void (await p.goto('/acme/settings/account')) },
  {
    n: '35',
    screen: 'command-palette',
    state: 'open',
    readme: 'command-palette',
    go: async (p) => void (await stream(p), await p.keyboard.press('ControlOrMeta+k'), await p.getByPlaceholder(/Type a command/).fill('fail')),
  },
  {
    n: '36',
    screen: 'keyboard-help',
    state: 'open',
    go: async (p) => void (await stream(p), await p.keyboard.press('Shift+?'), await expect(p.getByRole('dialog')).toBeVisible()),
  },
  { n: '37', screen: 'mobile-issues', state: 'default', mobile: true, go: async (p) => stream(p) },
  { n: '38', screen: 'mobile-issue-detail', state: 'top', mobile: true, go: async (p) => openIssue(p) },
  {
    n: '39',
    screen: 'mobile-navigation',
    state: 'open',
    mobile: true,
    go: async (p) => void (await stream(p), await p.getByRole('button', { name: 'Open navigation' }).click()),
  },
  { n: '40', screen: 'mobile-settings', state: 'members', mobile: true, go: async (p) => void (await p.goto('/acme/settings/members')) },
]

for (const theme of ['light', 'dark'] as const) {
  test(`screenshots (${theme})`, async ({ browser }) => {
    test.setTimeout(600_000)
    mkdirSync(DOCS, { recursive: true })
    if (REVIEW) mkdirSync(REVIEW, { recursive: true })
    for (const s of shots) {
      if (!REVIEW && !s.readme) continue
      const ctx = await browser.newContext({
        colorScheme: theme,
        deviceScaleFactor: 2,
        viewport: s.mobile ? { width: 390, height: 844 } : { width: 1440, height: 900 },
        isMobile: s.mobile,
        hasTouch: s.mobile,
      })
      const page = await ctx.newPage()
      // start from the demo data, signed in
      if (s.n !== '01' && s.n !== '02') {
        await page.goto('/acme/issues?mock=demo&mock_latency=0')
        await expect(page.getByRole('grid', { name: 'Issues' }).getByRole('row').first()).toBeVisible()
      }
      await s.go(page)
      await ready(page)
      const name = `${s.n}-${s.screen}-${s.state}-${theme}.png`
      if (REVIEW) await page.screenshot({ path: `${REVIEW}/${name}` })
      if (s.readme && README_SHOTS.has(s.readme)) await page.screenshot({ path: `${DOCS}${s.readme}-${theme}.png` })
      await ctx.close()
    }
  })
}
