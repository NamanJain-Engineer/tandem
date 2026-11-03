// Opening a terminal (xterm.open, measuring the cell, the first render) costs
// tens of milliseconds, more on software GL. A tab that brings six new panes
// would open them all in one frame and freeze the window. New views open one
// per frame instead, the focused one first (F-CD-04: spawns framed ≤ 300 ms,
// no UI stall > 100 ms). When nothing else is opening, a view opens at once.
// The focused view can hold the queue until it has connected, so its attach
// and spawn replies don't wait behind other views' opens (F-SH-07.7: each
// open blocks the page for hundreds of ms on software GL). Views on screen open
// before the queued views of a tab that was hidden since they were added. A
// view that has to wait starts its pane's process meanwhile (prestart.ts).
// Views not on screen don't open while one on screen hasn't shown its first
// output: their opens would come between it and that output (F-CD-03 AC2).

/** Opens the view; returns its cleanup. */
export type MountJob = () => () => void;

type Entry = { run: MountJob; visible: () => boolean; cleanup?: () => void; cancelled: boolean };

export type Frame = (fn: () => void) => void;
export type Wait = (ms: number, fn: () => void) => void;

/** The longest a view holds the queue. */
export const HOLD_MS = 1_000;

/** The longest a view on screen keeps views not on screen from opening. */
export const SHOWN_HOLD_MS = 1_000;

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
  /** Views on screen that haven't shown their first output yet. */
  private shownWaiting = 0;

  constructor(
    private frame: Frame = nextFrame,
    private wait: Wait = (ms, fn) => void setTimeout(fn, ms),
  ) {}

  /** Called while a view opens: the next one waits until `until` settles
   * (at most HOLD_MS). */
  hold(until: Promise<unknown>): void {
    this.held = until;
  }

  /** Called while a view on screen opens: views not on screen wait to open
   * until `until` settles (its first output, at most SHOWN_HOLD_MS); views
   * on screen still open one per frame. */
  holdHidden(until: Promise<unknown>): void {
    this.shownWaiting += 1;
    let done = false;
    const release = () => {
      if (done) return;
      done = true;
      this.shownWaiting -= 1;
      this.pick();
    };
    until.then(release, release);
    this.wait(SHOWN_HOLD_MS, release);
  }

  /** Opens now when idle, otherwise on a later frame: `urgent` ones first,
   * then those `visible` says are on screen when their turn comes. The
   * returned function cancels a pending open or cleans up an opened one. */
  add(run: MountJob, urgent = false, visible: () => boolean = () => true): () => void {
    const entry: Entry = { run, visible, cancelled: false };
    if (urgent) this.queue.unshift(entry);
    else this.queue.push(entry);
    this.pick();
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

  /** True when a view added now would wait for its turn rather than open at
   * once (its pane can start meanwhile, F-CD-03 AC2): behind an open, a view
   * on screen behind others on screen, one not on screen behind anything. */
  wouldWait(visible = true, urgent = false): boolean {
    if (this.busy) return true;
    if (visible) return !urgent && this.queue.some((e) => e.visible());
    return this.queue.length > 0 || this.shownWaiting > 0;
  }

  /** Opens the next view when none is opening: the first on screen, else the
   * first, unless a view on screen is still waiting for its first output. */
  private pick(): void {
    if (this.busy) return;
    const shown = this.queue.findIndex((e) => e.visible());
    if (shown < 0 && this.shownWaiting > 0) return;
    const [entry] = this.queue.splice(Math.max(shown, 0), 1);
    if (entry) this.start(entry);
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
      this.pick();
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
