// Desktop E2E runner (MILESTONES §1): builds the UI and a debug app that
// serves the built assets, creates a temp TANDEM_HOME, sets the test env,
// starts the API (test routes, mock billing) and the web app if they aren't
// up, then runs WebdriverIO against tauri-driver. Run under a display and a
// session bus (single-instance uses DBus on Linux):
//   dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" pnpm --filter desktop e2e -- --spec 'e2e/specs/m2-*.e2e.ts'
// After the run it enforces the M2 checks: no secret patterns in the logs and
// no analytics or crash hosts in the network log (telemetry is off).
//
// Windows (D-1714): the same runner drives the app through tauri-driver and
// msedgedriver (TANDEM_NATIVE_DRIVER, or msedgedriver on PATH).
// TANDEM_E2E_OFFLINE=1 skips the API, web app and mock servers, for the
// specs that don't need them (e2e/windows-smoke.txt; the Windows CI job).
import { execFileSync, spawnSync, spawn, type ChildProcess } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readFileSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { Launcher } from "@wdio/cli";
import { E2E_KNOWN_SENTENCE } from "./support/phone";

const WIN = process.platform === "win32";
/** Executables' suffix on this platform. */
const EXE = WIN ? ".exe" : "";
/** No API, web app or mock servers (TANDEM_E2E_OFFLINE=1). */
const OFFLINE = process.env.TANDEM_E2E_OFFLINE === "1";
const desktop = fileURLToPath(new URL("..", import.meta.url));
const root = join(desktop, "../..");

const args = process.argv.slice(2).filter((a) => a !== "--");
const specIndex = args.indexOf("--spec");
// `pnpm --filter desktop perf -- --suite <name>` runs e2e/perf/<name>.perf.ts
// (MILESTONES §1); results land in perf/results/<milestone>.json.
const suiteIndex = args.indexOf("--suite");
const suite = suiteIndex >= 0 ? args[suiteIndex + 1] : undefined;
const spec = suite ? `e2e/perf/${suite}.perf.ts` : specIndex >= 0 ? args[specIndex + 1] : undefined;
// `--spec-list <file>`: one spec path per line (`#` comments), e.g.
// e2e/windows-smoke.txt, the set the Windows CI job runs offline.
const listIndex = args.indexOf("--spec-list");
const listFile = listIndex >= 0 ? args[listIndex + 1] : undefined;
const specList =
  listFile !== undefined
    ? readFileSync(listFile, "utf8")
        .split("\n")
        .map((l) => l.replace(/#.*/, "").trim())
        .filter(Boolean)
    : undefined;

const API = "http://127.0.0.1:8787";
const WEB = "http://localhost:3000";
const FEED = "http://127.0.0.1:8790";
/** Dictate's models, served from the local cache (scripts/serve-models.ts, E2E-M8-8). */
const MODELS = "http://127.0.0.1:8792";
/** Fixed, so a server left running from an earlier run reads the same file. */
const MODELS_CONTROL = join(tmpdir(), "tandem-models-control.txt");
/** The real home and model cache, read before HOME points at the test's home. */
const REAL_HOME = homedir();
const MODEL_CACHE = process.env.TANDEM_MODEL_CACHE ?? join(REAL_HOME, ".cache", "tandem-models");
/** The browser's fixture pages (scripts/serve-fixtures.ts, E2E-M9-3). */
const FIXTURES = "http://127.0.0.1:5173";
/** The mock OAuth server and plugin upstreams (`mock-plugins`, D-954). */
const PLUGINS = "http://127.0.0.1:8791";
// Conductor's realtime voice provider stand-in (F-CN-01, D-1102).
const REALTIME = "http://127.0.0.1:8794";

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

function startDetached(
  cmd: string,
  cmdArgs: string[],
  env: NodeJS.ProcessEnv,
  logFile?: string,
): ChildProcess {
  const out = logFile ? openSync(logFile, "a") : "ignore";
  // `pnpm` and `tsx` are .cmd scripts on Windows, which need a shell.
  return spawn(cmd, cmdArgs, {
    cwd: root,
    stdio: ["ignore", out, out],
    detached: true,
    shell: WIN,
    env: { ...process.env, ...env },
  });
}

function stop(child?: ChildProcess) {
  if (!child?.pid) return;
  try {
    // Windows has no process groups: end the tree with taskkill.
    if (WIN) spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    else process.kill(-child.pid, "SIGTERM");
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
      if (/sk-[A-Za-z0-9]{20}|ghp_[A-Za-z0-9]{20}/.test(body))
        problems.push(`secret pattern in logs/${file}`);
    }
  }
  const apiLog = join(home, "api.log");
  if (existsSync(apiLog) && readFileSync(apiLog, "utf8").includes(E2E_KNOWN_SENTENCE)) {
    problems.push("the phone's plaintext reached the relay log (api.log)");
  }
  const net = join(home, "net.log");
  if (existsSync(net) && /posthog|sentry/.test(readFileSync(net, "utf8")))
    problems.push("analytics or crash host in net.log");
  return problems;
}

function startKeyring(): void {
  if (!process.env.DBUS_SESSION_BUS_ADDRESS) {
    const address = execFileSync("dbus-daemon", ["--session", "--fork", "--print-address=1"], {
      encoding: "utf8",
    })
      .trim()
      .split("\n")[0];
    if (address) process.env.DBUS_SESSION_BUS_ADDRESS = address;
  }
  const r = spawnSync("gnome-keyring-daemon", ["--unlock", "--components=secrets", "--daemonize"], {
    input: "\n",
    encoding: "utf8",
    env: process.env,
  });
  if (r.status !== 0)
    throw new Error(`gnome-keyring-daemon didn't start (apt-get install gnome-keyring): ${r.stderr}`);
  for (const line of r.stdout.split("\n")) {
    const [k, v] = line.split("=");
    if (k && v) process.env[k] = v;
  }
}

async function main() {
  // Remote workspaces (E2E-M14): the bundled tandem-host and the sshd
  // fixture (infra/docker-compose.yml, profile remote). Without Docker or
  // the musl target the m14 specs fail and say so; the others don't care.
  // No sshd fixture on Windows or offline (D-1714).
  const remote = !WIN && !OFFLINE && (!spec || spec.includes("m14"));
  const quiet = { cwd: root, stdio: "ignore" as const };
  if (!process.env.TANDEM_E2E_SKIP_BUILD) {
    // The helper first: the app's build copies resources/tandem-host into
    // target/, so a helper built after it isn't the one the app installs on
    // the host (the M14 specs then met the previous build's helper).
    if (remote && spawnSync("sh", ["scripts/build-tandem-host.sh"], quiet).status !== 0)
      console.warn(
        "E2E: couldn't build tandem-host (rustup target add x86_64-unknown-linux-musl; apt install musl-tools)",
      );
    execFileSync("pnpm", ["build"], { cwd: desktop, stdio: "inherit", shell: WIN });
    // Perf suites measure an optimised build (the `perf` profile keeps the test harness).
    const cargoProfile = suite && process.env.TANDEM_PERF_BUILD !== "debug" ? ["--profile", "perf"] : [];
    execFileSync(
      "cargo",
      ["build", "-p", "tandem-desktop", "--features", "custom-protocol,test-vendors", ...cargoProfile],
      { cwd: root, stdio: "inherit" },
    );
    // The mock authorization server and plugin upstreams (M12).
    execFileSync(
      "cargo",
      ["build", "-p", "tandem-gateway", "--features", "test-vendors", "--bin", "mock-plugins"],
      {
        cwd: root,
        stdio: "inherit",
      },
    );
    // The status hook and the fake engines (ARCHITECTURE §6.12).
    execFileSync("cargo", ["build", "-p", "tandem-engines", "--features", "fixtures", "--bins"], {
      cwd: root,
      stdio: "inherit",
    });
    execFileSync("cargo", ["build", "-p", "tandem-tasks", "--bins"], { cwd: root, stdio: "inherit" });
  }
  if (remote) {
    const up = spawnSync(
      "sh",
      [
        "-c",
        "bash scripts/gen-ssh-fixture.sh >/dev/null && docker compose -f infra/docker-compose.yml --profile remote up -d --wait sshd",
      ],
      quiet,
    );
    if (up.status !== 0)
      console.warn("E2E: the sshd fixture didn't start (Docker?); the m14 specs will fail");
  }
  const home = mkdtempSync(join(tmpdir(), "tandem-e2e-"));
  const results = join(root, "test-results");
  mkdirSync(results, { recursive: true });
  writeFileSync(join(results, "e2e-last-home"), home);
  console.log(`E2E_LAST_HOME=${home}`);

  // The app opens URLs through this script (TANDEM_OPEN_URL_CMD).
  const opener = join(home, WIN ? "open-url.cmd" : "open-url.sh");
  const signin = join(desktop, "e2e/helpers/browser-signin.ts");
  const log = join(home, "open-url.log");
  if (WIN) {
    writeFileSync(opener, `@"${join(root, "node_modules/.bin/tsx.cmd")}" "${signin}" %1 >>"${log}" 2>&1\r\n`);
  } else {
    writeFileSync(
      opener,
      `#!/bin/sh\nexec "${join(root, "node_modules/.bin/tsx")}" "${signin}" "$1" >>"${log}" 2>&1\n`,
    );
    chmodSync(opener, 0o755);
  }

  // Login shells read this HOME's profile, which puts the test engines first
  // on PATH, so an agent pane never starts a real engine installed on the
  // machine (ARCHITECTURE §17.4 "Fake CLI"). Only `claude` is installed by
  // default: a link to `fake-engine`, which behaves by the name it runs as.
  const fakes = join(home, "fakebin");
  mkdirSync(fakes, { recursive: true });
  const fakeEngine = join(root, `target/debug/fake-engine${EXE}`);
  // Windows: a copy (symlinks need Developer Mode).
  if (WIN) copyFileSync(fakeEngine, join(fakes, "claude.exe"));
  else symlinkSync(fakeEngine, join(fakes, "claude"));
  const zsh = ["/usr/bin/zsh", "/bin/zsh"].find((p) => existsSync(p));
  if (!zsh) console.warn("zsh not found: the shell specs need it (apt-get install zsh)");
  const userHome = join(home, "home");
  mkdirSync(userHome, { recursive: true });
  // The app's PATH: system folders and node (the scripted browser opener
  // needs it), but none of the engines installed on this machine.
  const nodeBin = WIN ? dirname(process.execPath) : join(home, "nodebin");
  if (!WIN) {
    mkdirSync(nodeBin, { recursive: true });
    symlinkSync(process.execPath, join(nodeBin, "node"));
  }
  // Recording `systemctl` and `crontab` for the background runner (F-RT-03,
  // E2E-M11-5): its unit files go under TANDEM_LAUNCHER_ROOT and nothing
  // touches this machine's real timers or crontab.
  const shims = join(home, "shims");
  // Windows routines go through schtasks, which no spec in the smoke set turns on.
  if (!WIN) {
    mkdirSync(shims, { recursive: true });
    writeFileSync(
      join(shims, "systemctl"),
      `#!/bin/sh\necho "$*" >>"${join(home, "systemctl.log")}"\nexit 0\n`,
    );
    writeFileSync(
      join(shims, "crontab"),
      `#!/bin/sh\nf="${join(home, "crontab.txt")}"\nif [ "$1" = "-l" ]; then [ -f "$f" ] && exec cat "$f"; exit 1; fi\ncat >"$f"\n`,
    );
    chmodSync(join(shims, "systemctl"), 0o755);
    chmodSync(join(shims, "crontab"), 0o755);
  }
  // Windows has no login profile to put the fakes first, so PATH does.
  const sysRoot = process.env.SystemRoot ?? "C:\\Windows";
  const appPath = WIN
    ? [fakes, nodeBin, join(sysRoot, "System32"), sysRoot, join(sysRoot, "System32", "WindowsPowerShell", "v1.0")].join(";")
    : [shims, nodeBin, "/usr/local/sbin", "/usr/local/bin", "/usr/sbin", "/usr/bin", "/sbin", "/bin"].join(":");
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

  writeFileSync(join(home, "ssh_config"), "");
  Object.assign(process.env, {
    HOME: userHome,
    TANDEM_HOME: home,
    TANDEM_FAKE_KEYCHAIN: "1",
    TANDEM_E2E: "1",
    // TANDEM_PERF_BUILD=debug runs a suite on the debug build (quick
    // iterations; results record the build).
    TANDEM_APP_BINARY: join(
      root,
      (suite && process.env.TANDEM_PERF_BUILD !== "debug" ? "target/perf/tandem" : "target/debug/tandem") + EXE,
    ),
    TANDEM_API_URL: API,
    TANDEM_WEB_URL: WEB,
    TANDEM_OPEN_URL_CMD: opener,
    TANDEM_NET_LOG: join(home, "net.log"),
    TANDEM_FAKE_PICK_FOLDER: join(home, "pick-folder.txt"),
    // ssh reads this config instead of the user's own (debug builds); E2E-M14-9
    // writes its jump hosts there.
    TANDEM_SSH_CONFIG: join(home, "ssh_config"),
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
    TANDEM_HOOK_BIN: join(root, `target/debug/tandem-hook${EXE}`),
    TANDEM_TASKS_BIN: join(root, `target/debug/tandem-tasks${EXE}`),
    TANDEM_LAUNCHER_ROOT: join(home, "launcher"),
    TANDEM_SPAWN_LOG: join(home, "spawn.log"),
    // "Create with AI" answers from this file (debug builds; E2E-M10-7, D-8).
    TANDEM_FIXTURE_TEAMMATE_DRAFT: join(home, "teammate-draft.json"),
    // The local update feed (scripts/mock-update-feed.ts, E2E-M5-7); it
    // answers "nothing new" unless a spec picks a scenario.
    TANDEM_UPDATE_URL: `${FEED}/v1/updates`,
    // Every plugin's upstream and sign-in host is this one mock (debug E2E
    // builds; catalog::rebase).
    TANDEM_E2E_PLUGIN_BASE: PLUGINS,
    // Scrollback snapshots often enough that a kill -9 leaves some (E2E-M5-3).
    TANDEM_SNAPSHOT_EVERY_MS: "1500",
    // Dictate downloads come from the local model server (debug builds).
    TANDEM_MODELS_BASE: MODELS,
    TANDEM_MODELS_CONTROL: MODELS_CONTROL,
    TANDEM_APP_PATH: appPath,
    TANDEM_FAKE_ENGINE_BIN: fakeEngine,
    // Detection looks only at this run's folders (no npm, no /usr/local/bin).
    TANDEM_DETECT_SYSTEM_DIRS: "",
    TANDEM_DETECT_NO_NPM: "1",
    ...(suite ? { TANDEM_MOCHA_TIMEOUT: "900000" } : {}),
    PLAYWRIGHT_CHROMIUM:
      process.env.PLAYWRIGHT_CHROMIUM ??
      (existsSync("/opt/pw-browsers/chromium") ? "/opt/pw-browsers/chromium" : ""),
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
  // The API's log is kept for the relay check (E2E_RELAY_LOG): the phone's
  // frames cross the relay in it, and their plaintext must never appear.
  const relayLog = join(home, "api.log");
  if (!OFFLINE && !(await up(`${API}/v1/health`))) {
    api = startDetached(
      "pnpm",
      ["--filter", "api", "start"],
      {
        TANDEM_API_TEST_ROUTES: "1",
        BILLING_PROVIDER: "mock",
        WEB_PUBLIC_URL: WEB,
        API_PUBLIC_URL: API,
        LOG_LEVEL: "info",
        REALTIME_MOCK_URL: "ws://127.0.0.1:8794/realtime",
      },
      relayLog,
    );
    await waitUp(`${API}/v1/health`, 60);
    process.env.E2E_RELAY_LOG = relayLog;
  }
  if (!OFFLINE && !(await up(`${WEB}/api/health`))) {
    web = startDetached("pnpm", ["--filter", "web", "dev"], { API_INTERNAL_URL: API });
    await waitUp(`${WEB}/api/health`, 120);
  }
  let feed: ChildProcess | undefined;
  if (OFFLINE) {
    // No update feed: the app reports it can't check, which no offline spec asks about.
  } else if (!(await up(`${FEED}/health`))) {
    feed = startDetached(
      join(root, "node_modules/.bin/tsx"),
      ["scripts/mock-update-feed.ts", "--port", "8790", "--scenario", "none"],
      {},
    );
    await waitUp(`${FEED}/health`, 30);
  } else {
    await fetch(`${FEED}/__scenario`, { method: "POST", body: JSON.stringify({ scenario: "none" }) });
  }

  let realtime: ChildProcess | undefined;
  if (!(await up(`${REALTIME}/health`))) {
    realtime = startDetached(
      join(root, "node_modules/.bin/tsx"),
      ["scripts/mock-realtime.ts", "--port", "8794"],
      {},
    );
    await waitUp(`${REALTIME}/health`, 30);
  } else {
    await fetch(`${REALTIME}/__reset`, { method: "POST" });
  }

  let plugins: ChildProcess | undefined;
  if (OFFLINE) {
    // No plugin upstreams.
  } else if (!(await up(`${PLUGINS}/__state`))) {
    plugins = startDetached(join(root, `target/debug/mock-plugins${EXE}`), ["8791"], {});
    await waitUp(`${PLUGINS}/__state`, 30);
  } else {
    await fetch(`${PLUGINS}/__reset`);
  }

  let fixtures: ChildProcess | undefined;
  const fixturesUp = async () =>
    (await fetch(`${FIXTURES}/health`)
      .then((r) => r.text())
      .catch(() => "")) === "ok";
  if (!OFFLINE && !(await fixturesUp())) {
    fixtures = startDetached(
      join(root, "node_modules/.bin/tsx"),
      ["scripts/serve-fixtures.ts", "--port", "5173"],
      {},
    );
    for (let i = 0; i < 60 && !(await fixturesUp()); i++) await new Promise((r) => setTimeout(r, 500));
    if (!(await fixturesUp())) throw new Error(`${FIXTURES} didn't come up (is something else on :5173?)`);
  }

  // Specs steer it through models-control.txt (normal, slow or a byte to corrupt).
  let models: ChildProcess | undefined;
  const modelsUp = async () =>
    (await fetch(`${MODELS}/health`)
      .then((r) => r.text())
      .catch(() => "")) === "ok";
  if (!OFFLINE && !(await modelsUp())) {
    models = startDetached(
      join(root, "node_modules/.bin/tsx"),
      ["scripts/serve-models.ts", "--port", "8792", "--control", MODELS_CONTROL],
      { HOME: REAL_HOME, TANDEM_MODEL_CACHE: MODEL_CACHE },
    );
    for (let i = 0; i < 60 && !(await modelsUp()); i++) await new Promise((r) => setTimeout(r, 500));
    if (!(await modelsUp())) throw new Error(`${MODELS} didn't come up (is something else on :8792?)`);
  }

  let code = 1;
  try {
    const launcher = new Launcher(
      join(desktop, "e2e/wdio.conf.ts"),
      specList ? { spec: specList.map((p) => join(desktop, p)) } : spec ? { spec: [join(desktop, spec)] } : {},
    );
    code = (await launcher.run()) ?? 1;
  } finally {
    stop(api);
    stop(web);
    stop(feed);
    stop(plugins);
    stop(realtime);
    stop(fixtures);
    stop(models);
    stop(wm);
  }
  const problems = checkArtifacts(home);
  for (const p of problems) console.error(`E2E check failed: ${p}`);
  process.exit(problems.length ? 1 : code);
}

void main();
