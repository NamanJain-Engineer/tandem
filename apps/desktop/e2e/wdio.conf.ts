import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const WIN = process.platform === "win32";
const desktop = fileURLToPath(new URL("..", import.meta.url));
/** The app inherits the driver's environment. Give it what a desktop
 * session has, not the test runner's (CI and agent variables would reach
 * every pane), and the run's minimal PATH (no engines installed on this
 * machine), see run.ts. */
function appEnv(): NodeJS.ProcessEnv {
  const keep = /^(HOME|USER|LOGNAME|SHELL|LANG|LC_[A-Z]+|TZ|TMPDIR|DISPLAY|XAUTHORITY|WAYLAND_DISPLAY|DBUS_SESSION_BUS_ADDRESS|XDG_[A-Z_]+|TANDEM_[A-Z0-9_]+|RUST_BACKTRACE|RUST_LOG|WEBKIT_[A-Z_]+|GDK_[A-Z_]+|LIBGL_[A-Z_]+|NO_AT_BRIDGE|PLAYWRIGHT_[A-Z_]+)$/;
  const env: NodeJS.ProcessEnv = {};
  // What a Windows session needs (WebView2 doesn't start without SystemRoot).
  const keepWindows = /^(SYSTEMROOT|WINDIR|COMSPEC|PATHEXT|TEMP|TMP|USERPROFILE|USERNAME|USERDOMAIN|APPDATA|LOCALAPPDATA|PROGRAMDATA|PROGRAMFILES|PROGRAMFILES\(X86\)|COMMONPROGRAMFILES|HOMEDRIVE|HOMEPATH|SYSTEMDRIVE|COMPUTERNAME|OS|PROCESSOR_ARCHITECTURE|NUMBER_OF_PROCESSORS|WEBVIEW2_[A-Z_]+)$/i;
  for (const [k, v] of Object.entries(process.env)) if (keep.test(k) || (WIN && keepWindows.test(k))) env[k] = v;
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
      const bin = WIN
        ? (execFileSync("where", ["tauri-driver"], { encoding: "utf8" }).split(/\r?\n/)[0] ?? "").trim()
        : execFileSync("sh", ["-c", "command -v tauri-driver"], { encoding: "utf8" }).trim();
      // Windows: tauri-driver drives WebView2 through msedgedriver, which
      // must match the installed WebView2 (D-1714).
      const native = WIN && process.env.TANDEM_NATIVE_DRIVER ? ["--native-driver", process.env.TANDEM_NATIVE_DRIVER] : [];
      driver = spawn(bin, native, { stdio: [null, process.stdout, process.stderr], env: appEnv() });
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
