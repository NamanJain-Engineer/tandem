// `pnpm --filter desktop perf -- --suite threads` (MILESTONES M7, UI-21,
// ARCHITECTURE §17 perf targets). A thread of 5,000 stored events: opening it
// to its last message, scrolling the transcript, loading an older page, and a
// reply holding a 2 MB code fence. Timings are taken in the page (no
// WebDriver round trips). Writes perf/results/m7.json. A miss only a
// reference machine can settle is recorded as [manual] in BUILD_LOG, never
// loosened here.
//
// The first thread opened in a session pays a one-time paint warm-up
// (whatever its size: on a software-GL VM it's most of a second). A small
// thread opens first and that cost is recorded as a baseline, so the
// targets measure what a thread's size costs.
import { browser } from "@wdio/globals";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { cpus, release, totalmem } from "node:os";
import { join } from "node:path";
import { freshOffline, invoke } from "../support/app";
import { threadMode } from "../support/thread";

const ROOT = new URL("../../../..", import.meta.url).pathname;
const OUT = join(ROOT, "perf/results/m7.json");
const EVENTS = 5_000;

type Metric = { value: number; target: number; op: "<=" | ">="; unit: string; pass: boolean };
const metrics: Record<string, Metric> = {};
function record(name: string, value: number, op: "<=" | ">=", target: number, unit: string): void {
  const v = Math.round(value * 10) / 10;
  metrics[name] = { value: v, target, op, unit, pass: op === "<=" ? v <= target : v >= target };
  console.log(`perf ${name}: ${v} ${unit} (target ${op} ${target})`);
}

async function seeded(events: number, fenceBytes?: number): Promise<string> {
  const thread = await invoke<{ id: string }>("thread_create", { args: { kind: "thread", workspaceId: null } });
  await invoke("test_thread_seed_events", { threadId: thread.id, events, fenceBytes: fenceBytes ?? null });
  await browser.waitUntil(
    async () => browser.execute((id: string) => !!document.querySelector(`[data-testid="thread-row"][data-thread-id="${id}"]`), thread.id),
    { timeout: 10_000 },
  );
  return thread.id;
}

/** The longest gap between frames from clicking a thread until a second
 * after its reply shows. */
async function openWorstFrame(id: string): Promise<number> {
  return (await browser.executeAsync(
    (id: string, done: (ms: number) => void) => {
      const t0 = performance.now();
      let prev = t0;
      let worstGap = 0;
      let shownAt = 0;
      const tick = (now: number) => {
        worstGap = Math.max(worstGap, now - prev);
        prev = now;
        const shown = !!document.querySelector(`[data-testid="thread-main"][data-thread-id="${id}"] [data-testid="thread-assistant"]`);
        if (shown && !shownAt) shownAt = now;
        if (shownAt && now - shownAt > 1_000) return done(worstGap);
        if (now - t0 > 20_000) return done(worstGap);
        requestAnimationFrame(tick);
      };
      requestAnimationFrame((now) => {
        prev = now;
        document.querySelector<HTMLElement>(`[data-testid="thread-row"][data-thread-id="${id}"]`)?.click();
        requestAnimationFrame(tick);
      });
    },
    id,
  )) as number;
}

const baseline: Record<string, number> = {};

describe("perf: threads (M7)", () => {
  let big = "";

  after(() => {
    mkdirSync(join(ROOT, "perf/results"), { recursive: true });
    const prior = existsSync(OUT) ? (JSON.parse(readFileSync(OUT, "utf8")) as Record<string, unknown>) : {};
    const body = {
      ...prior,
      threads: {
        date: new Date().toISOString(),
        machine: {
          os: `linux ${release()} (xvfb)`,
          cpus: cpus().length,
          cpuModel: cpus()[0]?.model,
          memGb: Math.round(totalmem() / 2 ** 30),
          build: process.env.TANDEM_APP_BINARY?.includes("/perf/") ? "perf" : "debug",
        },
        metrics,
        baseline,
      },
    };
    writeFileSync(OUT, `${JSON.stringify(body, null, 2)}\n`);
  });

  before(async function () {
    this.timeout(300_000);
    await freshOffline();
    await threadMode();
    const small = await seeded(60);
    big = await seeded(EVENTS);
    const cold = (await browser.executeAsync(
      (id: string, done: (r: { openMs: number; worstFrameMs: number }) => void) => {
        const t0 = performance.now();
        let prev = t0;
        let worst = 0;
        let openMs = 0;
        document.querySelector<HTMLElement>(`[data-testid="thread-row"][data-thread-id="${id}"]`)?.click();
        const tick = (now: number) => {
          worst = Math.max(worst, now - prev);
          prev = now;
          if (!openMs && document.querySelector('[data-testid="thread-user"]')) openMs = now - t0;
          if (openMs && now - t0 > openMs + 1_500) return done({ openMs, worstFrameMs: worst });
          if (now - t0 > 20_000) return done({ openMs: openMs || 20_000, worstFrameMs: worst });
          requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
      },
      small,
    )) as { openMs: number; worstFrameMs: number };
    baseline.coldFirstOpenMs = Math.round(cold.openMs);
    baseline.coldFirstWorstFrameMs = Math.round(cold.worstFrameMs);
    console.log(`perf cold first open (baseline, no target): ${baseline.coldFirstOpenMs} ms, worst frame ${baseline.coldFirstWorstFrameMs} ms`);
  });

  it("UI-21 a 5,000-event thread opens to its last message within 500 ms", async function () {
    this.timeout(60_000);
    const last = Math.ceil(EVENTS / 6) - 1;
    const ms = (await browser.executeAsync(
      (id: string, needle: string, done: (ms: number) => void) => {
        const t0 = performance.now();
        document.querySelector<HTMLElement>(`[data-testid="thread-row"][data-thread-id="${id}"]`)?.click();
        const check = () => {
          const users = document.querySelectorAll('[data-testid="thread-user"] p');
          const lastUser = users[users.length - 1];
          if (lastUser?.textContent?.includes(needle)) done(performance.now() - t0);
          else requestAnimationFrame(check);
        };
        requestAnimationFrame(check);
      },
      big,
      `Message ${last}:`,
    )) as number;
    record("threadOpenMs", ms, "<=", 500, "ms");
  });

  it("UI-21 the transcript scrolls at 60 fps (at least 55)", async function () {
    this.timeout(60_000);
    const fps = (await browser.executeAsync((done: (fps: number) => void) => {
      const log = document.querySelector<HTMLElement>('[data-testid="thread-main"] [role="log"]');
      if (!log) return done(0);
      // Within the loaded rows, away from the top (which loads a page).
      const top = Math.max(log.scrollHeight - log.clientHeight, 0);
      const span = Math.min(top * 0.6, 6_000);
      let frames = 0;
      const start = performance.now();
      const step = (now: number) => {
        const t = (now - start) / 2_000;
        if (t >= 1) return done(frames / ((now - start) / 1000));
        frames++;
        // Down and back up, a steady scroll.
        log.scrollTop = top - span * (t < 0.5 ? t * 2 : (1 - t) * 2);
        requestAnimationFrame(step);
      };
      requestAnimationFrame(step);
    })) as number;
    record("transcriptScrollFps", fps, ">=", 55, "fps");
  });

  it("ARCHITECTURE §17 scrolling to the top loads an older page in under 100 ms", async function () {
    this.timeout(60_000);
    const ms = (await browser.executeAsync((done: (ms: number) => void) => {
      const log = document.querySelector<HTMLElement>('[data-testid="thread-main"] [role="log"]');
      if (!log) return done(-1);
      const firstUser = () => document.querySelector('[data-testid="thread-user"] p')?.textContent ?? "";
      const before = firstUser();
      const t0 = performance.now();
      log.scrollTop = 0;
      log.dispatchEvent(new Event("scroll"));
      const check = () => {
        if (firstUser() !== before && firstUser() !== "") done(performance.now() - t0);
        else if (performance.now() - t0 > 5_000) done(5_000);
        else requestAnimationFrame(check);
      };
      requestAnimationFrame(check);
    })) as number;
    record("olderPageMs", ms, "<=", 99, "ms");
  });

  it("baseline: the frame rate and worst frame of an idle thread page (this machine)", async function () {
    this.timeout(30_000);
    const idle = (await browser.executeAsync((done: (r: { fps: number; worstMs: number }) => void) => {
      let prev = performance.now();
      let worst = 0;
      let frames = 0;
      const start = prev;
      const tick = (now: number) => {
        worst = Math.max(worst, now - prev);
        prev = now;
        if (now - start > 2_000) return done({ fps: frames / ((now - start) / 1000), worstMs: worst });
        frames++;
        requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    })) as { fps: number; worstMs: number };
    baseline.idleFps = Math.round(idle.fps * 10) / 10;
    baseline.idleWorstFrameMs = Math.round(idle.worstMs);
    console.log(`perf idle (baseline, no target): ${baseline.idleFps} fps, worst frame ${baseline.idleWorstFrameMs} ms`);
  });

  it("UI-21 a reply with a 2 MB code fence never blocks the page for more than 50 ms", async function () {
    this.timeout(120_000);
    // The same measure for a plain 6-event thread, as this machine's baseline.
    const plain = await seeded(6);
    const fence = await seeded(6, 2 * 1024 * 1024);
    baseline.plainOpenWorstFrameMs = Math.round(await openWorstFrame(plain));
    console.log(`perf plain thread open worst frame (baseline, no target): ${baseline.plainOpenWorstFrameMs} ms`);
    await browser.pause(500);
    record("bigFenceWorstFrameMs", await openWorstFrame(fence), "<=", 50, "ms");
  });

  it("meets every target", () => {
    const missed = Object.entries(metrics).filter(([, m]) => !m.pass);
    expect(missed.map(([name, m]) => `${name}=${m.value}${m.unit}`)).toEqual([]);
  });
});
