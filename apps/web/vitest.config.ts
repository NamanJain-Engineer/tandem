import { defineConfig } from "vitest/config";

export default defineConfig({
  esbuild: { jsx: "automatic" },
  test: { environment: "node", include: ["app/**/*.test.ts", "app/**/*.test.tsx", "lib/**/*.test.ts"] },
});
