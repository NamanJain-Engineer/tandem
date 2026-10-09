import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";
import { defineConfig } from "vite";

// The browser demo (vibezeo.com/demo/app/): the same interface against a
// simulated core, built into the website's static files.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: "/demo/app/",
  build: {
    target: "es2022",
    sourcemap: false,
    chunkSizeWarningLimit: 4096,
    outDir: resolve(__dirname, "../web/public/demo/app"),
    emptyOutDir: true,
    rollupOptions: { input: resolve(__dirname, "demo.html") },
  },
});
