import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { fileURLToPath, URL } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import { tanstackRouter } from '@tanstack/router-plugin/vite'
import react from '@vitejs/plugin-react'
import { defineConfig, type Plugin } from 'vite'

const backend = process.env.RUPTA_BACKEND ?? 'http://localhost:8080'
const mock = process.env.VITE_MOCK === '1'

/**
 * In mock mode, serves (dev) or emits (build) the MSW service worker from
 * the msw package. Production builds never contain it.
 */
function mockServiceWorker(): Plugin {
  const file = createRequire(import.meta.url).resolve('msw/mockServiceWorker.js')
  return {
    name: 'rupta-mock-service-worker',
    configureServer(server) {
      server.middlewares.use('/mockServiceWorker.js', (_req, res) => {
        res.setHeader('Content-Type', 'text/javascript')
        res.end(readFileSync(file))
      })
    },
    generateBundle() {
      this.emitFile({ type: 'asset', fileName: 'mockServiceWorker.js', source: readFileSync(file) })
    },
  }
}

export default defineConfig({
  plugins: [tanstackRouter({ target: 'react', autoCodeSplitting: true, quoteStyle: 'single' }), react(), tailwindcss(), ...(mock ? [mockServiceWorker()] : [])],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    proxy: {
      '/api': { target: backend, changeOrigin: false },
      '/auth': { target: backend, changeOrigin: false },
    },
  },
  build: {
    target: 'es2022',
    sourcemap: false,
    chunkSizeWarningLimit: 600,
  },
})
