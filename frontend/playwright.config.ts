import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
  testDir: 'e2e',
  fullyParallel: false,
  workers: 1,
  expect: { timeout: 15_000 },
  use: {
    baseURL: 'http://127.0.0.1:5190',
    locale: 'ja-JP',
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'], locale: 'ja-JP' } }],
  webServer: [
    { command: 'bun e2e/fake-fxtwitter.ts', url: 'http://127.0.0.1:3191/health' },
    { command: 'sh e2e/start-backend.sh', url: 'http://127.0.0.1:3190/api/session', timeout: 300_000 },
    {
      command: 'bunx vite --host 127.0.0.1 --port 5190 --strictPort',
      url: 'http://127.0.0.1:5190',
      env: { ARCMAN_BACKEND_URL: 'http://127.0.0.1:3190' },
    },
  ],
})
