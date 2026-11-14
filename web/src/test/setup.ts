import '@testing-library/jest-dom/vitest'
import { cleanup } from '@testing-library/react'
import { afterAll, afterEach, beforeAll, vi } from 'vitest'
import { resetMock, setMockLatency } from '@/mocks/handlers'
import { server } from '@/mocks/node'

// jsdom gaps used by Radix and the virtualiser
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver
window.matchMedia ??= ((query: string) => ({
  matches: false,
  media: query,
  onchange: null,
  addEventListener: () => {},
  removeEventListener: () => {},
  addListener: () => {},
  removeListener: () => {},
  dispatchEvent: () => false,
})) as unknown as typeof window.matchMedia
Element.prototype.scrollIntoView ??= vi.fn()
// jsdom has no layout; give elements a size so the virtualised list renders rows.
Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, get: () => 900 })
Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, get: () => 1200 })
Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => 900 })
Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, get: () => 1200 })
Element.prototype.hasPointerCapture ??= () => false
Element.prototype.releasePointerCapture ??= () => {}

beforeAll(() => {
  setMockLatency(0)
  server.listen({ onUnhandledRequest: 'error' })
})
afterEach(() => {
  cleanup()
  server.resetHandlers()
  resetMock({ latency: 0 })
  localStorage.clear()
})
afterAll(() => server.close())
