import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

export default defineConfig({
  esbuild: { jsx: "automatic" },
  resolve: {
    alias: { "next/font/local": fileURLToPath(new URL("./test/next-font-local.ts", import.meta.url)) },
  },
  test: { environment: "node", include: ["app/**/*.test.ts", "app/**/*.test.tsx", "lib/**/*.test.ts"] },
});
