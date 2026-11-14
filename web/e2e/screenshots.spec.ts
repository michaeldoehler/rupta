// Screenshots of the main screens in both themes, from the mock build.
//   pnpm screenshots                 -> docs/images (README set)
//   REVIEW_DIR=/path pnpm screenshots -> also a full review set of every screen
//   REVIEW_WAVE=2 REVIEW_DIR=/path      -> only the telemetry screens, with an index.md
import { mkdirSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { expect, test, type Page } from '@playwright/test'

const DOCS = fileURLToPath(new URL('../../docs/images/', import.meta.url))
const REVIEW = process.env.REVIEW_DIR
const README_SHOTS = new Set(['issues', 'issue-detail', 'stacktrace', 'release-detail', 'project-keys', 'command-palette', 'trace', 'logs', 'flamegraph', 'replay'])
const WAVE = process.env.REVIEW_WAVE

type Shot = { n: string; screen: string; state: string; go: (page: Page) => Promise<void>; mobile?: boolean; readme?: string; wave?: 2; note?: string }

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
  await page.getByRole('grid', { name: 'Profiles' }).getByRole('link', { name: /GET \/api\/products$/ }).first().click()
  await expect(page.getByTestId('flamegraph')).toBeVisible()
}

async function openReplay(page: Page) {
  await page.goto('/acme/replays')
  await page.getByRole('grid', { name: 'Replays' }).getByRole('link').first().click()
  await expect(page.getByTestId('replay-player').locator('iframe')).toBeAttached()
}

shots.push(
  { n: '41', wave: 2, screen: 'performance', state: 'transactions', note: 'Transactions table with throughput, percentiles, failure rate and apdex', go: async (p) => void (await p.goto('/acme/performance'), await expect(p.getByRole('table', { name: 'Transactions' })).toBeVisible()) },
  { n: '42', wave: 2, screen: 'performance', state: 'empty-search', note: 'No transactions match the search', go: async (p) => void (await p.goto('/acme/performance'), await p.getByLabel('Search transactions').fill('nothing-like-this'), await expect(p.getByText('No transactions match')).toBeVisible()) },
  { n: '43', wave: 2, screen: 'transaction-summary', state: 'browser-vitals', note: 'Summary of a page load: charts, web vitals against thresholds', go: async (p) => void (await p.goto('/acme/performance/summary?name=%2Fproducts%2F%3Aslug'), await expect(p.getByTestId('vital-lcp')).toBeVisible()) },
  { n: '44', wave: 2, screen: 'transaction-summary', state: 'histogram-samples', note: 'Duration histogram, related issues and slowest samples of an API endpoint', go: async (p) => void (await p.goto('/acme/performance/summary?name=POST%20%2Fapi%2Forders'), await expect(p.getByRole('table', { name: 'Slowest samples' })).toBeVisible(), await p.getByRole('img', { name: 'Duration histogram' }).scrollIntoViewIfNeeded()) },
  { n: '45', wave: 2, screen: 'trace', state: 'waterfall', readme: 'trace', note: 'Trace across browser, API and worker with error and log markers', go: openTrace },
  { n: '46', wave: 2, screen: 'trace', state: 'span-details', note: 'Span details panel with errors, logs, data', go: async (p) => void (await openTrace(p), await p.getByRole('treegrid', { name: 'Spans' }).getByRole('row').nth(3).click(), await expect(p.getByTestId('span-drawer')).toBeVisible()) },
  { n: '47', wave: 2, screen: 'trace', state: 'critical-path-zoomed', note: 'Critical path highlighted, timeline zoomed with the minimap window', go: async (p) => void (await openTrace(p), await p.getByRole('button', { name: 'Critical path' }).click(), await p.getByRole('button', { name: 'Zoom in' }).click()) },
  { n: '48', wave: 2, screen: 'trace', state: 'long-trace-collapsed', note: 'A batch job with 1,300 spans, collapsed below the transactions', go: async (p) => void (await p.goto('/acme/performance/summary?name=reindex-search'), await p.getByRole('table', { name: 'Slowest samples' }).getByRole('link').first().click(), await expect(p.getByRole('treegrid', { name: 'Spans' }).getByRole('row').first()).toBeVisible()) },
  { n: '49', wave: 2, screen: 'trace', state: 'not-found', note: 'A trace id with nothing stored', go: async (p) => void (await p.goto('/acme/traces/00000000000000000000000000000000'), await expect(p.getByText('Not found')).toBeVisible()) },
  { n: '50', wave: 2, screen: 'span-explorer', state: 'by-operation', note: 'Span explorer grouped by operation', go: async (p) => void (await p.goto('/acme/traces'), await expect(p.getByRole('table', { name: 'Span groups' })).toBeVisible()) },
  { n: '51', wave: 2, screen: 'span-explorer', state: 'spans', note: 'Individual spans (group_by=none) linking to their traces', go: async (p) => void (await p.goto('/acme/traces?group_by=none&q=op%3Adb.query'), await expect(p.getByRole('table', { name: 'Spans' })).toBeVisible()) },
  { n: '52', wave: 2, screen: 'logs', state: 'explorer', readme: 'logs', note: 'Logs: level histogram, facets, virtualised table', go: async (p) => void (await p.goto('/acme/logs?period=24h'), await grid(p, 'Log lines')) },
  { n: '53', wave: 2, screen: 'logs', state: 'autocomplete', note: 'Query bar suggesting attribute keys', go: async (p) => void (await p.goto('/acme/logs?period=24h'), await grid(p, 'Log lines'), await p.getByRole('combobox', { name: 'Search logs' }).click(), await p.getByRole('combobox', { name: 'Search logs' }).pressSequentially('http.'), await expect(p.getByRole('option').first()).toBeVisible()) },
  { n: '54', wave: 2, screen: 'logs', state: 'expanded-row', note: 'Expanded line with all attributes, trace link and issues of the trace', go: async (p) => void (await p.goto('/acme/logs?period=24h&query=level%3Awarn%20service.name%3Aapi'), await grid(p, 'Log lines'), await p.getByRole('grid', { name: 'Log lines' }).getByRole('row').filter({ has: p.getByRole('link', { name: 'Open trace' }) }).first().click(), await expect(p.getByText('View trace')).toBeVisible()) },
  { n: '55', wave: 2, screen: 'logs', state: 'live-tail', note: 'Live tail streaming new lines over Server-Sent Events', go: async (p) => void (await p.goto('/acme/logs?period=1h&live=true'), await expect(p.getByRole('status').getByText('Live', { exact: true })).toBeVisible({ timeout: 10_000 }), await p.waitForTimeout(3000)) },
  { n: '56', wave: 2, screen: 'logs', state: 'empty', note: 'No lines match', go: async (p) => void (await p.goto('/acme/logs?query=%22no%20such%20line%22'), await expect(p.getByText('No log lines match')).toBeVisible()) },
  { n: '57', wave: 2, screen: 'mobile-logs', state: 'default', mobile: true, note: 'Logs on a phone', go: async (p) => void (await p.goto('/acme/logs?period=24h'), await grid(p, 'Log lines')) },
  { n: '58', wave: 2, screen: 'metrics', state: 'explorer-grouped', note: 'Metric explorer: p95 by route, area chart, series table', go: async (p) => void (await p.goto('/acme/metrics?name=http.server.duration&op=p95&group_by=route&chart=area'), await expect(p.getByRole('table', { name: 'Series summary' })).toBeVisible()) },
  { n: '59', wave: 2, screen: 'metrics', state: 'counter-bars', note: 'A counter as bars', go: async (p) => void (await p.goto('/acme/metrics?name=checkout.completed'), await expect(p.getByRole('table', { name: 'Series summary' })).toBeVisible()) },
  { n: '60', wave: 2, screen: 'dashboards', state: 'list', note: 'Dashboards', go: async (p) => void (await p.goto('/acme/dashboards'), await expect(p.getByRole('link', { name: /Checkout health/ })).toBeVisible()) },
  { n: '61', wave: 2, screen: 'dashboard', state: 'grid', note: 'Dashboard with metric, transaction, log and issue widgets', go: async (p) => void (await p.goto('/acme/dashboards/1'), await expect(p.getByRole('list', { name: 'Widgets' })).toBeVisible()) },
  { n: '62', wave: 2, screen: 'dashboard', state: 'widget-editor', note: 'Add widget dialog with live preview', go: async (p) => { await p.goto('/acme/dashboards/2'); await p.getByRole('button', { name: 'Add widget' }).first().click(); await p.getByRole('dialog').getByLabel('Metric').click(); await p.getByRole('option', { name: 'jobs.processed' }).click() } },
  { n: '63', wave: 2, screen: 'profiles', state: 'list', note: 'Profile list', go: async (p) => void (await p.goto('/acme/profiles'), await grid(p, 'Profiles')) },
  { n: '64', wave: 2, screen: 'profiles', state: 'by-transaction', note: 'Profiles grouped by transaction', go: async (p) => void (await p.goto('/acme/profiles/transactions'), await expect(p.getByRole('link', { name: /GET \/api\/products/ }).first()).toBeVisible()) },
  { n: '65', wave: 2, screen: 'flamegraph', state: 'profile', readme: 'flamegraph', note: 'Flamegraph of a Python request, call tree', go: openProfile },
  { n: '66', wave: 2, screen: 'flamegraph', state: 'search-selected', note: 'Search highlighting and frame details', go: async (p) => void (await openProfile(p), await p.getByLabel('Search frames').fill('psycopg'), await p.getByRole('treegrid', { name: 'Call tree' }).getByRole('row').nth(4).click()) },
  { n: '67', wave: 2, screen: 'flamegraph', state: 'left-heavy-package', note: 'Left-heavy order, coloured by package, functions table', go: async (p) => void (await openProfile(p), await p.getByRole('radio', { name: 'Left heavy' }).click(), await p.getByRole('radio', { name: 'Package' }).click(), await p.getByRole('tab', { name: 'Functions' }).click()) },
  { n: '68', wave: 2, screen: 'flamegraph', state: 'aggregated', note: 'Aggregated flamegraph of a transaction', go: async (p) => void (await p.goto('/acme/profiles/aggregate?transaction=GET%20%2Fapi%2Fproducts'), await expect(p.getByTestId('flamegraph')).toBeVisible()) },
  { n: '69', wave: 2, screen: 'replays', state: 'list', note: 'Session replays', go: async (p) => void (await p.goto('/acme/replays'), await grid(p, 'Replays')) },
  { n: '70', wave: 2, screen: 'mobile-replays', state: 'list', mobile: true, note: 'Replays on a phone', go: async (p) => void (await p.goto('/acme/replays'), await grid(p, 'Replays')) },
  { n: '71', wave: 2, screen: 'replay', state: 'playing', readme: 'replay', note: 'Player with timeline markers and breadcrumbs (a real rrweb recording)', go: async (p) => void (await openReplay(p), await p.getByTestId('replay-player').getByRole('button', { name: 'Play' }).first().click(), await p.waitForTimeout(5500)) },
  { n: '72', wave: 2, screen: 'replay', state: 'network-panel', note: 'Network panel; the failed request at 0:15', go: async (p) => void (await openReplay(p), await p.getByRole('tab', { name: /Network/ }).click(), await p.getByRole('tabpanel').getByRole('button').filter({ hasText: '502' }).click(), await p.waitForTimeout(1200)) },
  { n: '73', wave: 2, screen: 'replay', state: 'console-errors', note: 'Console and errors panels', go: async (p) => void (await openReplay(p), await p.getByRole('tab', { name: /Console/ }).click()) },
  { n: '74', wave: 2, screen: 'issue-detail', state: 'trace-replay-links', note: 'Issue event linking to its trace and replay', go: async (p) => void (await openIssue(p), await expect(p.getByRole('link', { name: /^Trace [0-9a-f]{8}/ })).toBeVisible()) },
  { n: '75', wave: 2, screen: 'issue-detail', state: 'replays-tab', note: 'Replays tab of an issue', go: async (p) => { await stream(p, '?sort=freq&query=is%3Aunresolved%20ChunkLoadError'); await p.getByRole('grid', { name: 'Issues' }).getByRole('row').first().locator('[data-issue-link]').click(); await p.getByRole('navigation', { name: 'Issue sections' }).getByRole('link', { name: 'Replays' }).click(); await expect(p.getByText(/sessions? with this error|No replays/)).toBeVisible() } },
  { n: '76', wave: 2, screen: 'project-settings', state: 'telemetry-data', note: 'Per-type retention, sampling and switches', go: async (p) => void (await p.goto('/acme/settings/projects/web-app/telemetry'), await expect(p.getByRole('switch', { name: 'Accept logs' })).toBeVisible()) },
  { n: '77', wave: 2, screen: 'navigation', state: 'palette-trace', note: 'Command palette: telemetry pages and opening a trace by id', go: async (p) => void (await stream(p), await p.keyboard.press('ControlOrMeta+k'), await p.getByPlaceholder(/Type a command/).fill('0123456789abcdef0123456789abcdef')) },
  { n: '78', wave: 2, screen: 'keyboard-help', state: 'telemetry', note: 'Keyboard shortcuts including the telemetry pages', go: async (p) => void (await stream(p), await p.keyboard.press('Shift+?'), await expect(p.getByRole('dialog')).toBeVisible()) },
)

for (const theme of ['light', 'dark'] as const) {
  test(`screenshots (${theme})`, async ({ browser }) => {
    test.setTimeout(600_000)
    mkdirSync(DOCS, { recursive: true })
    if (REVIEW) mkdirSync(REVIEW, { recursive: true })
    for (const s of shots) {
      if (!REVIEW && !s.readme) continue
      if (WAVE === '2' && s.wave !== 2) continue
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
    if (REVIEW && WAVE === '2' && theme === 'dark') {
      const rows = shots
        .filter((s) => s.wave === 2)
        .map((s) => `| ${s.n} | ${s.screen} | ${s.state} | ${s.note ?? ''} | [light](${s.n}-${s.screen}-${s.state}-light.png) · [dark](${s.n}-${s.screen}-${s.state}-dark.png) |`)
      writeFileSync(
        `${REVIEW}/index.md`,
        `# rupta telemetry screens (review set)\n\nMock build (\`pnpm build:mock\`), 1440x900 at device scale factor 2; phones 390x844. Files starting with \`real-\` come from a local server filled by \`scripts/sdk-smoke.sh\`.\n\n| # | Screen | State | What it shows | Files |\n| --- | --- | --- | --- | --- |\n${rows.join('\n')}\n`,
      )
    }
  })
}
