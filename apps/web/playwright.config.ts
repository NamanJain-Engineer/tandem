import { defineConfig, devices } from "@playwright/test";

// Web E2E (MILESTONES §1). Needs `pnpm infra:up`, `pnpm db:migrate` and `pnpm db:seed`.
const apiEnv = {
  TANDEM_API_TEST_ROUTES: "1",
  BILLING_PROVIDER: "mock",
  WEB_PUBLIC_URL: "http://localhost:3000",
  // The test identity provider's page lives on the API (F-BL-06 P2 SSO).
  API_PUBLIC_URL: "http://127.0.0.1:8787",
  LOG_LEVEL: "warn",
};

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  workers: 1,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:3000",
    trace: "retain-on-failure",
    ...devices["Desktop Chrome"],
    // Use a preinstalled Chromium when Playwright's own revision isn't downloaded.
    launchOptions: process.env.PLAYWRIGHT_CHROMIUM ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM } : {},
  },
  webServer: [
    {
      command: "pnpm --filter api start",
      url: "http://127.0.0.1:8787/v1/health",
      reuseExistingServer: true,
      env: apiEnv,
      cwd: "../..",
      timeout: 60_000,
    },
    {
      command: "pnpm --filter web dev",
      url: "http://localhost:3000/api/health",
      reuseExistingServer: true,
      cwd: "../..",
      // No telemetry from a test run (D-2625).
      env: { API_INTERNAL_URL: "http://127.0.0.1:8787", NEXT_TELEMETRY_DISABLED: "1" },
      timeout: 120_000,
    },
  ],
});
