// Timings the perf suite reads in debug builds (MILESTONES M3 perf:
// "6 spawns framed ≤300 ms"): when each pane's view mounted and when its
// first output reached xterm. Nothing is recorded unless the harness asks.

export type PaneTimes = { mounted: number; firstOutput?: number };

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
  if (enabled) times.set(paneId, { mounted: performance.now() });
}

export function markFirstOutput(paneId: string): void {
  if (!enabled) return;
  const t = times.get(paneId);
  if (t && t.firstOutput === undefined) t.firstOutput = performance.now();
}

export function perfTimes(): Record<string, PaneTimes> {
  return Object.fromEntries(times);
}
