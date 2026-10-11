import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// `pnpm --filter web build:remote-lan`: the LAN web app (F-RM-04) into the
// desktop's resources, served by the desktop at /remote.
export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  base: "/remote/",
  plugins: [react()],
  build: {
    outDir: fileURLToPath(new URL("../../desktop/src-tauri/resources/remote-web", import.meta.url)),
    emptyOutDir: false,
    assetsDir: "assets",
    sourcemap: false,
    target: "es2020",
  },
});
