#!/usr/bin/env tsx
/**
 * Kill-fuzz for crash-safe restore (F-SH-07, MILESTONES M5 exit checklist).
 *
 *   pnpm tsx scripts/killfuzz.ts --iterations 20 [--seed 7] [--no-build] [--keep]
 *
 * One seeded profile (a workspace with 4 shells and 2 agents) is launched in
 * the real app and kill -9'd at random points: during startup, or once its
 * shells run. Every few rounds the store's own writer is killed mid-write
 * too. After each kill:
 * - the database passes `integrity_check` and `foreign_key_check`;
 * - every layout still matches its tab's open panes;
 * - agents came back paused, shells untouched (nothing auto-ran);
 * - no process the app started outlives it (orphans);
 * - the app wrote no crash report (no panic on restore).
 *
 * Needs a display (DISPLAY, or `xvfb-run` on PATH: it re-runs itself under
 * it). Never touches the network: the app's API points at a closed port.
 */
import { execFileSync, spawn, spawnSync, type ChildProcess } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const args = process.argv.slice(2);
const flag = (name: string) => args.includes(`--${name}`);
const option = (name: string, fallback: string) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? (args[i + 1] as string) : fallback;
};
const iterations = Number(option("iterations", "20"));
const seed = Number(option("seed", String(Date.now() % 100_000)));
const appBin = resolve(option("app", join(root, "target/debug/tandem")));
const fuzzBin = join(root, "target/debug/examples/killfuzz");

function haveXvfb(): boolean {
  return spawnSync("sh", ["-c", "command -v xvfb-run"]).status === 0;
}

if (!process.env.DISPLAY && !process.env.KILLFUZZ_UNDER_XVFB) {
  if (!haveXvfb()) {
    console.error("killfuzz: needs a display (set DISPLAY or install xvfb-run)");
    process.exit(2);
  }
  const r = spawnSync("xvfb-run", ["-a", process.execPath, ...process.execArgv, ...process.argv.slice(1)], {
    stdio: "inherit",
    env: { ...process.env, KILLFUZZ_UNDER_XVFB: "1" },
  });
  process.exit(r.status ?? 1);
}

/** mulberry32: a reproducible run for a given --seed. */
function rng(s: number): () => number {
  let a = s >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = rng(seed);
const between = (lo: number, hi: number) => Math.round(lo + rand() * (hi - lo));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

if (!flag("no-build")) {
  execFileSync("cargo", ["build", "-q", "-p", "tandem-store", "--example", "killfuzz"], {
    cwd: root,
    stdio: "inherit",
  });
  if (!existsSync(appBin)) {
    execFileSync("pnpm", ["--filter", "desktop", "build"], { cwd: root, stdio: "inherit" });
    execFileSync("cargo", ["build", "-q", "-p", "tandem-desktop", "--features", "custom-protocol"], {
      cwd: root,
      stdio: "inherit",
    });
  }
}
for (const bin of [appBin, fuzzBin]) {
  if (!existsSync(bin)) {
    console.error(`killfuzz: ${bin} is missing (build it, or drop --no-build)`);
    process.exit(2);
  }
}

const work = mkdtempSync(join(tmpdir(), "tandem-killfuzz-"));
const data = join(work, "data");
const userHome = join(work, "home");
const wsRoot = join(work, "ws");
mkdirSync(userHome, { recursive: true });
writeFileSync(join(userHome, ".profile"), "");

function store(...cmd: string[]): string {
  return execFileSync(fuzzBin, cmd, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}

const seeded = JSON.parse(store("seed", data, wsRoot).trim()) as { panes: string[] };
const paneIds = new Set(seeded.panes);

const appEnv: NodeJS.ProcessEnv = {
  HOME: userHome,
  USER: process.env.USER ?? "fuzz",
  LANG: "C.UTF-8",
  DISPLAY: process.env.DISPLAY,
  XAUTHORITY: process.env.XAUTHORITY,
  PATH: "/usr/local/bin:/usr/bin:/bin",
  SHELL: "/bin/sh",
  TANDEM_HOME: data,
  TANDEM_FAKE_KEYCHAIN: "1",
  TANDEM_SNAPSHOT_EVERY_MS: "300",
  // A closed port: nothing leaves the machine.
  TANDEM_API_URL: "http://127.0.0.1:9",
  TANDEM_WEB_URL: "http://127.0.0.1:9",
  NO_AT_BRIDGE: "1",
};

/** Children the app recorded in its registry (`$DATA/run/children-<pid>.json`). */
function registered(pid: number): number[] {
  const file = join(data, "run", `children-${pid}.json`);
  if (!existsSync(file)) return [];
  try {
    const doc = JSON.parse(readFileSync(file, "utf8")) as
      { children?: Array<{ pid: number }> } | Array<{ pid: number }>;
    const list = Array.isArray(doc) ? doc : (doc.children ?? []);
    return list.map((c) => c.pid);
  } catch {
    return [];
  }
}

function alive(pid: number): boolean {
  try {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    return !/\)\s+Z/.test(stat);
  } catch {
    return false;
  }
}

/** Live processes carrying one of the seeded panes' ids (started by the app). */
function strays(): number[] {
  const out: number[] = [];
  for (const name of readdirSync("/proc")) {
    if (!/^\d+$/.test(name)) continue;
    let env = "";
    try {
      env = readFileSync(`/proc/${name}/environ`, "latin1");
    } catch {
      continue;
    }
    const m = /(?:^|\0)TANDEM_PANE_ID=([^\0]+)/.exec(env);
    if (m?.[1] && paneIds.has(m[1]) && alive(Number(name))) out.push(Number(name));
  }
  return out;
}

async function waitExit(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return;
  await new Promise<void>((r) => child.once("exit", () => r()));
}

function crashReports(): string[] {
  const dir = join(data, "crashes");
  return existsSync(dir) ? readdirSync(dir) : [];
}

const problems: string[] = [];
const note = (i: number, what: string) => {
  problems.push(`#${i}: ${what}`);
  console.log(`  ✗ ${what}`);
};

function check(i: number, launched: boolean): void {
  try {
    const r = JSON.parse(store("check", data, ...(launched ? ["--launched"] : [])).trim()) as {
      ok: boolean;
      problems: string[];
    };
    for (const p of r.problems) note(i, p);
  } catch (e) {
    note(i, `the store didn't open: ${(e as Error).message.split("\n")[0]}`);
  }
}

console.log(`killfuzz: ${iterations} iterations, seed ${seed}, profile ${work}`);
let launchedOnce = false;
for (let i = 1; i <= iterations; i++) {
  // The first round always gets as far as live shells.
  const late = i === 1 || rand() < 0.6;
  const app = spawn(appBin, [], { env: appEnv, stdio: "ignore" });
  const pid = app.pid ?? 0;
  let detail = "";
  if (late) {
    const until = Date.now() + 40_000;
    while (registered(pid).length < 4 && Date.now() < until && app.exitCode === null) await sleep(100);
    const n = registered(pid).length;
    if (n < 4)
      note(
        i,
        `the app never started its 4 shells (${n}${app.exitCode !== null ? `, exited ${app.exitCode}` : ""})`,
      );
    const after = between(0, 1_000);
    await sleep(after);
    detail = `kill -9 ${after} ms after ${n} shells`;
  } else {
    const at = between(50, 1_800);
    await sleep(at);
    detail = `kill -9 ${at} ms into startup`;
  }
  const children = registered(pid);
  app.kill("SIGKILL");
  await waitExit(app);
  launchedOnce ||= late;
  console.log(`#${i} ${detail}`);
  // Shells get SIGHUP when the PTYs close; give them a moment.
  const until = Date.now() + 3_000;
  while ((children.some(alive) || strays().length > 0) && Date.now() < until) await sleep(100);
  const left = [...new Set([...children.filter(alive), ...strays()])];
  if (left.length > 0) {
    note(i, `${left.length} process(es) outlived the app: ${left.join(", ")}`);
    for (const p of left) {
      try {
        process.kill(p, "SIGKILL");
      } catch {
        // gone
      }
    }
  }
  check(i, launchedOnce);

  // Every third round: kill the store mid-write as well.
  if (i % 3 === 0) {
    const writer = spawn(fuzzBin, ["write", data], { stdio: "ignore" });
    const at = between(30, 600);
    await sleep(at);
    writer.kill("SIGKILL");
    await waitExit(writer);
    console.log(`#${i} store writer killed after ${at} ms`);
    check(i, launchedOnce);
  }
  const reports = crashReports();
  if (reports.length > 0) {
    note(i, `crash report(s): ${reports.join(", ")}`);
    for (const r of reports) rmSync(join(data, "crashes", r), { force: true });
  }
}

if (problems.length === 0) {
  console.log(`killfuzz: clean (${iterations} iterations, seed ${seed})`);
  if (!flag("keep")) rmSync(work, { recursive: true, force: true });
  process.exit(0);
}
console.log(`killfuzz: ${problems.length} problem(s) (seed ${seed}, profile kept at ${work}):`);
for (const p of problems) console.log(`  ${p}`);
process.exit(1);
