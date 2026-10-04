import { defineConfig } from "vitest/config";

// API tests run against the compose Postgres and Redis (`pnpm infra:up`).
export default defineConfig({
  test: {
    environment: "node",
    env: { LOG_LEVEL: "silent" },
    include: ["src/**/*.test.ts", "test/**/*.test.ts"],
    globalSetup: ["./test/globalSetup.ts"],
    fileParallelism: false,
    testTimeout: 20_000,
    hookTimeout: 30_000,
  },
});
