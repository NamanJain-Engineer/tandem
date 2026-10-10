// Desktop E2E runner (MILESTONES §1): builds the UI and a debug app that
// serves the built assets, creates a temp TANDEM_HOME, sets the test env,
// starts the API (test routes, mock billing) and the web app if they aren't
// up, then runs WebdriverIO against tauri-driver. Run under a display and a
// session bus (single-instance uses DBus on Linux):
//   dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" pnpm --filter desktop e2e -- --spec 'e2e/specs/m2-*.e2e.ts'
// After the run it enforces the M2 checks: no secret patterns in the logs and
// no analytics or crash hosts in the network log (telemetry is off).
import { execFileSync, spawnSync, spawn, type ChildProcess } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Launcher } from "@wdio/cli";

const desktop = new URL("..", import.meta.url).pathname;
const root = join(desktop, "../..");

const args = process.argv.slice(2).filter((a) => a !== "--");
const specIndex = args.indexOf("--spec");
// `pnpm --filter desktop perf -- --suite <name>` runs e2e/perf/<name>.perf.ts
// (MILESTONES §1); results land in perf/results/<milestone>.json.
const suiteIndex = args.indexOf("--suite");
const suite = suiteIndex >= 0 ? args[suiteIndex + 1] : undefined;
const spec = suite ? `e2e/perf/${suite}.perf.ts` : specIndex >= 0 ? args[specIndex + 1] : undefined;

const API = "http://127.0.0.1:8787";
const WEB = "http://localhost:3000";
const FEED = "http://127.0.0.1:8790";
/** The browser's fixture pages (scripts/serve-fixtures.ts, E2E-M9-3). */
const FIXTURES = "http://127.0.0.1:5173";

async function up(url: string): Promise<boolean> {
  try {
    return (await fetch(url)).ok;
  } catch {
    return false;
  }
}

async function waitUp(url: string, seconds: number): Promise<void> {
  for (let i = 0; i < seconds * 2 && !(await up(url)); i++) await new Promise((r) => setTimeout(r, 500));
  if (!(await up(url))) throw new Error(`${url} didn't come up`);
}

function startDetached(cmd: string, cmdArgs: string[], env: NodeJS.ProcessEnv): ChildProcess {
  return spawn(cmd, cmdArgs, { cwd: root, stdio: "ignore", detached: true, env: { ...process.env, ...env } });
}

function stop(child?: ChildProcess) {
  if (!child?.pid) return;
  try {
    process.kill(-child.pid, "SIGTERM");
  } catch {
    // already exited
  }
}

/** The M2 exit checks over the run's data folder. */
function checkArtifacts(home: string): string[] {
  const problems: string[] = [];
  const logs = join(home, "logs");
  if (existsSync(logs)) {
    for (const file of readdirSync(logs)) {
      const body = readFileSync(join(logs, file), "utf8");
      if (/sk-[A-Za-z0-9]{20}|ghp_[A-Za-z0-9]{20}/.test(body)) problems.push(`secret pattern in logs/${file}`);
    }
  }
  const net = join(home, "net.log");
  if (existsSync(net) && /posthog|sentry/.test(readFileSync(net, "utf8"))) problems.push("analytics or crash host in net.log");
  return problems;
}

function startKeyring(): void {
  if (!process.env.DBUS_SESSION_BUS_ADDRESS) {
    const address = execFileSync("dbus-daemon", ["--session", "--fork", "--print-address=1"], { encoding: "utf8" }).trim().split("\n")[0];
    if (address) process.env.DBUS_SESSION_BUS_ADDRESS = address;
  }
  const r = spawnSync("gnome-keyring-daemon", ["--unlock", "--components=secrets", "--daemonize"], {
    input: "\n",
    encoding: "utf8",
    env: process.env,
  });
  if (r.status !== 0) throw new Error(`gnome-keyring-daemon didn't start (apt-get install gnome-keyring): ${r.stderr}`);
  for (const line of r.stdout.split("\n")) {
    const [k, v] = line.split("=");
    if (k && v) process.env[k] = v;
  }
}

async function main() {
  if (!process.env.TANDEM_E2E_SKIP_BUILD) {
    execFileSync("pnpm", ["build"], { cwd: desktop, stdio: "inherit" });
    // Perf suites measure an optimised build (the `perf` profile keeps the test harness).
    const cargoProfile = suite && process.env.TANDEM_PERF_BUILD !== "debug" ? ["--profile", "perf"] : [];
    execFileSync("cargo", ["build", "-p", "tandem-desktop", "--features", "custom-protocol,test-vendors", ...cargoProfile], { cwd: root, stdio: "inherit" });
    // The status hook and the fake engines (ARCHITECTURE §6.12).
    execFileSync("cargo", ["build", "-p", "tandem-engines", "--features", "fixtures", "--bins"], { cwd: root, stdio: "inherit" });
    execFileSync("cargo", ["build", "-p", "tandem-tasks", "--bins"], { cwd: root, stdio: "inherit" });
  }
  const home = mkdtempSync(join(tmpdir(), "tandem-e2e-"));
  const results = join(root, "test-results");
  mkdirSync(results, { recursive: true });
  writeFileSync(join(results, "e2e-last-home"), home);
  console.log(`E2E_LAST_HOME=${home}`);

  // The app opens URLs through this script (TANDEM_OPEN_URL_CMD).
  const opener = join(home, "open-url.sh");
  writeFileSync(
    opener,
    `#!/bin/sh\nexec "${join(root, "node_modules/.bin/tsx")}" "${join(desktop, "e2e/helpers/browser-signin.ts")}" "$1" >>"${join(home, "open-url.log")}" 2>&1\n`,
  );
  chmodSync(opener, 0o755);

  // Login shells read this HOME's profile, which puts the test engines first
  // on PATH, so an agent pane never starts a real engine installed on the
  // machine (ARCHITECTURE §17.4 "Fake CLI"). Only `claude` is installed by
  // default: a link to `fake-engine`, which behaves by the name it runs as.
  const fakes = join(home, "fakebin");
  mkdirSync(fakes, { recursive: true });
  const fakeEngine = join(root, "target/debug/fake-engine");
  symlinkSync(fakeEngine, join(fakes, "claude"));
  const zsh = ["/usr/bin/zsh", "/bin/zsh"].find((p) => existsSync(p));
  if (!zsh) console.warn("zsh not found: the shell specs need it (apt-get install zsh)");
  const userHome = join(home, "home");
  mkdirSync(userHome, { recursive: true });
  // The app's PATH: system folders and node (the scripted browser opener
  // needs it), but none of the engines installed on this machine.
  const nodeBin = join(home, "nodebin");
  mkdirSync(nodeBin, { recursive: true });
  symlinkSync(process.execPath, join(nodeBin, "node"));
  const appPath = [nodeBin, "/usr/local/sbin", "/usr/local/bin", "/usr/sbin", "/usr/bin", "/sbin", "/bin"].join(":");
  // TANDEM_* variables never reach panes from the app; the profile sets the
  // test-only ones for the shells and engines started in them.
  const profile = [
    `export PATH="${fakes}:$PATH"`,
    `export TANDEM_FAKE_ENGINE_LOG="${join(home, "fake-engine.log")}"`,
    `export TANDEM_HOOK_LOG="${join(home, "hook.log")}"`,
    // `empty` in this file makes the fake engines' handoff briefs empty (E2E-M6-5).
    `export TANDEM_FAKE_BRIEF="${join(home, "brief.txt")}"`,
    // `<engine> <version>` lines change what a fake engine reports (E2E-M7-5).
    `export TANDEM_FAKE_VERSION="${join(home, "fake-version.txt")}"`,
    // `<engine> <model>,<model>` (or `signed-out`) lines: an ACP agent's models (E2E-M7-12).
    `export TANDEM_FAKE_MODELS="${join(home, "fake-models.txt")}"`,
    "",
  ].join("\n");
  for (const f of [".bash_profile", ".profile", ".zprofile"]) writeFileSync(join(userHome, f), profile);

  Object.assign(process.env, {
    HOME: userHome,
    TANDEM_HOME: home,
    TANDEM_FAKE_KEYCHAIN: "1",
    TANDEM_E2E: "1",
    // TANDEM_PERF_BUILD=debug runs a suite on the debug build (quick
    // iterations; results record the build).
    TANDEM_APP_BINARY: join(root, suite && process.env.TANDEM_PERF_BUILD !== "debug" ? "target/perf/tandem" : "target/debug/tandem"),
    TANDEM_API_URL: API,
    TANDEM_WEB_URL: WEB,
    TANDEM_OPEN_URL_CMD: opener,
    TANDEM_NET_LOG: join(home, "net.log"),
    TANDEM_FAKE_PICK_FOLDER: join(home, "pick-folder.txt"),
    // The native "Discard worktree?" confirm answers from this file (debug builds).
    TANDEM_FAKE_CONFIRM: join(home, "confirm.txt"),
    // Fails the nth worktree of a launch (debug builds; E2E-M6-3).
    TANDEM_FAKE_LANE_FAIL: join(home, "lane-fail.txt"),
    // Headless engine runs aren't panes and don't read the profile; debug
    // builds hand them these (E2E-M6-5).
    TANDEM_FAKE_BRIEF: join(home, "brief.txt"),
    TANDEM_FAKE_ENGINE_LOG: join(home, "fake-engine.log"),
    TANDEM_FAKE_VERSION: join(home, "fake-version.txt"),
    TANDEM_FAKE_MODELS: join(home, "fake-models.txt"),
    // The fake `codex app-server`'s usage windows, e.g. "40,85" (E2E-M6-8).
    TANDEM_FAKE_USAGE: join(home, "usage.txt"),
    // Where the usage share card "saves" (debug builds; F-AC-07).
    TANDEM_FAKE_SAVE_PATH: join(home, "save-path.txt"),
    TANDEM_HOOK_BIN: join(root, "target/debug/tandem-hook"),
    TANDEM_TASKS_BIN: join(root, "target/debug/tandem-tasks"),
    TANDEM_SPAWN_LOG: join(home, "spawn.log"),
    // The local update feed (scripts/mock-update-feed.ts, E2E-M5-7); it
    // answers "nothing new" unless a spec picks a scenario.
    TANDEM_UPDATE_URL: `${FEED}/v1/updates`,
    // Scrollback snapshots often enough that a kill -9 leaves some (E2E-M5-3).
    TANDEM_SNAPSHOT_EVERY_MS: "1500",
    TANDEM_APP_PATH: appPath,
    TANDEM_FAKE_ENGINE_BIN: fakeEngine,
    // Detection looks only at this run's folders (no npm, no /usr/local/bin).
    TANDEM_DETECT_SYSTEM_DIRS: "",
    TANDEM_DETECT_NO_NPM: "1",
    ...(suite ? { TANDEM_MOCHA_TIMEOUT: "900000" } : {}),
    PLAYWRIGHT_CHROMIUM: process.env.PLAYWRIGHT_CHROMIUM ?? (existsSync("/opt/pw-browsers/chromium") ? "/opt/pw-browsers/chromium" : ""),
    PATH: process.env.PATH ?? "",
    // The specs are written for a zsh login shell (E2E-M4-1 writes ~/.zshrc),
    // and the runner's own login shell may put a real engine on PATH through
    // /etc/profile.d: the app probes zsh, whatever shell runs this script.
    ...(zsh ? { SHELL: zsh } : {}),
  });
  // The startup suite relaunches signed in (16 live shells need a paid plan),
  // so it uses the real Secret Service keychain: an unlocked gnome-keyring on
  // a private session bus, inside this run's HOME.
  if (suite === "startup") {
    delete process.env.TANDEM_FAKE_KEYCHAIN;
    startKeyring();
  }

  // Maximize and placement need a window manager under Xvfb.
  let wm: ChildProcess | undefined;
  if (process.env.DISPLAY && !process.env.TANDEM_E2E_NO_WM) {
    wm = spawn("openbox", [], { stdio: "ignore", detached: true });
    wm.on("error", () => console.warn("openbox not found; maximize checks need a window manager"));
    await new Promise((r) => setTimeout(r, 500));
  }

  let api: ChildProcess | undefined;
  let web: ChildProcess | undefined;
  if (!(await up(`${API}/v1/health`))) {
    api = startDetached("pnpm", ["--filter", "api", "start"], {
      TANDEM_API_TEST_ROUTES: "1",
      BILLING_PROVIDER: "mock",
      WEB_PUBLIC_URL: WEB,
      LOG_LEVEL: "warn",
    });
    await waitUp(`${API}/v1/health`, 60);
  }
  if (!(await up(`${WEB}/api/health`))) {
    web = startDetached("pnpm", ["--filter", "web", "dev"], { API_INTERNAL_URL: API });
    await waitUp(`${WEB}/api/health`, 120);
  }
  let feed: ChildProcess | undefined;
  if (!(await up(`${FEED}/health`))) {
    feed = startDetached(join(root, "node_modules/.bin/tsx"), ["scripts/mock-update-feed.ts", "--port", "8790", "--scenario", "none"], {});
    await waitUp(`${FEED}/health`, 30);
  } else {
    await fetch(`${FEED}/__scenario`, { method: "POST", body: JSON.stringify({ scenario: "none" }) });
  }

  let fixtures: ChildProcess | undefined;
  const fixturesUp = async () => (await fetch(`${FIXTURES}/health`).then((r) => r.text()).catch(() => "")) === "ok";
  if (!(await fixturesUp())) {
    fixtures = startDetached(join(root, "node_modules/.bin/tsx"), ["scripts/serve-fixtures.ts", "--port", "5173"], {});
    for (let i = 0; i < 60 && !(await fixturesUp()); i++) await new Promise((r) => setTimeout(r, 500));
    if (!(await fixturesUp())) throw new Error(`${FIXTURES} didn't come up (is something else on :5173?)`);
  }

  let code = 1;
  try {
    const launcher = new Launcher(join(desktop, "e2e/wdio.conf.ts"), spec ? { spec: [join(desktop, spec)] } : {});
    code = (await launcher.run()) ?? 1;
  } finally {
    stop(api);
    stop(web);
    stop(feed);
    stop(fixtures);
    stop(wm);
  }
  const problems = checkArtifacts(home);
  for (const p of problems) console.error(`E2E check failed: ${p}`);
  process.exit(problems.length ? 1 : code);
}

void main();
