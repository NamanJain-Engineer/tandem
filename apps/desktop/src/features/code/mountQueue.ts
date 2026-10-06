// Opening a terminal (xterm.open, measuring the cell, the first render) costs
// tens of milliseconds, more on software GL. A tab that brings six new panes
// would open them all in one frame and freeze the window. New views open one
// per frame instead, the focused one first (F-CD-04: spawns framed ≤ 300 ms,
// no UI stall > 100 ms). When nothing else is opening, a view opens at once.

/** Opens the view; returns its cleanup. */
export type MountJob = () => () => void;

type Entry = { run: MountJob; cleanup?: () => void; cancelled: boolean };

export type Frame = (fn: () => void) => void;

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

  constructor(private frame: Frame = nextFrame) {}

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
    entry.cleanup = entry.run();
    this.frame(() => {
      this.busy = false;
      const next = this.queue.shift();
      if (next) this.start(next);
    });
  }
}

/** The app's queue. */
export const mountQueue = new MountQueue();
