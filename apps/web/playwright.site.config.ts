import { defineConfig, devices } from "@playwright/test";

// The static site as Vercel serves it (`pnpm build:site` → out/), for the
// home page's interactive demo (docs/FEATURES_TO_BUILD.md, D-128).
const PORT = 4322;
export default defineConfig({
  testDir: "./e2e-site",
  fullyParallel: false,
  workers: 1,
  timeout: 90_000,
  expect: { timeout: 15_000 },
  reporter: [["list"]],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: "retain-on-failure",
    ...devices["Desktop Chrome"],
    launchOptions: process.env.PLAYWRIGHT_CHROMIUM ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM } : {},
  },
  webServer: {
    command: `python3 -m http.server ${PORT} --bind 127.0.0.1 --directory out`,
    url: `http://127.0.0.1:${PORT}/`,
    reuseExistingServer: true,
    stdout: "ignore",
    stderr: "ignore",
    timeout: 30_000,
  },
});
