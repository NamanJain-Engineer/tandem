// `pnpm --filter desktop perf -- --suite startup` (MILESTONES M5, F-SH-07
// AC7, PRODUCT_SPEC §5.1). Cold start to an interactive window with a
// 16-shell layout, P50 of five launches, and the Dashboard opening with six
// agents. Writes perf/results/m5.json. The run uses the real Secret Service
// keychain (e2e/run.ts starts gnome-keyring) so the session survives each
// relaunch. A miss only a reference machine can settle is recorded as
// [manual] in BUILD_LOG, never loosened here.
import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { cpus, release, totalmem } from "node:os";
import { join } from "node:path";
import { addFolder, freshUser, invoke, paneIds, paneText, planFooter, relaunch, setPlan } from "../support/app";

const ROOT = new URL("../../../..", import.meta.url).pathname;
const OUT = join(ROOT, "perf/results/m5.json");
const LAUNCHES = 5;

type Metric = { value: number; target: number; op: "<=" | ">="; unit: string; pass: boolean };
const metrics: Record<string, Metric> = {};
function record(name: string, value: number, op: "<=" | ">=", target: number, unit: string): void {
  const v = Math.round(value * 10) / 10;
  metrics[name] = { value: v, target, op, unit, pass: op === "<=" ? v <= target : v >= target };
  console.log(`perf ${name}: ${v} ${unit} (target ${op} ${target})`);
}
const median = (xs: number[]) => {
  const s = [...xs].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? (s[m] as number) : ((s[m - 1] as number) + (s[m] as number)) / 2;
};

type Node = { type: "leaf"; paneId: string } | { type: "split"; dir: "row" | "col"; ratio: number; a: Node; b: Node };
function balanced(nodes: Node[], dir: "row" | "col"): Node {
  if (nodes.length === 1) return nodes[0] as Node;
  const half = Math.ceil(nodes.length / 2);
  return { type: "split", dir, ratio: half / nodes.length, a: balanced(nodes.slice(0, half), dir), b: balanced(nodes.slice(half), dir) };
}

/** When the app process started (epoch ms), from /proc. */
function appStartedAt(): number {
  const pid = execFileSync("pgrep", ["-n", "-f", "target/(debug|perf)/tandem$"], { encoding: "utf8" }).trim();
  const ticks = Number(readFileSync(`/proc/${pid}/stat`, "utf8").split(") ")[1]?.split(" ")[19]);
  const hz = Number(execFileSync("getconf", ["CLK_TCK"], { encoding: "utf8" }).trim());
  const bootS = Number(/^btime (\d+)$/m.exec(readFileSync("/proc/stat", "utf8"))?.[1]);
  return (bootS + ticks / hz) * 1000;
}

describe("perf: startup (M5)", () => {
  let ids: string[] = [];

  after(() => {
    mkdirSync(join(ROOT, "perf/results"), { recursive: true });
    const prior = existsSync(OUT) ? (JSON.parse(readFileSync(OUT, "utf8")) as Record<string, unknown>) : {};
    const body = {
      ...prior,
      startup: {
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

  before(async function () {
    this.timeout(300_000);
    const { email } = await freshUser("perf-startup");
    await setPlan(email, "pro");
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), { timeout: 20_000 });
    await addFolder(`/tmp/tandem-perf-startup-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [ws] = await invoke<Array<{ tabs: Array<{ id: string }> }>>("workspace_list");
    const tabId = ws?.tabs[0]?.id ?? "";
    ids = [(await paneIds())[0] ?? ""];
    for (let i = 1; i < 16; i++) ids.push((await invoke<{ id: string }>("pane_create", { tabId, kind: "terminal", cwd: null, engineId: null })).id);
    const rows = [0, 4, 8, 12].map((at) => balanced(ids.slice(at, at + 4).map((paneId) => ({ type: "leaf", paneId })), "row"));
    await invoke("layout_save", { tabId, tree: balanced(rows, "col"), activePaneId: ids[0], maximizedPaneId: null });
  });

  it("F-SH-07.7 cold start to an interactive window with 16 shells: P50 of five launches", async function () {
    this.timeout(600_000);
    const interactive: number[] = [];
    const allShells: number[] = [];
    for (let i = 0; i < LAUNCHES; i++) {
      await relaunch();
      const started = appStartedAt();
      // Interactive: the layout is up and the focused shell shows its prompt.
      await browser.waitUntil(
        async () => (await paneIds()).length === 16 && (await paneText(ids[0] ?? "")).trim().length > 0,
        { timeout: 60_000, interval: 25, timeoutMsg: "the 16-pane layout never became interactive" },
      );
      interactive.push(Date.now() - started);
      await browser.waitUntil(
        async () => {
          for (const id of ids) if ((await paneText(id)).trim().length === 0) return false;
          return true;
        },
        { timeout: 60_000, interval: 50, timeoutMsg: "not every shell started" },
      );
      allShells.push(Date.now() - started);
      console.log(`launch ${i + 1}: interactive ${interactive.at(-1)} ms, all 16 shells ${allShells.at(-1)} ms`);
    }
    record("coldStartInteractiveP50Ms", median(interactive), "<=", 2000, "ms");
    record("coldStartAllShellsP50Ms", median(allShells), "<=", 4000, "ms");
  });

  it("F-DB-01 the Dashboard opens with six agents within 500 ms", async function () {
    this.timeout(120_000);
    for (const [i, status] of ["working", "working", "working", "waiting", "idle", "idle"].entries())
      await invoke("test_fixture_agent", {
        agent: { id: `perf${i}`, title: `perf agent ${i}`, status, engineId: "claude", workspaceId: null, preview: null, approval: null },
      });
    // The Dashboard is the sidebar's tab; each sample opens the sidebar on it.
    if (!(await $('[data-testid="sidebar"]').isDisplayed().catch(() => false))) await $('[data-testid="sidebar-toggle"]').click();
    await $('[data-testid="sidebar-pill-dashboard"]').click();
    const samples: number[] = [];
    for (let i = 0; i < 5; i++) {
      if (await $('[data-testid="sidebar"]').isDisplayed().catch(() => false)) await $('[data-testid="sidebar-toggle"]').click();
      await $('[data-testid="sidebar"]').waitForDisplayed({ reverse: true, timeout: 5_000 });
      // Measured in the page: from the click until six cards are on screen.
      const ms = await browser.executeAsync((done: (ms: number) => void) => {
        const visible = () =>
          Array.from(document.querySelectorAll<HTMLElement>('[data-testid="agent-card"]')).filter((el) => el.offsetParent !== null).length;
        const t0 = performance.now();
        document.querySelector<HTMLElement>('[data-testid="sidebar-toggle"]')?.click();
        const tick = () => {
          if (visible() >= 6) requestAnimationFrame(() => done(performance.now() - t0));
          else if (performance.now() - t0 > 5_000) done(5_000);
          else requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
      });
      samples.push(ms);
    }
    record("dashboardOpenP50Ms", median(samples), "<=", 500, "ms");
  });
});
