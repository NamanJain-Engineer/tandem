// Desktop E2E runner (MILESTONES §1): builds the UI and a debug app that
// serves the built assets, creates a temp TANDEM_HOME, sets the test env,
// prepends the fake-engine dir to PATH, starts the API if it isn't up, then
// runs WebdriverIO against tauri-driver. Run under `xvfb-run -a`.
//   pnpm --filter desktop e2e -- --spec 'e2e/specs/m0-*.e2e.ts'
import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Launcher } from "@wdio/cli";

const desktop = new URL("..", import.meta.url).pathname;
const root = join(desktop, "../..");

const args = process.argv.slice(2).filter((a) => a !== "--");
const specIndex = args.indexOf("--spec");
const spec = specIndex >= 0 ? args[specIndex + 1] : undefined;

async function apiUp(): Promise<boolean> {
  try {
    const res = await fetch("http://127.0.0.1:8787/v1/health");
    return res.ok;
  } catch {
    return false;
  }
}

async function main() {
  if (!process.env.TANDEM_E2E_SKIP_BUILD) {
    execFileSync("pnpm", ["build"], { cwd: desktop, stdio: "inherit" });
    execFileSync("cargo", ["build", "-p", "tandem-desktop", "--features", "custom-protocol"], {
      cwd: root,
      stdio: "inherit",
    });
  }
  const home = mkdtempSync(join(tmpdir(), "tandem-e2e-"));
  process.env.TANDEM_HOME = home;
  process.env.TANDEM_FAKE_KEYCHAIN = "1";
  process.env.TANDEM_E2E = "1";
  process.env.TANDEM_APP_BINARY = join(root, "target/debug/tandem");
  process.env.PATH = `${join(root, "crates/tandem-engines/tests/fixtures/bin")}:${process.env.PATH ?? ""}`;

  let api: ReturnType<typeof spawn> | undefined;
  if (!(await apiUp())) {
    api = spawn("pnpm", ["dev", "--api-only", "--no-infra"], { cwd: root, stdio: "ignore", detached: true });
    for (let i = 0; i < 60 && !(await apiUp()); i++) await new Promise((r) => setTimeout(r, 500));
  }

  let code = 1;
  try {
    const launcher = new Launcher(
      join(desktop, "e2e/wdio.conf.ts"),
      spec ? { spec: [join(desktop, spec)] } : {},
    );
    code = (await launcher.run()) ?? 1;
  } finally {
    if (api?.pid) {
      try {
        process.kill(-api.pid, "SIGTERM");
      } catch {
        // already exited
      }
    }
  }
  process.exit(code);
}

void main();
