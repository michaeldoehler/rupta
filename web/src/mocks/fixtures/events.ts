// Builds full normalized events for fixture issues, following the event
// shape of the design brief. Deterministic per issue and event index.

import type { Breadcrumb, EventSummary, ExceptionValue, Frame, RuptaEvent } from '@/api/types'
import { Rng } from '../rng'
import { errorSpanId, replayIdAt, replaysForIssue, traceIdForEvent } from './ids'
import { LIB_FRAMES, TEMPLATES, type FrameSpec, type Template } from './templates'
import { HOUR, iso, type DbIssue, type DbProject } from './world'

function toFrame(f: FrameSpec, t: Template, rng: Rng, minify: boolean): Frame {
  const frame: Frame = {
    filename: f.file,
    abs_path:
      f.abs ??
      (t.platform === 'javascript'
        ? `webpack:///${f.file.replace(/^\.\//, '')}`
        : t.platform === 'php'
          ? `/var/www/${f.file}`
          : t.platform === 'node'
            ? `/app/${f.file}`
            : f.file),
    module: f.module ?? null,
    function: f.fn,
    lineno: f.line,
    colno: f.col ?? null,
    in_app: f.inApp,
    context_line: f.code ?? null,
    pre_context: f.pre ?? [],
    post_context: f.post ?? [],
    vars: f.vars ?? null,
    platform: null,
    symbolicated: false,
    raw: null,
  }
  if (t.platform === 'javascript' && minify && f.file.startsWith('./')) {
    const chunk = f.inApp ? `index-${rng.hex(8)}` : `vendor-${rng.hex(8)}`
    frame.symbolicated = true
    frame.raw = {
      filename: `/assets/${chunk}.js`,
      abs_path: `https://app.acme.dev/assets/${chunk}.js`,
      function: f.fn.length > 3 ? f.fn.slice(0, 1).toLowerCase() : f.fn,
      lineno: 1,
      colno: rng.int(1000, 90000),
      in_app: f.inApp,
      context_line: f.inApp
        ? 'function Pe(e){const{data:t,isLoading:n}=Ue(e.category);return n?r(Fe,{}):r("ul",{className:"grid grid-cols-4 gap-4",children:t.items.map(e=>r(Re,{product:e},e.id))})}'
        : null,
    }
  }
  return frame
}

function framesFor(t: Template, top: FrameSpec, rng: Rng, withCallers: boolean): Frame[] {
  const minify = t.platform === 'javascript'
  // stack frames are ordered oldest call first (as SDKs send them)
  const lib = LIB_FRAMES[t.platform].map((f) => toFrame(f, t, rng, minify))
  const callers = withCallers ? (t.callers ?? []).map((f) => toFrame(f, t, rng, minify)) : []
  return [...lib, ...callers.reverse(), toFrame(top, t, rng, minify)]
}

type CrumbSpec = { type: string; category: string; level?: string; message?: (r: Rng) => string; data?: (r: Rng) => Record<string, unknown> }

const CRUMB_MSGS: Record<'javascript' | 'server', CrumbSpec[]> = {
  javascript: [
    {
      type: 'navigation',
      category: 'navigation',
      data: (r: Rng) => ({ from: '/catalog', to: r.pick(['/catalog/shoes', '/cart', '/account/orders', '/checkout']) }),
    },
    {
      type: 'http',
      category: 'fetch',
      data: (r: Rng) => ({
        method: 'GET',
        url: r.pick(['/api/v2/products?category=shoes', '/api/v2/cart', '/api/v2/session']),
        status_code: r.pick([200, 200, 200, 304, 500, 404]),
      }),
    },
    { type: 'http', category: 'xhr', data: () => ({ method: 'POST', url: 'https://events.acme.dev/collect', status_code: 204 }) },
    {
      type: 'user',
      category: 'ui.click',
      message: (r: Rng) => r.pick(['button.btn-primary[data-test="add-to-cart"]', 'a.nav-link[href="/cart"]', 'div.product-card > img', 'button#checkout']),
    },
    { type: 'user', category: 'ui.input', message: () => 'input[name="search"]' },
    { type: 'debug', category: 'console', level: 'warning', message: () => 'Warning: Each child in a list should have a unique "key" prop.' },
    { type: 'debug', category: 'console', level: 'log', message: () => '[analytics] page_view sent' },
    { type: 'error', category: 'console', level: 'error', message: () => 'Failed to load resource: the server responded with a status of 500 ()' },
  ],
  server: [
    {
      type: 'query',
      category: 'db.query',
      message: (r: Rng) =>
        r.pick([
          'SELECT "orders"."id", "orders"."total" FROM "orders" WHERE "orders"."customer_id" = %s LIMIT 21',
          'UPDATE invoices SET status = ? WHERE id = ?',
          'SELECT * FROM `customers` WHERE `id` = ? LIMIT 1',
          'BEGIN',
          'COMMIT',
        ]),
    },
    {
      type: 'http',
      category: 'httplib',
      data: (r: Rng) => ({
        method: 'POST',
        url: r.pick(['https://api.stripe.com/v1/payment_intents', 'https://api.exchangerate.host/latest', 'http://search.internal:9200/products/_search']),
        status_code: r.pick([200, 200, 201, 502, 429]),
      }),
    },
    { type: 'default', category: 'cache', message: (r: Rng) => r.pick(['cache miss: cart:7f3a', 'cache hit: rates:EUR', 'cache set: session:91c2']) },
    { type: 'default', category: 'auth', message: () => 'User authenticated via session cookie' },
    {
      type: 'debug',
      category: 'logger',
      level: 'info',
      message: (r: Rng) => r.pick(['Processing order 48211', 'Retrying request (attempt 2/3)', 'Queued job render-invoice-pdf']),
    },
    { type: 'error', category: 'logger', level: 'warning', message: () => 'Slow query detected (1.84s)' },
  ],
}

function breadcrumbsFor(t: Template, rng: Rng, ts: number): Breadcrumb[] {
  const lib = t.platform === 'javascript' ? CRUMB_MSGS.javascript : CRUMB_MSGS.server
  const n = rng.int(8, 26)
  const out: Breadcrumb[] = []
  let at = ts - rng.int(20, 180) * 1000
  for (let i = 0; i < n; i++) {
    const spec = rng.pick(lib)
    at += rng.int(200, 9000)
    const data = spec.data ? spec.data(rng) : null
    const status = data && typeof data === 'object' && 'status_code' in data ? (data.status_code as number) : null
    out.push({
      timestamp: iso(Math.min(at, ts - 50)),
      type: spec.type,
      category: spec.category,
      level: spec.level ? spec.level : status && status >= 500 ? 'error' : status && status >= 400 ? 'warning' : 'info',
      message: spec.message ? spec.message(rng) : null,
      data,
    })
  }
  out.push({
    timestamp: iso(ts),
    type: 'error',
    category: 'exception',
    level: 'error',
    message: `${t.type ?? 'Message'}: ${t.value.split('\n')[0]}`,
    data: null,
  })
  return out
}

const FIRST = ['anna', 'ben', 'carla', 'deniz', 'emil', 'farah', 'gus', 'hana', 'ivan', 'julia']

export function eventIds(issue: DbIssue): string[] {
  const rng = new Rng(`ids:${issue.id}`)
  const n = Math.min(issue.count, 60)
  return Array.from({ length: n }, () => rng.uuid())
}

function eventTime(issue: DbIssue, index: number, total: number): number {
  if (total <= 1) return issue.last_seen
  // index 0 = oldest
  const span = issue.last_seen - issue.first_seen
  return Math.round(issue.first_seen + (span * index) / (total - 1))
}

export function buildEvent(issue: DbIssue, project: DbProject, index: number): RuptaEvent {
  const ids = eventIds(issue)
  const t = TEMPLATES[issue.template]
  const rng = new Rng(`${issue.id}:${index}`)
  const ts = eventTime(issue, index, ids.length)
  const isJs = t.platform === 'javascript'
  const browser = issue.tags.browser || 'Chrome 131'
  const [browserName, browserVersion] = [browser.replace(/ [\d.]+$/, ''), browser.match(/[\d.]+$/)?.[0] ?? '']
  const os = issue.tags.os || 'Linux'
  const user = rng.chance(t.platform === 'go' ? 0 : 0.85)
    ? {
        id: String(rng.int(1000, 99999)),
        email: `${rng.pick(FIRST)}${rng.int(1, 99)}@example.com`,
        username: null,
        ip_address: `198.51.100.${rng.int(2, 254)}`,
        geo: {
          country_code: rng.pick(['DE', 'US', 'FR', 'GB', 'NL', 'BR', 'JP']),
          city: rng.pick(['Berlin', 'Austin', 'Lyon', 'Leeds', 'Utrecht', 'São Paulo', 'Osaka']),
        },
        data: { plan: rng.pick(['free', 'pro', 'business']) },
      }
    : null
  const environment = index % 7 === 3 ? 'staging' : issue.tags.environment || 'production'
  const release = issue.last_release
  const url = isJs
    ? `https://app.acme.dev${t.transaction.replace(':category', 'shoes').replace('[slug]', 'winter-sale')}`
    : `https://api.acme.dev${t.transaction.split(' ').pop()?.replace('{invoice}', 'INV-2025-99999').replace('{id}', '42') ?? '/'}`
  const method = t.transaction.startsWith('POST') || t.transaction.includes('orders') ? 'POST' : 'GET'

  const exception: ExceptionValue[] = []
  if (!t.message) {
    if (t.cause) {
      exception.push({
        type: t.cause.type,
        value: t.cause.value,
        module: null,
        mechanism: { type: 'chained', handled: t.handled, source: '__cause__', exception_id: 1, parent_id: 0 },
        stacktrace: { frames: framesFor(t, t.cause.top, rng, false) },
      })
    }
    exception.push({
      type: t.type ?? 'Error',
      value: issue.title.includes(': ') ? issue.title.slice(issue.title.indexOf(': ') + 2) : t.value,
      module: t.platform === 'python' ? (t.top.module ?? null) : null,
      mechanism: { type: t.mechanism, handled: t.handled, exception_id: 0 },
      stacktrace: { frames: framesFor(t, t.top, rng, true) },
    })
  }

  const tags: [string, string][] = [
    ['environment', environment],
    ['level', issue.level],
    ['handled', t.handled ? 'yes' : 'no'],
    ['mechanism', t.mechanism],
  ]
  if (release) tags.push(['release', release])
  if (isJs) {
    tags.push(
      ['browser', browser],
      ['browser.name', browserName],
      ['os', os],
      ['os.name', os.replace(/ [\d.]+$/, '')],
      ['device', issue.tags.device || 'Desktop'],
      ['url', url],
    )
  } else {
    tags.push(
      ['server_name', issue.tags.server_name || 'web-1'],
      ['runtime', ({ python: 'CPython 3.13.0', php: 'php 8.4.1', node: 'node v22.11.0', go: 'go go1.23.3' } as Record<string, string>)[t.platform] ?? ''],
      ['os.name', 'Linux'],
    )
  }
  tags.push(['transaction', t.transaction])
  if (user) tags.push(['user', `id:${user.id}`])
  tags.push(['customer_tier', rng.pick(['free', 'pro', 'business'])])
  if (isJs) tags.push(['feature.new_checkout', rng.chance(0.4) ? 'on' : 'off'])

  const traceId = traceIdForEvent(issue.id, index)
  const contexts: RuptaEvent['contexts'] = {
    trace: { trace_id: traceId, span_id: errorSpanId(traceId), op: isJs ? 'navigation' : 'http.server', status: 'internal_error' },
  }
  // Browser events carry the session replay that captured them (when sampled).
  if (isJs && index % 5 !== 2) {
    const linked = replaysForIssue(issue.id)
    contexts.replay = { replay_id: replayIdAt(linked[index % linked.length]), type: 'replay' }
  }
  if (isJs) {
    contexts.browser = { name: browserName, version: browserVersion, type: 'browser' }
    contexts.os = { name: os.replace(/ [\d.]+$/, ''), version: os.match(/[\d.]+$/)?.[0] ?? '', type: 'os' }
    contexts.device = {
      family: issue.tags.device || 'Desktop',
      model: issue.tags.device || 'Unknown',
      brand: issue.tags.device?.startsWith('iPhone') ? 'Apple' : null,
      type: 'device',
    }
    contexts.culture = {
      locale: rng.pick(['en-US', 'de-DE', 'fr-FR']),
      timezone: rng.pick(['Europe/Berlin', 'America/Chicago', 'Europe/Paris']),
      type: 'culture',
    }
    contexts.react = { version: '19.3.1', type: 'default' }
  } else {
    contexts.os = { name: 'Linux', version: '6.8.0', kernel_version: '6.8.0-48-generic', type: 'os' }
    contexts.runtime =
      (
        {
          python: { name: 'CPython', version: '3.13.0' },
          php: { name: 'php', version: '8.4.1' },
          node: { name: 'node', version: 'v22.11.0' },
          go: { name: 'go', version: 'go1.23.3' },
        } as Record<string, Record<string, unknown>>
      )[t.platform] ?? {}
    contexts.app = { app_start_time: iso(ts - 3 * HOUR), app_memory: 212_000_000, type: 'app' }
    contexts.device = { arch: 'x86_64', num_cpu: 4, memory_size: 8_589_934_592, type: 'device' }
  }

  const headers: [string, string][] = isJs
    ? [
        ['Accept', 'text/html,application/xhtml+xml'],
        ['Accept-Language', 'en-US,en;q=0.9'],
        ['Referer', 'https://app.acme.dev/catalog'],
        ['User-Agent', `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) ${browserName}/${browserVersion} Safari/537.36`],
      ]
    : [
        ['Accept', 'application/json'],
        ['Authorization', '[Filtered]'],
        ['Content-Type', 'application/json'],
        ['Host', 'api.acme.dev'],
        ['User-Agent', 'acme-web/2.14.0'],
        ['X-Forwarded-For', '198.51.100.23'],
        ['X-Request-Id', rng.hex(16)],
      ]

  const sdkName = {
    javascript: project.platform === 'javascript-nextjs' ? 'sentry.javascript.nextjs' : 'sentry.javascript.react',
    node: 'sentry.javascript.node',
    python: 'sentry.python.django',
    php: project.platform === 'php-symfony' ? 'sentry.php.symfony' : 'sentry.php.laravel',
    go: 'sentry.go',
  }[t.platform]

  return {
    event_id: ids[index] ?? rng.uuid(),
    project_id: project.id,
    issue_id: issue.id,
    timestamp: iso(ts),
    received: iso(ts + rng.int(40, 900)),
    level: issue.level,
    platform: t.platform === 'node' ? 'node' : t.platform,
    message: t.message ? t.value : null,
    title: issue.title,
    culprit: issue.culprit,
    logger: t.platform === 'python' ? 'django.request' : null,
    environment,
    release,
    dist: null,
    server_name: isJs ? null : issue.tags.server_name || 'web-1',
    transaction: t.transaction,
    fingerprint: ['{{ default }}'],
    tags,
    user,
    request:
      t.platform === 'go' && index % 2
        ? null
        : {
            url,
            method,
            headers,
            query_string: isJs
              ? [
                  ['ref', 'newsletter'],
                  ['utm_source', 'email'],
                ]
              : method === 'GET'
                ? [
                    ['page', '2'],
                    ['limit', '50'],
                  ]
                : null,
            data: method === 'POST' ? { items: [{ sku: 'A-1003', qty: 1 }], coupon: 'WINTER25', customer: { email: '[Filtered]' } } : null,
            cookies: isJs
              ? [
                  ['session', '[Filtered]'],
                  ['theme', 'dark'],
                ]
              : null,
            env: isJs ? null : { REMOTE_ADDR: '10.0.1.17', SERVER_NAME: 'api.acme.dev' },
          },
    contexts,
    sdk: {
      name: sdkName,
      version: { javascript: '9.14.0', node: '9.14.0', python: '2.19.2', php: '4.11.0', go: '0.31.1' }[t.platform],
      integrations: isJs
        ? ['InboundFilters', 'FunctionToString', 'BrowserApiErrors', 'Breadcrumbs', 'GlobalHandlers', 'LinkedErrors', 'Dedupe', 'HttpContext', 'BrowserTracing']
        : ['logging', 'stdlib', 'excepthook', 'dedupe', 'atexit', 'modules', 'argv', 'threading'],
      packages: [{ name: `npm:@sentry/${t.platform === 'javascript' ? 'react' : 'node'}`, version: '9.14.0' }],
    },
    exception: exception.length ? { values: exception } : null,
    threads: null,
    breadcrumbs: { values: breadcrumbsFor(t, rng, ts) },
    extra:
      t.platform === 'python'
        ? { celery_task_id: rng.uuid(), retries: rng.int(0, 3), sys_argv: ['/app/manage.py', 'runserver'] }
        : isJs
          ? { componentStack: '\n    at ProductList\n    at Suspense\n    at CategoryPage\n    at Layout', 'session:duration': rng.int(5, 900) }
          : { job_id: String(rng.int(1000, 99999)) },
    modules:
      t.platform === 'python'
        ? { django: '5.1.3', djangorestframework: '3.15.2', celery: '5.4.0', psycopg: '3.2.3', redis: '5.2.0', stripe: '11.2.0' }
        : t.platform === 'php'
          ? { 'laravel/framework': 'v11.33.2', 'guzzlehttp/guzzle': '7.9.2', 'stripe/stripe-php': 'v16.2.0' }
          : isJs
            ? { react: '19.3.1', 'react-dom': '19.3.1', '@tanstack/react-query': '5.107.1' }
            : null,
    debug_meta: isJs
      ? {
          images: [
            {
              type: 'sourcemap',
              code_file: 'https://app.acme.dev/assets/index.js',
              debug_id: `${rng.hex(8)}-${rng.hex(4)}-${rng.hex(4)}-${rng.hex(4)}-${rng.hex(12)}`,
            },
          ],
        }
      : null,
    errors: index % 5 === 4 && isJs ? [{ type: 'js_no_source', name: 'url', value: 'https://app.acme.dev/assets/vendor-4f2a1c0e.js' }] : null,
    previous_event_id: index > 0 ? ids[index - 1] : null,
    next_event_id: index < ids.length - 1 ? ids[index + 1] : null,
  }
}

export function eventSummary(issue: DbIssue, project: DbProject, index: number): EventSummary {
  const e = buildEvent(issue, project, index)
  const tag = (k: string) => e.tags.find(([key]) => key === k)?.[1] ?? null
  return {
    event_id: e.event_id,
    timestamp: e.timestamp,
    title: e.title,
    message: e.message ?? e.exception?.values.at(-1)?.value ?? null,
    level: e.level,
    environment: e.environment,
    release: e.release,
    user: e.user ? { id: e.user.id, email: e.user.email, ip_address: e.user.ip_address } : null,
    browser: tag('browser'),
    os: tag('os') ?? tag('os.name'),
    url: e.request?.url ?? null,
    transaction: e.transaction,
  }
}
