// The WebGL context pool (ARCHITECTURE §5.6, F-CD-04): at most 8 panes hold a
// WebGL renderer. The focused pane comes first, then visible panes by how
// recently they were focused or shown; everyone else uses xterm's DOM
// renderer (same model, so nothing is lost when a pane changes renderer).
//
// Changes are batched: every pane a layout or mode switch shows in one commit
// counts as shown at the same moment, so they don't evict each other one by
// one. Creating a WebGL context is slow (tens of ms, far more on software GL),
// so contexts are attached one per frame after the switch has painted.

export const POOL_CAPACITY = 8;
/** A hidden pane gives its context back after this long (F-CD-04 AC3). */
export const HIDDEN_RELEASE_MS = 8_000;
/** Gap between two context attaches, so a frame can paint in between. */
export const ATTACH_GAP_MS = 16;

/** What the pool needs from a pane's terminal. */
export type Renderer = {
  /** Loads the WebGL addon; false when WebGL isn't available. */
  attach: () => boolean;
  /** Disposes the WebGL addon (xterm falls back to the DOM renderer). */
  detach: () => void;
};

type Entry = {
  renderer: Renderer;
  focused: boolean;
  visible: boolean;
  /** Batch in which it was last focused or shown, for eviction order. */
  touched: number;
  /** Registration order, to break ties. */
  order: number;
  holds: boolean;
  /** Its last attach failed: not retried until it's shown or focused again. */
  failed: boolean;
  hideTimer?: ReturnType<typeof setTimeout>;
};

type Listener = (paneId: string, webgl: boolean) => void;

export class WebglPool {
  private entries = new Map<string, Entry>();
  private listeners = new Set<Listener>();
  private clock = 0;
  private registered = 0;
  /** The stamp shared by every change until the next rebalance runs. */
  private stamp: number | null = null;
  private pending: ReturnType<typeof setTimeout> | null = null;

  constructor(
    private capacity = POOL_CAPACITY,
    private hiddenReleaseMs = HIDDEN_RELEASE_MS,
  ) {}

  /** A pane joins (mounted). */
  register(paneId: string, renderer: Renderer): void {
    const prev = this.entries.get(paneId);
    if (prev) {
      clearTimeout(prev.hideTimer);
      if (prev.holds) prev.renderer.detach();
    }
    this.entries.set(paneId, {
      renderer,
      focused: false,
      visible: false,
      touched: this.touch(),
      order: ++this.registered,
      holds: false,
      failed: false,
    });
  }

  /** A pane leaves (unmounted): its context goes back. With `renderer`, only
   * if that view still owns the entry (a remount may have replaced it). */
  unregister(paneId: string, renderer?: Renderer): void {
    const e = this.entries.get(paneId);
    if (!e || (renderer && e.renderer !== renderer)) return;
    clearTimeout(e.hideTimer);
    if (e.holds) e.renderer.detach();
    this.entries.delete(paneId);
    this.request();
  }

  /** Visibility and focus changed for a pane. */
  update(paneId: string, state: { visible: boolean; focused: boolean }): void {
    const e = this.entries.get(paneId);
    if (!e) return;
    const becameVisible = state.visible && !e.visible;
    const becameFocused = state.focused && !e.focused;
    e.visible = state.visible;
    e.focused = state.focused && state.visible;
    if (becameVisible || becameFocused) {
      e.touched = this.touch();
      e.failed = false;
    }
    clearTimeout(e.hideTimer);
    e.hideTimer = undefined;
    if (!state.visible && e.holds) {
      // Hidden panes keep their context a while (a quick tab flip shouldn't
      // churn GPU contexts), then give it back.
      e.hideTimer = setTimeout(() => {
        e.hideTimer = undefined;
        if (!e.visible && e.holds) this.drop(paneId, e);
        this.request();
      }, this.hiddenReleaseMs);
    }
    this.request();
  }

  /** The renderer lost its context: drop it and try again in 2 s. */
  lost(paneId: string): void {
    const e = this.entries.get(paneId);
    if (!e?.holds) return;
    this.drop(paneId, e);
    setTimeout(() => this.request(), 2_000);
  }

  /** The GPU setting changed: every pane starts over (attach is refused
   * while it's off, so all fall back to the DOM renderer). */
  reset(): void {
    for (const [id, e] of this.entries) {
      e.failed = false;
      if (e.holds) this.drop(id, e);
    }
    this.request();
  }

  liveContexts(): number {
    let n = 0;
    for (const e of this.entries.values()) if (e.holds) n++;
    return n;
  }

  holds(paneId: string): boolean {
    return this.entries.get(paneId)?.holds ?? false;
  }

  /** Notified when a pane switches renderer (the "Lite" badge). */
  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private drop(paneId: string, e: Entry): void {
    e.renderer.detach();
    e.holds = false;
    for (const l of this.listeners) l(paneId, false);
  }

  private touch(): number {
    if (this.stamp === null) this.stamp = ++this.clock;
    return this.stamp;
  }

  /** Rebalances once the current batch of changes is in. */
  private request(delay = 0): void {
    if (this.pending !== null) return;
    this.pending = setTimeout(() => {
      this.pending = null;
      this.rebalance();
    }, delay);
  }

  /** Gives contexts to the highest-priority visible panes: frees the ones no
   * longer wanted, attaches one, and comes back next frame for the rest. */
  private rebalance(): void {
    this.stamp = null;
    const ranked = [...this.entries.entries()]
      .filter(([, e]) => e.visible)
      .sort(
        ([, a], [, b]) =>
          Number(b.focused) - Number(a.focused) ||
          b.touched - a.touched ||
          // Shown together: keep the contexts already held, then pane order.
          Number(b.holds) - Number(a.holds) ||
          a.order - b.order,
      );
    const wanted = new Set(ranked.slice(0, this.capacity).map(([id]) => id));
    // Free contexts first: lower-priority visible holders, then hidden ones
    // only when the pool is actually full.
    for (const [id, e] of this.entries) {
      if (e.holds && e.visible && !wanted.has(id)) this.drop(id, e);
    }
    const missing = ranked.filter(([id, e]) => wanted.has(id) && !e.holds && !e.failed);
    const next = missing[0];
    if (!next) return;
    const [id, e] = next;
    if (this.liveContexts() >= this.capacity) {
      // Evict the hidden holder seen longest ago.
      const victim = [...this.entries.entries()]
        .filter(([, h]) => h.holds && !h.visible)
        .sort(([, a], [, b]) => a.touched - b.touched || a.order - b.order)[0];
      if (!victim) return;
      clearTimeout(victim[1].hideTimer);
      this.drop(victim[0], victim[1]);
    }
    if (e.renderer.attach()) {
      e.holds = true;
      for (const l of this.listeners) l(id, true);
    } else {
      // Stays on the DOM renderer until it's shown or focused again.
      e.failed = true;
    }
    // The rest get their turn next frame.
    if (missing.length > 1) this.request(ATTACH_GAP_MS);
  }
}

/** The app's pool. */
export const webglPool = new WebglPool();
