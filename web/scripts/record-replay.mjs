// Records a real session with rrweb's recorder on a small demo shop
// (scripts/replay-demo.html) in Chromium and writes it as the mock
// fixture src/mocks/fixtures/replay-recording.json.
//
//   node scripts/record-replay.mjs
//
// The demo page adds the custom events the browser SDK's replay
// integration records (breadcrumbs, performance spans), so the player sees
// the same shapes as with real SDK data. Uses PLAYWRIGHT_CHROMIUM_EXECUTABLE
// or the preinstalled Chromium.
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { chromium } from '@playwright/test'

const require = createRequire(import.meta.url)
const here = (p) => fileURLToPath(new URL(p, import.meta.url))
const rrwebUmd = readFileSync(require.resolve('rrweb').replace(/rrweb\.cjs$/, 'rrweb.umd.cjs'), 'utf8')
const html = readFileSync(here('./replay-demo.html'), 'utf8')
const executablePath = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ?? ['/opt/pw-browsers/chromium-1243/chrome-linux64/chrome'].find((p) => existsSync(p))
const ORIGIN = 'https://shop.acme.dev'

const browser = await chromium.launch({ executablePath })
const page = await browser.newPage({ viewport: { width: 1280, height: 760 } })
page.on('pageerror', (e) => console.warn('page error:', e.message))
let orders = 0
await page.route(`${ORIGIN}/**`, async (route) => {
  const url = new URL(route.request().url())
  if (url.pathname === '/api/cart') return route.fulfill({ status: 201, contentType: 'application/json', body: '{"ok":true}' })
  if (url.pathname === '/api/orders') {
    orders += 1
    // The first attempt fails like a payment provider timeout.
    if (orders === 1) return route.fulfill({ status: 502, contentType: 'application/json', body: '{"error":"payment provider timeout"}' })
    return route.fulfill({ status: 201, contentType: 'application/json', body: '{"id":"A-10482"}' })
  }
  return route.fulfill({ status: 200, contentType: 'text/html', body: html })
})
// rrweb creates about:blank frames for untainted prototypes; init scripts run
// there too, so only the top-level shop page loads the recorder.
await page.addInitScript(`if (location.origin === '${ORIGIN}' && window.top === window) {
${rrwebUmd}
window.__events = [];
document.addEventListener('DOMContentLoaded', () => {
  window.rrweb.record({ emit: (e) => window.__events.push(e), maskAllInputs: false, maskInputOptions: { password: true }, sampling: { mousemove: 50, scroll: 150, input: 'last' }, recordCanvas: false });
  window.__recording = true;
  const t = Date.now() / 1000;
  window.rrweb.record.addCustomEvent('performanceSpan', { op: 'navigation.navigate', description: location.href, startTimestamp: performance.timeOrigin / 1000, endTimestamp: t, data: { size: 9800, duration: Math.round(t * 1000 - performance.timeOrigin) } });
  window.rrweb.record.addCustomEvent('breadcrumb', { timestamp: t, type: 'default', category: 'navigation', message: '', data: { from: null, to: location.pathname } });
});
}`)

const pause = (ms) => page.waitForTimeout(ms)
const moveTo = async (selector) => {
  const box = await page.locator(selector).first().boundingBox()
  if (box) await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 18 })
}
const click = async (selector) => {
  await moveTo(selector)
  await pause(250)
  await page.locator(selector).first().click()
}

await page.goto(`${ORIGIN}/`)
await pause(1500)
await page.mouse.move(400, 300, { steps: 10 })
await page.mouse.wheel(0, 400)
await pause(900)
await page.mouse.wheel(0, -400)
await pause(600)
await click('.shop-now')
await pause(1200)
await click('.add-to-cart >> nth=0')
await pause(900)
await click('.add-to-cart >> nth=2')
await pause(700)
await page.evaluate(() => console.warn('Recommendation widget slow to load (1240 ms)'))
await click('.add-to-cart >> nth=4')
await pause(1100)
await click('.cart')
await pause(1000)
await click('#email')
await page.keyboard.type('maria.keller@example.com', { delay: 45 })
await click('#card')
await page.keyboard.type('4242424242424242', { delay: 35 })
await pause(500)
await click('.place-order')
const errorAt = Date.now()
await pause(1500)
// A stretch without input, so "skip inactivity" has something to skip.
await pause(9000)
await click('#retry')
await pause(1500)
await click('.btn.secondary')
await pause(1200)

const events = await page.evaluate(() => window.__events)
await browser.close()
const start = events[0].timestamp
const out = {
  // How the fixture was made; the mock API shifts timestamps to "now".
  source: 'rrweb 2.2.2 recorder, scripts/record-replay.mjs on scripts/replay-demo.html',
  url: `${ORIGIN}/`,
  errors: [{ offset_ms: errorAt - start, title: 'Error: Checkout failed: payment provider timed out (502)' }],
  events,
}
const file = here('../src/mocks/fixtures/replay-recording.json')
writeFileSync(file, JSON.stringify(out))
console.warn(`wrote ${events.length} events, ${Math.round((events.at(-1).timestamp - start) / 1000)} s, ${Math.round(JSON.stringify(out).length / 1024)} KiB to ${file}`)
