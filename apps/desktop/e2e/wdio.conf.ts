import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const desktop = new URL("..", import.meta.url).pathname;
/** The app inherits the driver's environment. Give it what a desktop
 * session has, not the test runner's (CI and agent variables would reach
 * every pane), and the run's minimal PATH (no engines installed on this
 * machine), see run.ts. */
function appEnv(): NodeJS.ProcessEnv {
  const keep = /^(HOME|USER|LOGNAME|SHELL|LANG|LC_[A-Z]+|TZ|TMPDIR|DISPLAY|XAUTHORITY|WAYLAND_DISPLAY|DBUS_SESSION_BUS_ADDRESS|XDG_[A-Z_]+|TANDEM_[A-Z0-9_]+|RUST_BACKTRACE|RUST_LOG|WEBKIT_[A-Z_]+|GDK_[A-Z_]+|LIBGL_[A-Z_]+|NO_AT_BRIDGE|PLAYWRIGHT_[A-Z_]+)$/;
  const env: NodeJS.ProcessEnv = {};
  for (const [k, v] of Object.entries(process.env)) if (keep.test(k)) env[k] = v;
  env.PATH = process.env.TANDEM_APP_PATH ?? process.env.PATH;
  return env;
}

let driver: ChildProcess | undefined;

export const config: WebdriverIO.Config = {
  runner: "local",
  hostname: "127.0.0.1",
  port: 4444,
  specs: [join(desktop, "e2e/specs/**/*.e2e.ts")],
  maxInstances: 1,
  capabilities: [
    {
      // @ts-expect-error tauri-driver specific capability
      "tauri:options": { application: process.env.TANDEM_APP_BINARY },
      // WebKitWebDriver speaks classic WebDriver only (no BiDi).
      "wdio:enforceWebDriverClassic": true,
    },
  ],
  logLevel: "warn",
  framework: "mocha",
  reporters: ["spec"],
  // Perf suites wait out heavy loads; run.ts raises this for them.
  mochaOpts: { ui: "bdd", timeout: Number(process.env.TANDEM_MOCHA_TIMEOUT ?? 60_000) },
  onPrepare: () =>
    new Promise<void>((resolve) => {
      // Found on the test runner's PATH; the app then gets the minimal one.
      const bin = execFileSync("sh", ["-c", "command -v tauri-driver"], { encoding: "utf8" }).trim();
      driver = spawn(bin, [], { stdio: [null, process.stdout, process.stderr], env: appEnv() });
      setTimeout(resolve, 1500);
    }),
  onComplete: () => {
    driver?.kill();
  },
  // A screenshot of the app for each failed test, in test-results/.
  afterTest: async (test, _context, result) => {
    if (result.passed) return;
    const name = `${test.parent} ${test.title}`.replace(/[^\w.-]+/g, "_").slice(0, 120);
    mkdirSync(join(desktop, "../../test-results/e2e"), { recursive: true });
    await browser.saveScreenshot(join(desktop, `../../test-results/e2e/${name}.png`)).catch(() => undefined);
    const state = await browser
      .execute(() => (window as unknown as { __tandemTest?: { debug: () => unknown } }).__tandemTest?.debug() ?? null)
      .catch((e: unknown) => String(e));
    writeFileSync(join(desktop, `../../test-results/e2e/${name}.json`), JSON.stringify(state, null, 2));
  },
};
