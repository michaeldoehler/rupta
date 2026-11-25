import { setupWorker } from 'msw/browser'
import { handlers, resetMock, setMockLatency } from './handlers'

/**
 * Starts the mock API in the browser. URL switches (kept for the session):
 * - `?mock=fresh`     first-run state: setup wizard, no users signed in
 * - `?mock=signed-out` existing instance, nobody signed in
 * - `?mock=demo`     reset the data, signed in as the demo owner
 * - `?mock_scale=20`  multiply the number of issues (20 ≈ 13k issues)
 * - `?mock_latency=400` add latency to every request (ms)
 */
export async function startMockWorker() {
  const url = new URL(location.href)
  const mode = url.searchParams.get('mock')
  const scaleParam = url.searchParams.get('mock_scale')
  const latencyParam = url.searchParams.get('mock_latency')
  let scale = Number(sessionStorage.getItem('rupta-mock-scale') ?? 1)
  if (scaleParam) {
    scale = Number(scaleParam) || 1
    sessionStorage.setItem('rupta-mock-scale', String(scale))
  }
  if (mode === 'fresh') resetMock({ auth: { setupDone: false, userId: null }, scale })
  else if (mode === 'signed-out') resetMock({ auth: { setupDone: true, userId: null }, scale })
  else if (mode === 'demo') resetMock({ auth: { setupDone: true, userId: '1' }, scale })
  else if (scale !== 1) resetMock({ auth: JSON.parse(localStorage.getItem('rupta-mock-auth') ?? '{}'), scale })
  const latency = latencyParam ?? sessionStorage.getItem('rupta-mock-latency')
  if (latencyParam) sessionStorage.setItem('rupta-mock-latency', latencyParam)
  setMockLatency(latency ? Number(latency) : 120)

  if (mode || scaleParam || latencyParam) {
    ;['mock', 'mock_scale', 'mock_latency'].forEach((k) => url.searchParams.delete(k))
    history.replaceState(null, '', url.toString())
  }

  const worker = setupWorker(...handlers)
  await worker.start({ onUnhandledFrame: 'bypass', quiet: true, serviceWorker: { url: '/mockServiceWorker.js' } })
}
