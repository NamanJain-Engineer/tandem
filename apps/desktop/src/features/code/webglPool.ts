// The WebGL context pool (ARCHITECTURE §5.6, F-CD-04): at most 8 panes hold a
// WebGL renderer. The focused pane comes first, then visible panes by how
// recently they were focused or shown; everyone else uses xterm's DOM
// renderer (same model, so nothing is lost when a pane changes renderer).

export const POOL_CAPACITY = 8;
/** A hidden pane gives its context back after this long (F-CD-04 AC3). */
export const HIDDEN_RELEASE_MS = 8_000;

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
  /** Last time it was focused or shown, for eviction order. */
  touched: number;
  holds: boolean;
  hideTimer?: ReturnType<typeof setTimeout>;
};

type Listener = (paneId: string, webgl: boolean) => void;

export class WebglPool {
  private entries = new Map<string, Entry>();
  private listeners = new Set<Listener>();
  private clock = 0;

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
    this.entries.set(paneId, { renderer, focused: false, visible: false, touched: ++this.clock, holds: false });
  }

  /** A pane leaves (unmounted): its context goes back. With `renderer`, only
   * if that view still owns the entry (a remount may have replaced it). */
  unregister(paneId: string, renderer?: Renderer): void {
    const e = this.entries.get(paneId);
    if (!e || (renderer && e.renderer !== renderer)) return;
    clearTimeout(e.hideTimer);
    if (e.holds) e.renderer.detach();
    this.entries.delete(paneId);
    this.rebalance();
  }

  /** Visibility and focus changed for a pane. */
  update(paneId: string, state: { visible: boolean; focused: boolean }): void {
    const e = this.entries.get(paneId);
    if (!e) return;
    const becameVisible = state.visible && !e.visible;
    const becameFocused = state.focused && !e.focused;
    e.visible = state.visible;
    e.focused = state.focused && state.visible;
    if (becameVisible || becameFocused) e.touched = ++this.clock;
    clearTimeout(e.hideTimer);
    e.hideTimer = undefined;
    if (!state.visible && e.holds) {
      // Hidden panes keep their context a while (a quick tab flip shouldn't
      // churn GPU contexts), then give it back.
      e.hideTimer = setTimeout(() => {
        e.hideTimer = undefined;
        if (!e.visible && e.holds) this.drop(paneId, e);
        this.rebalance();
      }, this.hiddenReleaseMs);
    }
    this.rebalance();
  }

  /** The renderer lost its context: drop it and try again in 2 s. */
  lost(paneId: string): void {
    const e = this.entries.get(paneId);
    if (!e?.holds) return;
    this.drop(paneId, e);
    setTimeout(() => this.rebalance(), 2_000);
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

  /** Gives contexts to the highest-priority visible panes. */
  private rebalance(): void {
    const ranked = [...this.entries.entries()]
      .filter(([, e]) => e.visible)
      .sort(([, a], [, b]) => Number(b.focused) - Number(a.focused) || b.touched - a.touched);
    const wanted = new Set(ranked.slice(0, this.capacity).map(([id]) => id));
    // Free contexts first: lower-priority visible holders, then hidden ones
    // only when the pool is actually full.
    for (const [id, e] of this.entries) {
      if (e.holds && e.visible && !wanted.has(id)) this.drop(id, e);
    }
    let live = this.liveContexts();
    for (const [id] of ranked) {
      if (!wanted.has(id)) continue;
      const e = this.entries.get(id);
      if (!e || e.holds) continue;
      if (live >= this.capacity) {
        // Evict the hidden holder seen longest ago.
        const victim = [...this.entries.entries()]
          .filter(([, h]) => h.holds && !h.visible)
          .sort(([, a], [, b]) => a.touched - b.touched)[0];
        if (!victim) break;
        clearTimeout(victim[1].hideTimer);
        this.drop(victim[0], victim[1]);
        live--;
      }
      if (e.renderer.attach()) {
        e.holds = true;
        live++;
        for (const l of this.listeners) l(id, true);
      }
    }
  }
}

/** The app's pool. */
export const webglPool = new WebglPool();
