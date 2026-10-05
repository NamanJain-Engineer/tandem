// Writes packages/protocol/shortcuts.json from the Rust command registry
// (F-SH-09). The UI's shortcut dispatcher and palette read it, and the docs
// render the shortcut table from it. `pnpm gen:protocol` runs this too, so the
// gate's drift check covers it.
import { execFileSync } from "node:child_process";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const out = join(root, "packages/protocol/shortcuts.json");

execFileSync("cargo", ["test", "-p", "tandem-core", "--quiet", "--lib", "export_shortcuts"], {
  cwd: root,
  stdio: ["ignore", "ignore", "inherit"],
  env: { ...process.env, TANDEM_SHORTCUTS_OUT: out },
});
console.log(`shortcuts: ${out}`);
