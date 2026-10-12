import { defineConfig } from "vitest/config";

// The app's logic (lib/) runs in Node; screens are thin and React Native
// isn't loaded here.
export default defineConfig({
  test: { environment: "node", include: ["lib/**/*.test.ts", "ui/**/*.test.ts", "plugins/**/*.test.ts", "*.test.ts"] },
});
