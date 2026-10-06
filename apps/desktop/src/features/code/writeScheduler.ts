// Output pacing across panes (F-CD-04, ARCHITECTURE §5.5). xterm parses on
// the main thread, so 16 panes streaming at full speed would leave no time
// for input, rendering or the focused pane. The focused pane writes at once.
// Every other pane's output waits here and is handed to xterm from one
// timer loop: round-robin, at most BACKGROUND_BYTES per tick, and only once
// the previous tick's output has been parsed, so the main thread always has
// idle time. Each pane is visited at most every COALESCE_MS (≤30 fps).
// Output that waits isn't acked, so Rust's ack window holds the programs
// back. Timers (not animation frames) keep hidden panes and minimised
// windows draining, so agents never stall on a window nobody looks at.

export const BACKGROUND_BYTES = 256 * 1024;
export const TICK_MS = 16;
export const COALESCE_MS = 33;

/** Hands bytes to xterm; `parsed` runs once xterm has processed them. */
export type PaneWriter = (data: Uint8Array, parsed: () => void) => void;

type Queue = { write: PaneWriter; chunks: Uint8Array[]; bytes: number; lastFlush: number };

export type Clock = {
  now: () => number;
  setTimer: (fn: () => void, ms: number) => unknown;
};

const realClock: Clock = {
  now: () => performance.now(),
  setTimer: (fn, ms) => setTimeout(fn, ms),
};

export class WriteScheduler {
  private queues = new Map<string, Queue>();
  private order: string[] = [];
  private next = 0;
  private inFlight = 0;
  private timer: unknown = null;

  constructor(
    private clock: Clock = realClock,
    private budget = BACKGROUND_BYTES,
  ) {}

  register(paneId: string, write: PaneWriter): void {
    this.queues.set(paneId, { write, chunks: [], bytes: 0, lastFlush: -Infinity });
    if (!this.order.includes(paneId)) this.order.push(paneId);
  }

  /** With `write`, only if that view still owns the pane (a remount may have replaced it). */
  unregister(paneId: string, write?: PaneWriter): void {
    const q = this.queues.get(paneId);
    if (!q || (write && q.write !== write)) return;
    this.queues.delete(paneId);
    this.order = this.order.filter((id) => id !== paneId);
  }

  /** Background output for a pane; it reaches xterm on a later tick. */
  push(paneId: string, data: Uint8Array): void {
    const q = this.queues.get(paneId);
    if (!q || data.byteLength === 0) return;
    q.chunks.push(data);
    q.bytes += data.byteLength;
    this.schedule();
  }

  /** Bytes waiting for a pane. */
  pending(paneId: string): number {
    return this.queues.get(paneId)?.bytes ?? 0;
  }

  /** Writes everything waiting for a pane now (it was focused, or exited). */
  flush(paneId: string): void {
    const q = this.queues.get(paneId);
    if (!q || q.bytes === 0) return;
    const data = take(q, q.bytes);
    q.lastFlush = this.clock.now();
    q.write(data, () => undefined);
  }

  private schedule(): void {
    if (this.timer !== null) return;
    this.timer = this.clock.setTimer(() => {
      this.timer = null;
      this.tick();
    }, TICK_MS);
  }

  private tick(): void {
    // The last tick's output is still being parsed: give the thread back.
    if (this.inFlight > 0) {
      this.schedule();
      return;
    }
    let budget = this.budget;
    const now = this.clock.now();
    const n = this.order.length;
    for (let i = 0; i < n && budget > 0; i++) {
      const id = this.order[(this.next + i) % n] ?? "";
      const q = this.queues.get(id);
      if (!q || q.bytes === 0 || now - q.lastFlush < COALESCE_MS) continue;
      const data = take(q, Math.min(q.bytes, budget));
      budget -= data.byteLength;
      q.lastFlush = now;
      this.inFlight++;
      q.write(data, () => {
        this.inFlight--;
      });
      this.next = (this.next + i + 1) % n;
    }
    if ([...this.queues.values()].some((q) => q.bytes > 0)) this.schedule();
  }
}

/** Removes and merges the first `count` bytes of a queue. */
function take(q: Queue, count: number): Uint8Array {
  const out = new Uint8Array(count);
  let at = 0;
  while (at < count) {
    const head = q.chunks[0];
    if (!head) break;
    const n = Math.min(head.byteLength, count - at);
    out.set(head.subarray(0, n), at);
    at += n;
    if (n === head.byteLength) q.chunks.shift();
    else q.chunks[0] = head.subarray(n);
  }
  q.bytes -= at;
  return at === count ? out : out.subarray(0, at);
}

/** The app's scheduler. */
export const writeScheduler = new WriteScheduler();
