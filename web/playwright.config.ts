import { existsSync } from 'node:fs'
import { defineConfig, devices } from '@playwright/test'

// Use a preinstalled Chromium when the bundled one is not downloaded.
const localChrome = ['/opt/pw-browsers/chromium-1243/chrome-linux64/chrome'].find((p) => existsSync(p))
const executablePath = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ?? localChrome
const port = 4174

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  workers: process.env.CI ? 2 : undefined,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  timeout: 60_000,
  use: {
    baseURL: `http://localhost:${port}`,
    trace: 'retain-on-failure',
    launchOptions: executablePath ? { executablePath } : undefined,
  },
  projects: [
    { name: 'e2e', testIgnore: /screenshots\.spec\.ts/, use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } } },
    { name: 'screenshots', testMatch: /screenshots\.spec\.ts/, use: { ...devices['Desktop Chrome'], deviceScaleFactor: 2 } },
  ],
  // The e2e suite runs the production build in mock mode (no backend needed).
  webServer: {
    command: `pnpm build:mock && pnpm exec vite preview --outDir dist-mock --port ${port} --strictPort`,
    url: `http://localhost:${port}`,
    reuseExistingServer: !process.env.CI,
    timeout: 180_000,
  },
})
