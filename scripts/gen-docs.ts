// `pnpm gen:docs` (F-WB-02): writes what the docs render from code, so it
// can't drift: packages/protocol/settings.json from the Rust settings schema.
// The shortcuts (shortcuts.json, gen:shortcuts), the engine registry
// (crates/tandem-engines/registry.json) and the platform matrix
// (apps/web/lib/release.ts) are read where they live. `pnpm gen:protocol`
// runs this too, so the gate's drift check covers it.
import { execFileSync } from "node:child_process";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const out = join(root, "packages/protocol/settings.json");

execFileSync("cargo", ["test", "-p", "tandem-core", "--quiet", "--lib", "export_settings"], {
  cwd: root,
  stdio: ["ignore", "ignore", "inherit"],
  env: { ...process.env, TANDEM_SETTINGS_OUT: out },
});
console.log(`settings: ${out}`);
