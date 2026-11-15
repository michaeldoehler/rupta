// Screenshots of the screens (one dark theme), from the mock build.
//   pnpm screenshots                  -> docs/images (README set)
//   REVIEW_DIR=/path pnpm screenshots -> also a review set of every screen, with index.md
//   REVIEW_WAVE=2 REVIEW_DIR=/path    -> only the telemetry screens
//   REVIEW_WAVE=3 REVIEW_DIR=/path    -> only the integration screens
//   REVIEW_WAVE=3,4 REVIEW_DIR=/path  -> the integration screens and the service marks
import { mkdirSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { expect, test, type Page } from '@playwright/test'

const DOCS = fileURLToPath(new URL('../../docs/images/', import.meta.url))
const REVIEW = process.env.REVIEW_DIR
const README_SHOTS = new Set([
  'issues',
  'issue-detail',
  'stacktrace',
  'release-detail',
  'project-keys',
  'command-palette',
  'trace',
  'logs',
  'flamegraph',
  'replay',
  'linked-tickets',
])
const WAVES = process.env.REVIEW_WAVE ? new Set(process.env.REVIEW_WAVE.split(',')) : null
// SHOTS=45,46 retakes only these numbers (the index still lists every shot).
const ONLY = process.env.SHOTS ? new Set(process.env.SHOTS.split(',')) : null

type Shot = {
  n: string
  screen: string
  state: string
  go: (page: Page) => Promise<void>
  mobile?: boolean
  wide?: boolean
  readme?: string
  wave?: 2 | 3 | 4
  /** Taken signed out (no demo sign-in first). */
  signedOut?: boolean
  note?: string
  streaming?: boolean
}

async function ready(page: Page, streaming = false) {
  // A live tail keeps a request open, so the network never goes idle; a
  // replay player can keep it busy too, so the wait is bounded.
  if (!streaming) await page.waitForLoadState('networkidle', { timeout: 8000 }).catch(() => undefined)
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

/** The busiest unresolved issue; the mock links tickets to it. */
async function openLinked(page: Page) {
  await stream(page, '?sort=freq')
  await page.getByRole('grid', { name: 'Issues' }).getByRole('row').first().locator('[data-issue-link]').click()
  await expect(page.getByTestId('linked-tickets').getByRole('listitem').first()).toBeVisible()
}

async function connectDialog(page: Page) {
  await page.goto('/acme/settings/integrations')
  await page.getByRole('button', { name: 'Connect tracker' }).click()
  const d = page.getByRole('dialog', { name: 'Connect an issue tracker' })
  await d.getByLabel('Site URL').fill('https://ops.atlassian.net')
  await d.getByLabel('Account email').fill('rupta-bot@acme.dev')
  return d
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
        await expect(p.getByRole('heading', { name: 'Sessions' })).toBeVisible()
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
    go: async (p) => {
      await p.goto('/acme/settings/sso')
      await p.getByRole('button', { name: /organization provider/ }).click()
      await p.getByLabel('Issuer URL').fill('https://acme.zitadel.cloud')
    },
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

// ---- telemetry (wave 2) ------------------------------------------------------

async function grid(page: Page, name: string) {
  await expect(page.getByRole('grid', { name }).getByRole('row').first()).toBeVisible()
}

async function openTrace(page: Page) {
  await page.goto('/acme/performance/summary?name=%2Fcheckout')
  await page.getByRole('table', { name: 'Slowest samples' }).getByRole('link').first().click()
  await expect(page.getByRole('treegrid', { name: 'Spans' }).getByRole('row').first()).toBeVisible()
}

async function openProfile(page: Page) {
  await page.goto('/acme/profiles')
  await page
    .getByRole('grid', { name: 'Profiles' })
    .getByRole('link', { name: /GET \/api\/products api/ })
    .first()
    .click()
  await expect(page.getByTestId('flamegraph')).toBeVisible()
}

async function openReplay(page: Page) {
  await page.goto('/acme/replays')
  await page.getByRole('grid', { name: 'Replays' }).getByRole('link').first().click()
  await expect(page.getByTestId('replay-player').locator('iframe')).toBeAttached()
}

shots.push(
  {
    n: '41',
    wave: 2,
    screen: 'performance',
    state: 'transactions',
    note: 'Transactions table with throughput, percentiles, failure rate and apdex',
    go: async (p) => void (await p.goto('/acme/performance'), await expect(p.getByRole('table', { name: 'Transactions' })).toBeVisible()),
  },
  {
    n: '42',
    wave: 2,
    screen: 'performance',
    state: 'empty-search',
    note: 'No transactions match the search',
    go: async (p) =>
      void (
        await p.goto('/acme/performance'),
        await p.getByLabel('Search transactions').fill('nothing-like-this'),
        await expect(p.getByText('No transactions match')).toBeVisible()
      ),
  },
  {
    n: '43',
    wave: 2,
    screen: 'transaction-summary',
    state: 'browser-vitals',
    note: 'Summary of a page load: charts, web vitals against thresholds',
    go: async (p) => void (await p.goto('/acme/performance/summary?name=%2Fproducts%2F%3Aslug'), await expect(p.getByTestId('vital-lcp')).toBeVisible()),
  },
  {
    n: '44',
    wave: 2,
    screen: 'transaction-summary',
    state: 'histogram-samples',
    note: 'Duration histogram, related issues and slowest samples of an API endpoint',
    go: async (p) =>
      void (
        await p.goto('/acme/performance/summary?name=POST%20%2Fapi%2Forders'),
        await expect(p.getByRole('table', { name: 'Slowest samples' })).toBeVisible(),
        await p.getByRole('img', { name: 'Duration histogram' }).scrollIntoViewIfNeeded()
      ),
  },
  {
    n: '45',
    wave: 2,
    screen: 'trace',
    state: 'waterfall',
    readme: 'trace',
    note: 'Trace across browser, API and worker with error and log markers',
    go: openTrace,
  },
  {
    n: '46',
    wave: 2,
    screen: 'trace',
    state: 'span-details',
    note: 'Span details panel with errors, logs, data',
    go: async (p) =>
      void (
        await openTrace(p),
        await p.getByRole('treegrid', { name: 'Spans' }).getByRole('row').nth(3).click(),
        await expect(p.getByTestId('span-drawer')).toBeVisible()
      ),
  },
  {
    n: '47',
    wave: 2,
    screen: 'trace',
    state: 'critical-path-zoomed',
    note: 'Critical path highlighted, timeline zoomed with the minimap window',
    go: async (p) =>
      void (await openTrace(p), await p.getByRole('button', { name: 'Critical path' }).click(), await p.getByRole('button', { name: 'Zoom in' }).click()),
  },
  {
    n: '48',
    wave: 2,
    screen: 'trace',
    state: 'long-trace-collapsed',
    note: 'A batch job with 1,300 spans, collapsed below the transactions',
    go: async (p) =>
      void (
        await p.goto('/acme/performance/summary?name=reindex-search'),
        await p.getByRole('table', { name: 'Slowest samples' }).getByRole('link').first().click(),
        await expect(p.getByRole('treegrid', { name: 'Spans' }).getByRole('row').first()).toBeVisible()
      ),
  },
  {
    n: '49',
    wave: 2,
    screen: 'trace',
    state: 'not-found',
    note: 'A trace id with nothing stored',
    go: async (p) => void (await p.goto('/acme/traces/00000000000000000000000000000000'), await expect(p.getByText('Not found')).toBeVisible()),
  },
  {
    n: '50',
    wave: 2,
    screen: 'span-explorer',
    state: 'by-operation',
    note: 'Span explorer grouped by operation',
    go: async (p) => void (await p.goto('/acme/traces'), await expect(p.getByRole('table', { name: 'Span groups' })).toBeVisible()),
  },
  {
    n: '51',
    wave: 2,
    screen: 'span-explorer',
    state: 'spans',
    note: 'Individual spans (group_by=none) linking to their traces',
    go: async (p) => void (await p.goto('/acme/traces?group_by=none&q=op%3Adb.query'), await expect(p.getByRole('table', { name: 'Spans' })).toBeVisible()),
  },
  {
    n: '52',
    wave: 2,
    screen: 'logs',
    state: 'explorer',
    readme: 'logs',
    note: 'Logs: level histogram, facets, virtualised table',
    go: async (p) => void (await p.goto('/acme/logs?period=24h'), await grid(p, 'Log lines')),
  },
  {
    n: '53',
    wave: 2,
    screen: 'logs',
    state: 'autocomplete',
    note: 'Query bar suggesting attribute keys',
    go: async (p) =>
      void (
        await p.goto('/acme/logs?period=24h'),
        await grid(p, 'Log lines'),
        await p.getByRole('combobox', { name: 'Search logs' }).click(),
        await p.getByRole('combobox', { name: 'Search logs' }).pressSequentially('http.'),
        await expect(p.getByRole('option').first()).toBeVisible()
      ),
  },
  {
    n: '54',
    wave: 2,
    screen: 'logs',
    state: 'expanded-row',
    note: 'Expanded line with all attributes, trace link and issues of the trace',
    go: async (p) =>
      void (
        await p.goto('/acme/logs?period=24h&query=level%3Awarn%20service.name%3Aapi'),
        await grid(p, 'Log lines'),
        await p
          .getByRole('grid', { name: 'Log lines' })
          .getByRole('row')
          .filter({ has: p.getByRole('link', { name: 'Open trace' }) })
          .first()
          .click(),
        await expect(p.getByText('View trace')).toBeVisible()
      ),
  },
  {
    n: '55',
    wave: 2,
    streaming: true,
    screen: 'logs',
    state: 'live-tail',
    note: 'Live tail streaming new lines over Server-Sent Events',
    go: async (p) =>
      void (
        await p.goto('/acme/logs?period=1h&live=true'),
        await expect(p.getByRole('status').getByText('Live', { exact: true })).toBeVisible({ timeout: 10_000 }),
        await p.waitForTimeout(3000)
      ),
  },
  {
    n: '56',
    wave: 2,
    screen: 'logs',
    state: 'empty',
    note: 'No lines match',
    go: async (p) => void (await p.goto('/acme/logs?query=%22no%20such%20line%22'), await expect(p.getByText('No log lines match')).toBeVisible()),
  },
  {
    n: '57',
    wave: 2,
    screen: 'mobile-logs',
    state: 'default',
    mobile: true,
    note: 'Logs on a phone',
    go: async (p) => void (await p.goto('/acme/logs?period=24h'), await grid(p, 'Log lines')),
  },
  {
    n: '58',
    wave: 2,
    screen: 'metrics',
    state: 'explorer-grouped',
    note: 'Metric explorer: p95 by route, area chart, series table',
    go: async (p) =>
      void (
        await p.goto('/acme/metrics?name=http.server.duration&op=p95&group_by=route&chart=area'),
        await expect(p.getByRole('table', { name: 'Series summary' })).toBeVisible()
      ),
  },
  {
    n: '59',
    wave: 2,
    screen: 'metrics',
    state: 'counter-bars',
    note: 'A counter as bars',
    go: async (p) => void (await p.goto('/acme/metrics?name=checkout.completed'), await expect(p.getByRole('table', { name: 'Series summary' })).toBeVisible()),
  },
  {
    n: '60',
    wave: 2,
    screen: 'dashboards',
    state: 'list',
    note: 'Dashboards',
    go: async (p) => void (await p.goto('/acme/dashboards'), await expect(p.getByRole('link', { name: /Checkout health/ })).toBeVisible()),
  },
  {
    n: '61',
    wave: 2,
    screen: 'dashboard',
    state: 'grid',
    note: 'Dashboard with metric, transaction, log and issue widgets',
    go: async (p) => void (await p.goto('/acme/dashboards/1'), await expect(p.getByRole('list', { name: 'Widgets' })).toBeVisible()),
  },
  {
    n: '62',
    wave: 2,
    screen: 'dashboard',
    state: 'widget-editor',
    note: 'Add widget dialog with live preview',
    go: async (p) => {
      await p.goto('/acme/dashboards/2')
      await p.getByRole('button', { name: 'Add widget' }).first().click()
      await p.getByRole('dialog').getByLabel('Metric').click()
      await p.getByRole('option', { name: 'jobs.processed' }).click()
    },
  },
  {
    n: '63',
    wave: 2,
    screen: 'profiles',
    state: 'list',
    note: 'Profile list',
    go: async (p) => void (await p.goto('/acme/profiles'), await grid(p, 'Profiles')),
  },
  {
    n: '64',
    wave: 2,
    screen: 'profiles',
    state: 'by-transaction',
    note: 'Profiles grouped by transaction',
    go: async (p) =>
      void (await p.goto('/acme/profiles/transactions'), await expect(p.getByRole('link', { name: /GET \/api\/products/ }).first()).toBeVisible()),
  },
  { n: '65', wave: 2, screen: 'flamegraph', state: 'profile', readme: 'flamegraph', note: 'Flamegraph of a Python request, call tree', go: openProfile },
  {
    n: '66',
    wave: 2,
    screen: 'flamegraph',
    state: 'search-selected',
    note: 'Search highlighting and frame details',
    go: async (p) =>
      void (
        await openProfile(p),
        await p.getByLabel('Search frames').fill('psycopg'),
        await p.getByRole('treegrid', { name: 'Call tree' }).getByRole('row').nth(4).click()
      ),
  },
  {
    n: '67',
    wave: 2,
    screen: 'flamegraph',
    state: 'left-heavy-package',
    note: 'Left-heavy order, coloured by package, functions table',
    go: async (p) =>
      void (
        await openProfile(p),
        await p.getByRole('radio', { name: 'Left heavy' }).click(),
        await p.getByRole('radio', { name: 'Package' }).click(),
        await p.getByRole('tab', { name: 'Functions' }).click()
      ),
  },
  {
    n: '68',
    wave: 2,
    screen: 'flamegraph',
    state: 'aggregated',
    note: 'Aggregated flamegraph of a transaction',
    go: async (p) =>
      void (await p.goto('/acme/profiles/aggregate?transaction=GET%20%2Fapi%2Fproducts'), await expect(p.getByTestId('flamegraph')).toBeVisible()),
  },
  {
    n: '69',
    wave: 2,
    screen: 'replays',
    state: 'list',
    note: 'Session replays',
    go: async (p) => void (await p.goto('/acme/replays'), await grid(p, 'Replays')),
  },
  {
    n: '70',
    wave: 2,
    screen: 'mobile-replays',
    state: 'list',
    mobile: true,
    note: 'Replays on a phone',
    go: async (p) => void (await p.goto('/acme/replays'), await grid(p, 'Replays')),
  },
  {
    n: '71',
    wave: 2,
    screen: 'replay',
    state: 'playing',
    readme: 'replay',
    note: 'Player with timeline markers and breadcrumbs (a real rrweb recording)',
    go: async (p) =>
      void (await openReplay(p), await p.getByTestId('replay-player').getByRole('button', { name: 'Play' }).first().click(), await p.waitForTimeout(5500)),
  },
  {
    n: '72',
    wave: 2,
    screen: 'replay',
    state: 'network-panel',
    note: 'Network panel; the failed request at 0:15',
    go: async (p) =>
      void (
        await openReplay(p),
        await p.getByRole('tab', { name: /Network/ }).click(),
        await p.getByRole('tabpanel').getByRole('button').filter({ hasText: '502' }).click(),
        await p.waitForTimeout(1200)
      ),
  },
  {
    n: '73',
    wave: 2,
    screen: 'replay',
    state: 'console-errors',
    note: 'Console and errors panels',
    go: async (p) => void (await openReplay(p), await p.getByRole('tab', { name: /Console/ }).click()),
  },
  {
    n: '74',
    wave: 2,
    screen: 'issue-detail',
    state: 'trace-replay-links',
    note: 'Issue event linking to its trace and replay',
    go: async (p) => void (await openIssue(p), await expect(p.getByRole('link', { name: /^Trace [0-9a-f]{8}/ })).toBeVisible()),
  },
  {
    n: '75',
    wave: 2,
    screen: 'issue-detail',
    state: 'replays-tab',
    note: 'Replays tab of an issue',
    go: async (p) => {
      await stream(p, '?sort=freq&query=is%3Aunresolved%20ChunkLoadError')
      await p.getByRole('grid', { name: 'Issues' }).getByRole('row').first().locator('[data-issue-link]').click()
      await p.getByRole('navigation', { name: 'Issue sections' }).getByRole('link', { name: 'Replays' }).click()
      await expect(p.getByText(/sessions? with this error|No replays/)).toBeVisible()
    },
  },
  {
    n: '76',
    wave: 2,
    screen: 'project-settings',
    state: 'telemetry-data',
    note: 'Per-type retention, sampling and switches',
    go: async (p) =>
      void (await p.goto('/acme/settings/projects/web-app/telemetry'), await expect(p.getByRole('switch', { name: 'Accept logs' })).toBeVisible()),
  },
  {
    n: '79',
    screen: 'issues',
    state: 'split-preview',
    wide: true,
    note: 'Issue stream with the preview panel (1920x1080)',
    go: async (p) => {
      await stream(p)
      await p.evaluate(() => localStorage.setItem('rupta-issue-split', '1'))
      await p.reload()
      await expect(p.getByRole('grid', { name: 'Issues' }).getByRole('row').first()).toBeVisible()
      await p.keyboard.press('j')
      await p.keyboard.press('j')
      await expect(p.getByTestId('issue-preview').locator('#exception, #message').first()).toBeVisible()
    },
  },
  {
    n: '80',
    screen: 'issue-detail',
    state: 'context-panel-wide',
    wide: true,
    note: 'Issue details with the context panel (1920x1080)',
    go: async (p) => openIssue(p),
  },
  {
    n: '81',
    screen: 'top-bar',
    state: 'explore-menu',
    note: 'Top bar with the Explore menu open',
    go: async (p) =>
      void (
        await stream(p),
        await p
          .getByRole('navigation', { name: 'Main' })
          .getByRole('button', { name: /Explore/ })
          .click()
      ),
  },
  {
    n: '77',
    wave: 2,
    screen: 'navigation',
    state: 'palette-trace',
    note: 'Command palette: telemetry pages and opening a trace by id',
    go: async (p) =>
      void (await stream(p), await p.keyboard.press('ControlOrMeta+k'), await p.getByPlaceholder(/Type a command/).fill('0123456789abcdef0123456789abcdef')),
  },
  {
    n: '78',
    wave: 2,
    screen: 'keyboard-help',
    state: 'telemetry',
    note: 'Keyboard shortcuts including the telemetry pages',
    go: async (p) => void (await stream(p), await p.keyboard.press('Shift+?'), await expect(p.getByRole('dialog')).toBeVisible()),
  },
  {
    n: '82',
    wave: 3,
    screen: 'settings-integrations',
    state: 'list-tested',
    note: 'Organization integrations: last error of GitHub, Jira tested, Linear disabled',
    go: async (p) => {
      await p.goto('/acme/settings/integrations')
      const list = p.getByRole('list', { name: 'Integrations' })
      await list.getByRole('listitem', { name: 'Jira' }).getByRole('button', { name: 'Test' }).click()
      await expect(list.getByRole('listitem', { name: 'Jira' }).getByRole('status')).toBeVisible()
      await list.getByRole('listitem', { name: 'GitHub' }).getByRole('button', { name: 'Test' }).click()
      await expect(list.getByRole('listitem', { name: 'GitHub' }).getByRole('status')).toBeVisible()
    },
  },
  {
    n: '83',
    wave: 3,
    screen: 'settings-integrations',
    state: 'connect-jira',
    note: 'Connect dialog for Jira Cloud with the token hint',
    go: async (p) => void (await (await connectDialog(p)).getByLabel('API token').fill('ATATT3xFfGF0secret')),
  },
  {
    n: '84',
    wave: 3,
    screen: 'settings-integrations',
    state: 'connect-refused',
    note: 'The tracker refused the token (422 from the server)',
    go: async (p) => {
      const d = await connectDialog(p)
      await d.getByLabel('API token').fill('bad-token')
      await d.getByRole('button', { name: 'Connect' }).click()
      await expect(d.getByText(/refused the token/)).toBeVisible()
    },
  },
  {
    n: '85',
    wave: 3,
    screen: 'settings-integrations',
    state: 'connect-linear',
    note: 'Linear: the signing secret comes from Linear and is pasted here',
    go: async (p) => {
      await p.goto('/acme/settings/integrations')
      await p.getByRole('button', { name: 'Connect tracker' }).click()
      await p
        .getByRole('dialog')
        .getByRole('radio', { name: /Linear/ })
        .click()
    },
  },
  {
    n: '86',
    wave: 3,
    screen: 'settings-integrations',
    state: 'webhook-secret',
    note: 'After connecting: webhook URL, the one-time secret and the setup steps',
    go: async (p) => {
      const d = await connectDialog(p)
      await d.getByLabel('API token').fill('ATATT3xFfGF0secret')
      await d.getByRole('button', { name: 'Connect' }).click()
      await expect(p.getByRole('dialog', { name: /Set up the webhook/ }).getByText(/^whsec_/)).toBeVisible()
    },
  },
  {
    n: '87',
    wave: 3,
    screen: 'settings-integrations',
    state: 'edit',
    note: 'Editing: the token is write-only, only its hint is shown',
    go: async (p) => void (await p.goto('/acme/settings/integrations'), await p.getByRole('button', { name: 'Edit Jira' }).click()),
  },
  {
    n: '88',
    wave: 3,
    screen: 'settings-integrations',
    state: 'remove-confirm',
    go: async (p) => void (await p.goto('/acme/settings/integrations'), await p.getByRole('button', { name: 'Delete GitHub' }).click()),
  },
  {
    n: '89',
    wave: 3,
    screen: 'project-integrations',
    state: 'defaults',
    note: 'Project defaults: target from the tracker, issue type, labels, sync switches',
    go: async (p) =>
      void (await p.goto('/acme/settings/projects/web-app/integrations'), await expect(p.getByText('New tickets go to Web storefront.')).toBeVisible()),
  },
  {
    n: '90',
    wave: 3,
    screen: 'project-integrations',
    state: 'target-picker',
    go: async (p) => {
      await p.goto('/acme/settings/projects/web-app/integrations')
      await p.getByRole('combobox', { name: 'Jira project' }).first().click()
      await expect(p.getByRole('option', { name: /Payments/ })).toBeVisible()
    },
  },
  {
    n: '91',
    wave: 3,
    screen: 'issue-detail',
    state: 'linked-tickets',
    readme: 'linked-tickets',
    wide: true,
    note: 'Context panel with linked tickets (1920x1080)',
    go: async (p) => void (await openLinked(p), await p.getByTestId('linked-tickets').scrollIntoViewIfNeeded()),
  },
  {
    n: '92',
    wave: 3,
    screen: 'issue-detail',
    state: 'actions-menu-tickets',
    go: async (p) => void (await openLinked(p), await p.getByRole('button', { name: 'More issue actions' }).click()),
  },
  {
    n: '93',
    wave: 3,
    screen: 'issue-detail',
    state: 'create-ticket',
    note: 'Create ticket, prefilled from the ticket draft',
    go: async (p) => {
      await openLinked(p)
      await p.getByTestId('linked-tickets').getByRole('button', { name: 'Create ticket' }).click()
      await expect(p.getByRole('dialog').getByLabel('Title')).not.toHaveValue('')
    },
  },
  {
    n: '94',
    wave: 3,
    screen: 'issue-detail',
    state: 'create-ticket-preview',
    go: async (p) => {
      await openLinked(p)
      await p.getByTestId('linked-tickets').getByRole('button', { name: 'Create ticket' }).click()
      const d = p.getByRole('dialog')
      await expect(d.getByLabel('Title')).not.toHaveValue('')
      await d.getByRole('button', { name: 'Preview' }).click()
      await expect(d.getByText('Top tags')).toBeVisible()
    },
  },
  {
    n: '95',
    wave: 3,
    screen: 'issue-detail',
    state: 'link-existing-error',
    note: 'Link existing: an unknown reference (422)',
    go: async (p) => {
      await openLinked(p)
      await p.getByTestId('linked-tickets').getByRole('button', { name: 'Link existing' }).click()
      const d = p.getByRole('dialog')
      await d.getByRole('textbox', { name: 'Ticket' }).fill('see the ticket')
      await d.getByRole('button', { name: 'Link ticket' }).click()
      await expect(d.getByText(/Use a key or URL/)).toBeVisible()
    },
  },
  {
    n: '96',
    wave: 3,
    screen: 'issue-detail',
    state: 'activity-tickets',
    note: 'Activity with ticket entries: linked, status in the tracker, failed and done sync',
    go: async (p) => {
      await openLinked(p)
      await p
        .getByRole('navigation', { name: 'Issue sections' })
        .getByRole('link', { name: /^Activity/ })
        .click()
      await expect(p.getByText(/could not update/)).toBeVisible()
    },
  },
  {
    n: '97',
    wave: 3,
    screen: 'issues',
    state: 'split-preview-tickets',
    wide: true,
    note: 'Split view preview with the linked tickets (1920x1080)',
    go: async (p) => {
      await stream(p, '?sort=freq')
      await p.evaluate(() => localStorage.setItem('rupta-issue-split', '1'))
      await p.reload()
      await expect(p.getByRole('grid', { name: 'Issues' }).getByRole('row').first()).toBeVisible()
      await p.keyboard.press('j')
      await expect(p.getByTestId('issue-preview').getByTestId('linked-tickets').getByRole('listitem').first()).toBeVisible()
    },
  },
  {
    n: '98',
    wave: 3,
    screen: 'alerts',
    state: 'rule-create-ticket',
    note: 'Alert rule action: create a ticket',
    go: async (p) => {
      await p.goto('/acme/alerts/new')
      await p.getByLabel('Name').fill('Ticket for new errors in web-app')
      await p.getByRole('combobox', { name: 'Action type' }).first().click()
      await p.getByRole('option', { name: 'Create a ticket in' }).click()
      await p.getByLabel('Ticket target (optional)').fill('WEB')
    },
  },
  {
    n: '99',
    wave: 3,
    screen: 'settings-channels',
    state: 'pagerduty',
    note: 'PagerDuty channel: routing key write-only, region',
    go: async (p) => {
      await p.goto('/acme/settings/channels')
      await p.getByRole('button', { name: 'Add channel' }).click()
      const d = p.getByRole('dialog')
      await d.getByRole('radio', { name: 'PagerDuty' }).click()
      await d.getByLabel('Name').fill('Primary on-call')
      await d.getByLabel(/Integration key/).fill('R0UT1NGKEY0123456789abcdef012345')
    },
  },
  {
    n: '100',
    wave: 3,
    screen: 'settings-channels',
    state: 'internal-url-refused',
    note: 'A webhook to an internal address, refused by the server',
    go: async (p) => {
      await p.goto('/acme/settings/channels')
      await p.getByRole('button', { name: 'Add channel' }).click()
      const d = p.getByRole('dialog')
      await d.getByRole('radio', { name: 'Webhook' }).click()
      await d.getByLabel('Name').fill('Local hook')
      await d.getByLabel(/URL/).fill('http://10.0.0.12:9000/hook')
      await d.getByRole('button', { name: 'Add channel' }).click()
      await expect(d.getByText(/the address is internal/)).toBeVisible()
    },
  },
  {
    n: '101',
    wave: 3,
    screen: 'settings-channels',
    state: 'list-paging',
    note: 'Channels with PagerDuty and Opsgenie (keys stored)',
    go: async (p) => {
      await p.goto('/acme/settings/channels')
      for (const [kind, name, field, key] of [
        ['PagerDuty', 'Primary on-call', /Integration key/, 'R0UT1NGKEY0123456789abcdef012345'],
        ['Opsgenie', 'Ops team', /API key/, '0a1b2c3d-genie'],
      ] as const) {
        await p.getByRole('button', { name: 'Add channel' }).click()
        const d = p.getByRole('dialog')
        await d.getByRole('radio', { name: kind }).click()
        await d.getByLabel('Name').fill(name)
        await d.getByLabel(field).fill(key)
        await d.getByRole('button', { name: 'Add channel' }).click()
        await expect(d).toBeHidden()
      }
    },
  },
  {
    n: '102',
    wave: 3,
    mobile: true,
    screen: 'issue-detail',
    state: 'mobile-linked-tickets',
    go: async (p) => void (await openLinked(p), await p.getByTestId('linked-tickets').scrollIntoViewIfNeeded()),
  },
  {
    n: '103',
    wave: 4,
    signedOut: true,
    screen: 'login',
    state: 'sso-zitadel-mark',
    note: 'Sign-in with the instance SSO provider named Zitadel: the ZITADEL mark on the button',
    go: async (p) => void (await p.goto('/login?mock=signed-out'), await expect(p.getByRole('link', { name: 'Continue with Zitadel' })).toBeVisible()),
  },
  {
    n: '104',
    wave: 4,
    screen: 'org-settings',
    state: 'sso-presets-marks',
    note: 'SSO presets: the ZITADEL mark on the Zitadel preset, a key for the others; pointer on Keycloak',
    go: async (p) => {
      await p.goto('/acme/settings/sso')
      await p.getByRole('button', { name: /organization provider/ }).click()
      await p.getByRole('radio', { name: 'Keycloak' }).hover()
    },
  },
  {
    n: '105',
    wave: 4,
    screen: 'settings-integrations',
    state: 'picker-hover-focus',
    note: 'Tracker picker: GitHub focused with the keyboard (lime ring), pointer on GitLab (palm border, lift)',
    go: async (p) => {
      const d = await connectDialog(p)
      await d.getByRole('radio', { name: 'Jira Cloud' }).focus()
      for (let i = 0; i < 3; i++) await p.keyboard.press('ArrowRight')
      await expect(d.getByRole('radio', { name: 'GitHub' })).toBeFocused()
      await d.getByRole('radio', { name: 'GitLab' }).hover()
    },
  },
  {
    n: '106',
    wave: 4,
    screen: 'settings-channels',
    state: 'list-marks',
    note: 'Channels: Slack and Microsoft Teams monograms, email and webhook icons, PagerDuty and Opsgenie marks',
    go: async (p) => {
      await p.goto('/acme/settings/channels')
      for (const [kind, name, field, key] of [
        ['PagerDuty', 'Primary on-call', /Integration key/, 'R0UT1NGKEY0123456789abcdef012345'],
        ['Opsgenie', 'Ops team', /API key/, '0a1b2c3d-genie'],
      ] as const) {
        await p.getByRole('button', { name: 'Add channel' }).click()
        const d = p.getByRole('dialog')
        await d.getByRole('radio', { name: kind }).click()
        await d.getByLabel('Name').fill(name)
        await d.getByLabel(field).fill(key)
        await d.getByRole('button', { name: 'Add channel' }).click()
        await expect(d).toBeHidden()
      }
    },
  },
  {
    n: '107',
    wave: 4,
    screen: 'settings-channels',
    state: 'add-picker',
    note: 'Add channel: the channel type picker as logo tiles, Microsoft Teams selected',
    go: async (p) => {
      await p.goto('/acme/settings/channels')
      await p.getByRole('button', { name: 'Add channel' }).click()
      await p.getByRole('dialog').getByRole('radio', { name: 'Microsoft Teams' }).click()
    },
  },
  {
    n: '108',
    wave: 4,
    screen: 'alerts',
    state: 'channel-select-marks',
    note: 'Alert rule action: the channel list with the service marks',
    go: async (p) => {
      await p.goto('/acme/alerts/new')
      await p.getByRole('button', { name: 'Add action' }).click()
      await p.getByRole('combobox', { name: 'Channel' }).first().click()
      await expect(p.getByRole('listbox')).toBeVisible()
    },
  },
  {
    n: '109',
    wave: 4,
    screen: 'alerts',
    state: 'tracker-select-marks',
    note: 'Alert rule action: create a ticket, the tracker list with the marks',
    go: async (p) => {
      await p.goto('/acme/alerts/new')
      await p.getByRole('combobox', { name: 'Action type' }).first().click()
      await p.getByRole('option', { name: 'Create a ticket in' }).click()
      await p.getByRole('combobox', { name: 'Issue tracker' }).click()
      await expect(p.getByRole('listbox')).toBeVisible()
    },
  },
  {
    n: '110',
    wave: 4,
    screen: 'issue-detail',
    state: 'link-ticket-picker',
    note: 'Link an existing ticket: the tracker picker as logo tiles',
    go: async (p) => {
      await openLinked(p)
      await p.getByTestId('linked-tickets').getByRole('button', { name: 'Link existing' }).click()
      await expect(p.getByRole('radiogroup', { name: 'Tracker' })).toBeVisible()
    },
  },
)

for (const theme of ['dark'] as const) {
  test(`screenshots (${theme})`, async ({ browser }) => {
    test.setTimeout(1_800_000)
    mkdirSync(DOCS, { recursive: true })
    if (REVIEW) mkdirSync(REVIEW, { recursive: true })
    for (const s of shots) {
      if (!REVIEW && !s.readme) continue
      if (WAVES && !WAVES.has(String(s.wave))) continue
      if (ONLY && !ONLY.has(s.n)) continue
      const ctx = await browser.newContext({
        colorScheme: theme,
        deviceScaleFactor: s.wide ? 1 : 2,
        viewport: s.mobile ? { width: 390, height: 844 } : s.wide ? { width: 1920, height: 1080 } : { width: 1440, height: 900 },
        isMobile: s.mobile,
        hasTouch: s.mobile,
      })
      const page = await ctx.newPage()
      page.setDefaultTimeout(20_000)
      // start from the demo data, signed in
      if (s.n !== '01' && s.n !== '02' && !s.signedOut) {
        await page.goto('/acme/issues?mock=demo&mock_latency=0')
        await expect(page.getByRole('grid', { name: 'Issues' }).getByRole('row').first()).toBeVisible()
      }
      await s.go(page)
      await ready(page, s.streaming)
      const name = `${s.n}-${s.screen}-${s.state}.png`
      if (REVIEW) await page.screenshot({ path: `${REVIEW}/${name}` })
      if (s.readme && README_SHOTS.has(s.readme)) await page.screenshot({ path: `${DOCS}${s.readme}.png` })
      await ctx.close()
    }
    if (REVIEW) {
      const rows = shots
        .filter((s) => !WAVES || WAVES.has(String(s.wave)))
        .map((s) => {
          const size = s.mobile ? '390x844 @2x' : s.wide ? '1920x1080 @1x' : '1440x900 @2x'
          return `| ${s.n} | ${s.screen} | ${s.state} | ${size} | ${s.note ?? ''} | [${s.n}-${s.screen}-${s.state}.png](${s.n}-${s.screen}-${s.state}.png) |`
        })
      writeFileSync(
        `${REVIEW}/index.md`,
        `# rupta screens (review set, dark theme)\n\nMock build (\`pnpm build:mock\`) unless the file name starts with \`real-\` (a local server filled by \`scripts/sdk-smoke.sh\`).\n\n| # | Screen | State | Size | What it shows | File |\n| --- | --- | --- | --- | --- | --- |\n${rows.join('\n')}\n`,
      )
    }
  })
}
