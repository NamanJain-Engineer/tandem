// `pnpm --filter desktop perf -- --suite terminals` (MILESTONES M3,
// PRODUCT_SPEC §5.1). Measures the terminal targets on this machine and
// writes perf/results/m3.json; every target is asserted. Debug build under
// Xvfb: a miss that only a reference machine can settle is recorded as
// [manual] in BUILD_LOG, never loosened here.
import { $, $$, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { cpus, totalmem, release } from "node:os";
import { join } from "node:path";
import { inflateSync } from "node:zlib";
import { addFolder, freshUser, invoke, mod, paneIds, planFooter, sendToPane, setPlan, waitForApp, waitForPaneText } from "../support/app";

const ROOT = new URL("../../../..", import.meta.url).pathname;
const OUT = join(ROOT, "perf/results/m3.json");

type Metric = { value: number; target: number; op: "<=" | ">="; unit: string; pass: boolean };
const metrics: Record<string, Metric> = {};

function record(name: string, value: number, op: "<=" | ">=", target: number, unit: string): void {
  const v = Math.round(value * 10) / 10;
  metrics[name] = { value: v, target, op, unit, pass: op === "<=" ? v <= target : v >= target };
  console.log(`perf ${name}: ${v} ${unit} (target ${op} ${target})`);
}

function p95(xs: number[]): number {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.max(0, Math.ceil(0.95 * s.length) - 1)] ?? NaN;
}

type Node = { type: "leaf"; paneId: string } | { type: "split"; dir: "row" | "col"; ratio: number; a: Node; b: Node };
function balanced(nodes: Node[], dir: "row" | "col"): Node {
  if (nodes.length === 1) return nodes[0] as Node;
  const half = Math.ceil(nodes.length / 2);
  return { type: "split", dir, ratio: half / nodes.length, a: balanced(nodes.slice(0, half), dir), b: balanced(nodes.slice(half), dir) };
}
const grid = (ids: string[], cols: number): Node => {
  const rows: Node[] = [];
  for (let i = 0; i < ids.length; i += cols) rows.push(balanced(ids.slice(i, i + cols).map((paneId) => ({ type: "leaf", paneId })), "row"));
  return balanced(rows, "col");
};

/** The app and its webview processes (not the shells). */
function appPids(): number[] {
  const out = (cmd: string) => {
    try {
      return execFileSync("pgrep", ["-f", cmd], { encoding: "utf8" }).trim().split("\n").filter(Boolean).map(Number);
    } catch {
      return [];
    }
  };
  return [...out("target/(debug|perf)/tandem$"), ...out("WebKitWebProcess"), ...out("WebKitNetworkProcess")];
}

function cpuTicks(pids: number[]): number {
  let total = 0;
  for (const pid of pids) {
    try {
      const fields = readFileSync(`/proc/${pid}/stat`, "utf8").split(") ")[1]?.split(" ") ?? [];
      total += Number(fields[11]) + Number(fields[12]); // utime + stime
    } catch {
      // exited
    }
  }
  return total;
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

/** Decodes an 8-bit RGB(A) PNG (what WebDriver screenshots are). */
function decodePng(buf: Buffer): { w: number; h: number; px: Uint8Array } {
  const w = buf.readUInt32BE(16);
  const h = buf.readUInt32BE(20);
  const channels = buf[25] === 6 ? 4 : 3;
  const idat: Buffer[] = [];
  for (let at = 8; at < buf.length; ) {
    const len = buf.readUInt32BE(at);
    if (buf.toString("ascii", at + 4, at + 8) === "IDAT") idat.push(buf.subarray(at + 8, at + 8 + len));
    at += 12 + len;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const stride = w * channels;
  const px = new Uint8Array(w * h * 4);
  const prev = new Uint8Array(stride);
  const cur = new Uint8Array(stride);
  for (let y = 0; y < h; y++) {
    const filter = raw[y * (stride + 1)] ?? 0;
    for (let x = 0; x < stride; x++) {
      const v = raw[y * (stride + 1) + 1 + x] ?? 0;
      const a = x >= channels ? (cur[x - channels] ?? 0) : 0;
      const b = prev[x] ?? 0;
      const c = x >= channels ? (prev[x - channels] ?? 0) : 0;
      const paeth = () => {
        const p = a + b - c;
        const [pa, pb, pc] = [Math.abs(p - a), Math.abs(p - b), Math.abs(p - c)];
        return pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      };
      cur[x] = (v + [0, a, b, (a + b) >> 1, paeth()][filter]!) & 0xff;
    }
    for (let x = 0; x < w; x++) {
      for (let k = 0; k < 3; k++) px[(y * w + x) * 4 + k] = cur[x * channels + k] ?? 0;
      px[(y * w + x) * 4 + 3] = 255;
    }
    prev.set(cur);
  }
  return { w, h, px };
}

type Hook = {
  paneCursor: (id: string) => { x: number; y: number } | null;
  webglHolds: (id: string) => boolean;
  perf: { enable: () => void; times: () => Record<string, { mounted: number; firstOutput?: number }>; writes: () => Record<string, number> };
};

/** The streaming load (MILESTONES M3 mocks): 50 MB of `yes` per pane. */
const STREAM = "yes 'stream 0123456789 abcdefghijklmnopqrstuvwxyz' | head -c 50M\n";

/** Waits until no pane has written for `quietMs` (streams done or stopped). */
async function drained(quietMs = 3_000, timeout = 300_000): Promise<void> {
  const total = async () =>
    Object.values((await browser.execute(() => (window as unknown as { __tandemTest: Hook }).__tandemTest.perf.writes())) as Record<string, number>).reduce(
      (a, b) => a + b,
      0,
    );
  let last = await total();
  let since = Date.now();
  await browser.waitUntil(
    async () => {
      const now = await total();
      if (now !== last) {
        last = now;
        since = Date.now();
      }
      return Date.now() - since >= quietMs;
    },
    { timeout, interval: 500, timeoutMsg: "output never stopped" },
  );
}

describe("perf: terminals", () => {
  let ids: string[] = [];
  let workspaceId = "";

  after(() => {
    mkdirSync(join(ROOT, "perf/results"), { recursive: true });
    const prior = existsSync(OUT) ? (JSON.parse(readFileSync(OUT, "utf8")) as Record<string, unknown>) : {};
    const body = {
      ...prior,
      terminals: {
        date: new Date().toISOString(),
        machine: { os: `linux ${release()} (xvfb)`, cpus: cpus().length, cpuModel: cpus()[0]?.model, memGb: Math.round(totalmem() / 2 ** 30), build: process.env.TANDEM_APP_BINARY?.includes("/perf/") ? "perf" : "debug" },
        metrics,
      },
    };
    writeFileSync(OUT, `${JSON.stringify(body, null, 2)}\n`);
  });

  before(async function () {
    this.timeout(300_000);
    // The page is busy while 15 panes stream; give its scripts room.
    await browser.setTimeout({ script: 300_000 });
    const { email } = await freshUser("perf-terminals");
    await setPlan(email, "pro");
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), { timeout: 20_000 });
    await addFolder(`/tmp/tandem-perf-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [ws] = await invoke<Array<{ id: string; tabs: Array<{ id: string }> }>>("workspace_list");
    workspaceId = ws?.id ?? "";
    const tabId = ws?.tabs[0]?.id ?? "";
    ids = [(await paneIds())[0] ?? ""];
    for (let i = 1; i < 16; i++) ids.push((await invoke<{ id: string }>("pane_create", { tabId, kind: "terminal", cwd: null, engineId: null })).id);
    await invoke("layout_save", { tabId, tree: grid(ids, 4), activePaneId: ids[0], maximizedPaneId: null });
    await browser.refresh();
    await waitForApp();
    await browser.waitUntil(async () => (await paneIds()).length === 16, { timeout: 30_000 });
    for (const id of ids) await waitForPaneText(id, "#", 30_000);
    await browser.execute(() => (window as unknown as { __tandemTest: Hook }).__tandemTest.perf.enable());
  });

  beforeEach(function () {
    // Each measurement can wait out 15 streams of 50 MB.
    this.currentTest?.timeout(600_000);
  });

  it("mode switch ≤ 100 ms", async () => {
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    const samples: number[] = [];
    for (let i = 0; i < 10; i++) {
      for (const [key, label] of [
        ["3", "Thread"],
        ["2", "Code"],
      ] as const) {
        const ms = (await browser.executeAsync(
          (k: string, want: string, ctrl: boolean, done: (ms: number) => void) => {
            const t0 = performance.now();
            window.dispatchEvent(new KeyboardEvent("keydown", { key: k, code: `Digit${k}`, ctrlKey: ctrl, metaKey: !ctrl, bubbles: true, cancelable: true }));
            const check = () => {
              const on = document.querySelector<HTMLElement>('[role="radiogroup"][aria-label="Work mode"] [role="radio"][aria-checked="true"]')?.innerText.trim();
              if (on === want) requestAnimationFrame(() => done(performance.now() - t0));
              else if (performance.now() - t0 > 2_000) done(2_000);
              else requestAnimationFrame(check);
            };
            requestAnimationFrame(check);
          },
          key,
          label,
          mod() === "Control",
        )) as number;
        samples.push(ms);
      }
    }
    record("modeSwitchP95Ms", p95(samples), "<=", 100, "ms");
  });

  it("spawning 6 agent panes: frames visible ≤ 300 ms, no UI stall > 100 ms", async () => {
    const tab = await invoke<{ id: string }>("tab_create", { workspaceId, name: "Agents" });
    const agents: string[] = [];
    for (let i = 0; i < 6; i++) agents.push((await invoke<{ id: string }>("pane_create", { tabId: tab.id, kind: "agent", cwd: null, engineId: "claude" })).id);
    await invoke("layout_save", { tabId: tab.id, tree: grid(agents, 3), activePaneId: agents[0], maximizedPaneId: null });
    await browser.execute(() => (window as unknown as { __tandemTest: Hook }).__tandemTest.perf.enable());
    await browser.refresh();
    await waitForApp();
    await browser.waitUntil(async () => (await paneIds()).length === 16, { timeout: 30_000 });
    // Watch frame gaps while the tab opens and its six agents start.
    await browser.execute(() => {
      const w = window as unknown as { __maxGap: number; __gapRun: boolean };
      w.__maxGap = 0;
      w.__gapRun = true;
      let last = performance.now();
      const tick = () => {
        const now = performance.now();
        w.__maxGap = Math.max(w.__maxGap, now - last);
        last = now;
        if (w.__gapRun) requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    });
    // Each agent is framed from the click that shows its tab, so the time a
    // view waits to open counts too (D-141).
    const clickedAt = (await browser.execute((sel: string) => {
      const at = performance.now();
      document.querySelector<HTMLElement>(sel)?.click();
      return at;
    }, `[data-testid="tab"][data-tab-id="${tab.id}"] button`)) as number;
    for (const id of agents) await waitForPaneText(id, "Fake Claude Code", 15_000);
    const gap = (await browser.execute(() => {
      const w = window as unknown as { __maxGap: number; __gapRun: boolean };
      w.__gapRun = false;
      return w.__maxGap;
    })) as number;
    const times = (await browser.execute(() => (window as unknown as { __tandemTest: Hook }).__tandemTest.perf.times())) as Record<
      string,
      { mounted: number; firstOutput?: number }
    >;
    const framed = agents.map((id) => (times[id]?.firstOutput ?? Infinity) - clickedAt);
    record("sixAgentsFramedMs", Math.max(...framed), "<=", 300, "ms");
    record("sixAgentsMaxStallMs", gap, "<=", 100, "ms");
    // Back to the 16 shells for the rest.
    await $$('[data-testid="tab"]')[0]?.click();
    await browser.waitUntil(async () => (await paneIds()).length === 16, { timeout: 30_000 });
  });

  it("DOM and WebGL renderers differ by ≤ 1% of pixels", async () => {
    const holds = (id: string) => browser.execute((p: string) => (window as unknown as { __tandemTest: Hook }).__tandemTest.webglHolds(p), id);
    const gl: string[] = [];
    const dom: string[] = [];
    for (const id of ids) ((await holds(id)) ? gl : dom).push(id);
    expect(gl.length).toBeGreaterThan(0);
    expect(dom.length).toBeGreaterThan(0);
    const [a, b] = [gl[gl.length - 1] ?? "", dom[0] ?? ""];
    const art = "clear; for i in 1 2 3 4 5 6; do printf '\\033[31mred \\033[32mgreen \\033[1;34mbold blue\\033[0m plain 0123456789 ~!@#$%%^&*()\\n'; done\n";
    for (const id of [a, b]) await sendToPane(id, art);
    for (const id of [a, b]) await waitForPaneText(id, "plain 0123456789");
    // Neither holds focus (cursors match).
    await $(`[data-testid="terminal"][data-pane-id="${ids.find((id) => id !== a && id !== b)}"]`).click();
    await browser.pause(1_000);
    const shot = async (id: string) => {
      const el = await $(`[data-testid="terminal"][data-pane-id="${id}"]`);
      return decodePng(Buffer.from(await browser.takeElementScreenshot(await el.elementId), "base64"));
    };
    const [x, y] = [await shot(a), await shot(b)];
    const w = Math.min(x.w, y.w);
    const h = Math.min(x.h, y.h);
    let differ = 0;
    for (let row = 0; row < h; row++) {
      for (let col = 0; col < w; col++) {
        const i = (row * x.w + col) * 4;
        const j = (row * y.w + col) * 4;
        const d = Math.max(Math.abs((x.px[i] ?? 0) - (y.px[j] ?? 0)), Math.abs((x.px[i + 1] ?? 0) - (y.px[j + 1] ?? 0)), Math.abs((x.px[i + 2] ?? 0) - (y.px[j + 2] ?? 0)));
        if (d > 48) differ++;
      }
    }
    record("domVsWebglPixelDiffPercent", (differ / (w * h)) * 100, "<=", 1, "%");
  });

  it("keypress → echo in the focused terminal with 16 panes streaming: P95 ≤ 50 ms", async () => {
    for (const id of ids.slice(1)) await sendToPane(id, STREAM);
    await $(`[data-testid="terminal"][data-pane-id="${ids[0]}"]`).click();
    await browser.pause(2_000);
    const samples = (await browser.executeAsync(
      (pane: string, n: number, done: (xs: number[]) => void) => {
        const t = (window as unknown as { __tandemTest: Hook }).__tandemTest;
        const ipc = (window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
        const out: number[] = [];
        const one = (i: number) => {
          if (i >= n) return done(out);
          const before = t.paneCursor(pane);
          const t0 = performance.now();
          void ipc.invoke("pty_write", { paneId: pane, data: "x" });
          const check = () => {
            const c = t.paneCursor(pane);
            if (c && before && (c.x !== before.x || c.y !== before.y)) {
              out.push(performance.now() - t0);
              setTimeout(() => one(i + 1), 30);
            } else if (performance.now() - t0 > 2_000) {
              out.push(2_000);
              one(i + 1);
            } else requestAnimationFrame(check);
          };
          requestAnimationFrame(check);
        };
        one(0);
      },
      ids[0] ?? "",
      100,
    )) as number[];
    await sendToPane(ids[0] ?? "", "\x15");
    record("keyEchoP95Ms", p95(samples), "<=", 50, "ms");
  });

  it("unfocused visible terminals coalesce to ≤ 30 writes/s", async () => {
    const pane = ids[5] ?? "";
    const count = async () => ((await browser.execute(() => (window as unknown as { __tandemTest: Hook }).__tandemTest.perf.writes())) as Record<string, number>)[pane] ?? 0;
    const before = await count();
    await browser.pause(3_000);
    record("unfocusedWritesPerSec", ((await count()) - before) / 3, "<=", 30, "writes/s");
  });

  it("focused terminal frame rate under streaming ≥ 50 fps", async () => {
    await sendToPane(ids[0] ?? "", STREAM);
    await browser.pause(1_000);
    const fps = (await browser.executeAsync((done: (fps: number) => void) => {
      let frames = 0;
      const start = performance.now();
      const tick = () => {
        frames++;
        if (performance.now() - start < 3_000) requestAnimationFrame(tick);
        else done((frames * 1000) / (performance.now() - start));
      };
      requestAnimationFrame(tick);
    })) as number;
    record("focusedFps", fps, ">=", 50, "fps");
    for (const id of ids) await sendToPane(id, "\x03");
    await drained();
  });

  it("app idle CPU with 16 panes and no output ≤ 2% of one core, memory ≤ 1.2 GB", async () => {
    await browser.pause(3_000);
    const pids = appPids();
    const hz = Number(execFileSync("getconf", ["CLK_TCK"], { encoding: "utf8" }).trim()) || 100;
    const t0 = cpuTicks(pids);
    await browser.pause(10_000);
    record("idleCpuPercent", ((cpuTicks(pids) - t0) / hz / 10) * 100, "<=", 2, "% of a core");
    record("sixteenPaneRssMb", rssMb(pids), "<=", 1229, "MB");
    console.log(`perf memory: app ${rssMb(appPids().slice(0, 1)).toFixed(0)} MB, webview processes ${rssMb(appPids().slice(1)).toFixed(0)} MB`);
  });

  it("meets every target", () => {
    const missed = Object.entries(metrics).filter(([, m]) => !m.pass);
    expect(missed.map(([name, m]) => `${name}=${m.value}${m.unit}`)).toEqual([]);
  });
});
