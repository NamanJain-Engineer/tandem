// `pnpm --filter desktop perf -- --suite memory` (MILESTONES M15 task 7,
// PRODUCT_SPEC §5.1 memory budgets). Measures the base app with no panes and
// what each background terminal pane adds once its scrollback is full, and
// writes perf/results/m15.json (`memory`). The gateway runs inside the app
// process (D-4), so its budget is part of the base figure. A miss only a
// reference machine can settle is recorded as [manual] in BUILD_LOG, never
// loosened here.
import { browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { cpus, release, totalmem } from "node:os";
import { join } from "node:path";
import {
  addFolder,
  freshOffline,
  invoke,
  mod,
  paneIds,
  sendToPane,
  waitForApp,
  waitForPaneText,
} from "../support/app";

const ROOT = new URL("../../../..", import.meta.url).pathname;
const OUT = join(ROOT, "perf/results/m15.json");
const SCROLLBACK_LINES = 10_000; // the term_scrollback default
const PANES = 4; // the Free plan's limit

type Metric = { value: number; target: number; op: "<=" | ">="; unit: string; pass: boolean };
const metrics: Record<string, Metric> = {};

function record(name: string, value: number, op: "<=" | ">=", target: number, unit: string): void {
  const v = Math.round(value * 10) / 10;
  metrics[name] = { value: v, target, op, unit, pass: op === "<=" ? v <= target : v >= target };
  console.log(`perf ${name}: ${v} ${unit} (target ${op} ${target})`);
}

/** The app's process and everything under it except the shells it runs:
 * the WebKit web and network processes are its children. */
function appTree(): number[] {
  const app = (() => {
    try {
      return Number(
        execFileSync("pgrep", ["-nf", "target/(debug|perf)/tandem$"], { encoding: "utf8" }).trim(),
      );
    } catch {
      return 0;
    }
  })();
  if (!app) return [];
  const out = [app];
  for (let i = 0; i < out.length; i++) {
    let kids: number[] = [];
    try {
      kids = execFileSync("pgrep", ["-P", String(out[i])], { encoding: "utf8" })
        .trim()
        .split("\n")
        .filter(Boolean)
        .map(Number);
    } catch {
      // none
    }
    for (const k of kids) {
      const comm = (() => {
        try {
          return readFileSync(`/proc/${k}/comm`, "utf8").trim();
        } catch {
          return "";
        }
      })();
      if (comm.startsWith("WebKit")) out.push(k);
    }
  }
  return out;
}

/** Proportional set size (MB): shared pages split between the processes
 * that map them, so libraries aren't counted once per process. */
function pssMb(pids: number[]): number {
  let kb = 0;
  for (const pid of pids) {
    try {
      kb += Number(/^Pss:\s+(\d+)/m.exec(readFileSync(`/proc/${pid}/smaps_rollup`, "utf8"))?.[1] ?? 0);
    } catch {
      // exited
    }
  }
  return kb / 1024;
}

function rssMb(pids: number[]): number {
  let kb = 0;
  for (const pid of pids) {
    try {
      kb += Number(/VmRSS:\s+(\d+)/.exec(readFileSync(`/proc/${pid}/status`, "utf8"))?.[1] ?? 0);
    } catch {
      // exited
    }
  }
  return kb / 1024;
}

/** Memory once it stops moving (the webview settles after a change). */
async function settledMb(): Promise<number> {
  let last = pssMb(appTree());
  for (let i = 0; i < 20; i++) {
    await browser.pause(1000);
    const now = pssMb(appTree());
    if (Math.abs(now - last) < 2) {
      const each = appTree().map(
        (pid) => `${readFileSync(`/proc/${pid}/comm`, "utf8").trim()} ${pssMb([pid]).toFixed(0)}`,
      );
      console.log(
        `perf memory: PSS ${now.toFixed(0)} MB (${each.join(", ")}), RSS sum ${rssMb(appTree()).toFixed(0)} MB`,
      );
      return now;
    }
    last = now;
  }
  return last;
}

/** Opens PANES shells, fills each past its scrollback, moves them to a
 * background tab and returns what each added pane costs (MB). */
async function perBackgroundPane(): Promise<number> {
  await addFolder(`/tmp/tandem-perf-memory-${Date.now()}`);
  await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
  const [ws] = await invoke<Array<{ id: string; tabs: Array<{ id: string }> }>>("workspace_list");
  const tabId = ws?.tabs[0]?.id ?? "";
  const ids = [(await paneIds())[0] ?? ""];
  for (const id of ids) await waitForPaneText(id, "#", 30_000);
  // One shell is open; the rest are measured against it.
  const before = await settledMb();
  for (let i = 1; i < PANES; i++)
    ids.push(
      (await invoke<{ id: string }>("pane_create", { tabId, kind: "terminal", cwd: null, engineId: null }))
        .id,
    );
  const leaf = (paneId: string) => ({ type: "leaf", paneId });
  const tree = {
    type: "split",
    dir: "col",
    ratio: 0.5,
    a: { type: "split", dir: "row", ratio: 0.5, a: leaf(ids[0] ?? ""), b: leaf(ids[1] ?? "") },
    b: { type: "split", dir: "row", ratio: 0.5, a: leaf(ids[2] ?? ""), b: leaf(ids[3] ?? "") },
  };
  await invoke("layout_save", { tabId, tree, activePaneId: ids[0], maximizedPaneId: null });
  await browser.refresh();
  await waitForApp();
  await browser.waitUntil(async () => (await paneIds()).length === PANES, { timeout: 30_000 });
  for (const id of ids) await waitForPaneText(id, "#", 30_000);
  for (const id of ids) {
    await sendToPane(id, `seq 1 ${SCROLLBACK_LINES * 2}; echo full-$((6*7))\r`);
  }
  for (const id of ids) await waitForPaneText(id, "full-42", 120_000);
  // A new tab puts the four panes in the background.
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), "t"]);
  await browser.waitUntil(async () => (await paneIds()).every((p) => !ids.includes(p)), { timeout: 15_000 });
  // Hidden panes give their WebGL context back after 8 s (F-CD-04 AC3).
  await browser.waitUntil(
    async () =>
      (await browser.execute(() =>
        (window as unknown as { __tandemTest: { webglContexts: () => number } }).__tandemTest.webglContexts(),
      )) === 0,
    { timeout: 30_000, timeoutMsg: "background panes kept their WebGL contexts" },
  );
  const after = await settledMb();
  // Conservative: the growth covers three new panes and four full
  // scrollbacks (the first pane's too), divided by the three added.
  console.log(`perf memory: ${before.toFixed(0)} MB before, ${after.toFixed(0)} MB with ${PANES} full panes`);
  return (after - before) / (PANES - 1);
}

const BUDGET_MB = Math.round(((SCROLLBACK_LINES * 200) / 2 ** 20 + 2) * 10) / 10;

describe("perf: memory", () => {
  after(() => {
    mkdirSync(join(ROOT, "perf/results"), { recursive: true });
    const prior = existsSync(OUT) ? (JSON.parse(readFileSync(OUT, "utf8")) as Record<string, unknown>) : {};
    const body = {
      ...prior,
      memory: {
        date: new Date().toISOString(),
        machine: {
          os: `linux ${release()} (xvfb)`,
          cpus: cpus().length,
          cpuModel: cpus()[0]?.model,
          memGb: Math.round(totalmem() / 2 ** 30),
          build: process.env.TANDEM_APP_BINARY?.includes("/perf/") ? "perf" : "debug",
        },
        metrics,
      },
    };
    writeFileSync(OUT, `${JSON.stringify(body, null, 2)}\n`);
  });

  it("§5.1 base app with no panes ≤ 350 MB (app and webview, the gateway included)", async function () {
    this.timeout(120_000);
    await freshOffline();
    await browser.pause(3000);
    record("baseAppMb", await settledMb(), "<=", 350, "MB");
  });

  it("§5.1 each background pane with full scrollback ≤ (lines × 200 B) + 2 MB", async function () {
    this.timeout(600_000);
    record("perBackgroundPaneMb", await perBackgroundPane(), "<=", BUDGET_MB, "MB");
  });

  it("§5.1 the same with GPU rendering off (xterm's own memory, no WebGL)", async function () {
    this.timeout(600_000);
    await freshOffline();
    await invoke("settings_set", { key: "term_gpu_rendering", value: "off" });
    record("perBackgroundPaneNoGpuMb", await perBackgroundPane(), "<=", BUDGET_MB, "MB");
    await invoke("settings_set", { key: "term_gpu_rendering", value: "auto" });
  });

  it("§5.1 every memory budget measured here is met", () => {
    const missed = Object.entries(metrics).filter(([, m]) => !m.pass);
    if (missed.length)
      throw new Error(
        `missed: ${missed.map(([k, m]) => `${k} ${m.value} ${m.unit} (target ${m.op} ${m.target})`).join("; ")}`,
      );
  });
});
