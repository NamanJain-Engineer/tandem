// Opening a terminal (xterm.open, measuring the cell, the first render) costs
// tens of milliseconds, more on software GL. A tab that brings six new panes
// would open them all in one frame and freeze the window. New views open one
// per frame instead, the focused one first (F-CD-04: spawns framed ≤ 300 ms,
// no UI stall > 100 ms). When nothing else is opening, a view opens at once.
// The focused view can hold the queue until it has connected, so its attach
// and spawn replies don't wait behind other views' opens (F-SH-07.7: each
// open blocks the page for hundreds of ms on software GL).

/** Opens the view; returns its cleanup. */
export type MountJob = () => () => void;

type Entry = { run: MountJob; cleanup?: () => void; cancelled: boolean };

export type Frame = (fn: () => void) => void;
export type Wait = (ms: number, fn: () => void) => void;

/** The longest a view holds the queue. */
export const HOLD_MS = 1_000;

/** The next frame, or a timer when the window is hidden (no frames run). */
const nextFrame: Frame = (fn) => {
  let done = false;
  const once = () => {
    if (done) return;
    done = true;
    fn();
  };
  requestAnimationFrame(once);
  setTimeout(once, 100);
};

export class MountQueue {
  private queue: Entry[] = [];
  private busy = false;
  private held: Promise<unknown> | null = null;

  constructor(
    private frame: Frame = nextFrame,
    private wait: Wait = (ms, fn) => void setTimeout(fn, ms),
  ) {}

  /** Called while a view opens: the next one waits until `until` settles
   * (at most HOLD_MS). */
  hold(until: Promise<unknown>): void {
    this.held = until;
  }

  /** Opens now when idle, otherwise on a later frame (`urgent` ones first).
   * The returned function cancels a pending open or cleans up an opened one. */
  add(run: MountJob, urgent = false): () => void {
    const entry: Entry = { run, cancelled: false };
    if (!this.busy && this.queue.length === 0) {
      this.start(entry);
    } else if (urgent) {
      this.queue.unshift(entry);
    } else {
      this.queue.push(entry);
    }
    return () => {
      entry.cancelled = true;
      this.queue = this.queue.filter((e) => e !== entry);
      entry.cleanup?.();
      entry.cleanup = undefined;
    };
  }

  /** Views waiting to open. */
  pending(): number {
    return this.queue.length;
  }

  private start(entry: Entry): void {
    this.busy = true;
    this.held = null;
    entry.cleanup = entry.run();
    // run() may have called hold().
    const held = this.held as Promise<unknown> | null;
    this.held = null;
    const next = () => {
      this.busy = false;
      const entry = this.queue.shift();
      if (entry) this.start(entry);
    };
    this.frame(() => {
      if (!held) return next();
      let done = false;
      const once = () => {
        if (done) return;
        done = true;
        next();
      };
      held.then(once, once);
      this.wait(HOLD_MS, once);
    });
  }
}

/** The app's queue. */
export const mountQueue = new MountQueue();
