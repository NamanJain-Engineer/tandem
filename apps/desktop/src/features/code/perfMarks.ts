// Timings the perf suite reads in debug builds (MILESTONES M3 perf:
// "6 spawns framed ≤300 ms"): when each pane's view mounted and when its
// first output reached xterm. Nothing is recorded unless the harness asks.

export type PaneTimes = { mounted: number; firstOutput?: number; phases?: Record<string, number> };

const times = new Map<string, PaneTimes>();
const writes = new Map<string, number>();
const KEY = "tandem.perfMarks";

function stored(): boolean {
  try {
    return sessionStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

// Survives a reload, so the suite can time a fresh mount.
let enabled = stored();

/** Turns recording on (the harness's `__tandemTest.perf.enable()`). */
export function enablePerfMarks(): void {
  enabled = true;
  try {
    sessionStorage.setItem(KEY, "1");
  } catch {
    /* private mode: this page only */
  }
}

/** One xterm write (flush) for a pane: unfocused panes coalesce to ≤30/s. */
export function markWrite(paneId: string): void {
  if (enabled) writes.set(paneId, (writes.get(paneId) ?? 0) + 1);
}

export function writeCounts(): Record<string, number> {
  return Object.fromEntries(writes);
}

export function markMounted(paneId: string): void {
  if (!enabled) return;
  const prev = times.get(paneId);
  times.set(paneId, { mounted: performance.now(), phases: prev?.phases });
}

/** A named step of a pane's start (effect, spawned, first frame…), once. */
export function markPhase(paneId: string, phase: string): void {
  if (!enabled) return;
  const t = times.get(paneId) ?? { mounted: NaN };
  t.phases ??= {};
  t.phases[phase] ??= performance.now();
  times.set(paneId, t);
}

export function markFirstOutput(paneId: string): void {
  if (!enabled) return;
  const t = times.get(paneId);
  if (t && t.firstOutput === undefined) t.firstOutput = performance.now();
}

export function perfTimes(): Record<string, PaneTimes> {
  return Object.fromEntries(times);
}

export type Span = { name: string; at: number; ms: number };
const spans: Span[] = [];

/** Times `fn` as a named span (WebGL attach, detach…) when recording. */
export function timed<T>(name: string, fn: () => T): T {
  if (!enabled) return fn();
  const at = performance.now();
  try {
    return fn();
  } finally {
    spans.push({ name, at, ms: performance.now() - at });
  }
}

export function perfSpans(): Span[] {
  return spans.slice();
}

const reflows = new Map<string, number>();

/** A terminal's grid changed size (cols × rows): the reflow F-SH-10 counts.
 * Always counted (one map write per resize). */
export function markReflow(paneId: string): void {
  reflows.set(paneId, (reflows.get(paneId) ?? 0) + 1);
}

export function reflowCounts(reset = false): Record<string, number> {
  const out = Object.fromEntries(reflows);
  if (reset) reflows.clear();
  return out;
}
