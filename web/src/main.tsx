import '@fontsource-variable/geist'
import '@fontsource-variable/geist-mono'
import './index.css'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './app'

async function boot() {
  if (import.meta.env.VITE_MOCK === '1') {
    const { startMockWorker } = await import('./mocks/browser')
    await startMockWorker()
  }
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  )
}

void boot()
