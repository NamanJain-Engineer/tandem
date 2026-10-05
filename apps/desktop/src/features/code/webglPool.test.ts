import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WebglPool, type Renderer } from "./webglPool";

function fake(ok = true): Renderer & { attached: boolean; attaches: number } {
  const r = {
    attached: false,
    attaches: 0,
    attach: () => {
      r.attaches++;
      r.attached = ok;
      return ok;
    },
    detach: () => {
      r.attached = false;
    },
  };
  return r;
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("WebGL context pool (F-CD-04)", () => {
  it("F-CD-04.2 at most 8 of 16 visible panes hold a context; the focused one always does", () => {
    const pool = new WebglPool();
    const panes = Array.from({ length: 16 }, (_, i) => ({ id: `p${i}`, r: fake() }));
    for (const p of panes) pool.register(p.id, p.r);
    for (const p of panes) pool.update(p.id, { visible: true, focused: false });
    expect(pool.liveContexts()).toBe(8);
    // The most recently shown panes win.
    expect(panes.slice(8).every((p) => p.r.attached)).toBe(true);
    // Focusing a DOM-rendered pane moves a context to it.
    pool.update("p0", { visible: true, focused: true });
    expect(panes[0]?.r.attached).toBe(true);
    expect(pool.liveContexts()).toBe(8);
  });

  it("F-CD-04.3 a pane hidden for 8 s gives its context back and gets one again on show", () => {
    const pool = new WebglPool();
    const r = fake();
    pool.register("a", r);
    pool.update("a", { visible: true, focused: true });
    expect(pool.liveContexts()).toBe(1);
    pool.update("a", { visible: false, focused: false });
    vi.advanceTimersByTime(7_999);
    expect(pool.holds("a")).toBe(true);
    vi.advanceTimersByTime(1);
    expect(pool.holds("a")).toBe(false);
    expect(pool.liveContexts()).toBe(0);
    pool.update("a", { visible: true, focused: false });
    expect(pool.holds("a")).toBe(true);
    expect(r.attaches).toBe(2);
  });

  it("a hidden holder is evicted when a visible pane needs a context", () => {
    const pool = new WebglPool(2);
    const [a, b, c] = [fake(), fake(), fake()];
    pool.register("a", a);
    pool.register("b", b);
    pool.register("c", c);
    pool.update("a", { visible: true, focused: false });
    pool.update("b", { visible: true, focused: false });
    pool.update("a", { visible: false, focused: false });
    pool.update("c", { visible: true, focused: false });
    expect(a.attached).toBe(false);
    expect(b.attached && c.attached).toBe(true);
  });

  it("without WebGL, panes stay on the DOM renderer; a lost context retries after 2 s", () => {
    const pool = new WebglPool();
    const none = fake(false);
    pool.register("x", none);
    pool.update("x", { visible: true, focused: true });
    expect(pool.liveContexts()).toBe(0);
    const ok = fake();
    pool.register("y", ok);
    pool.update("y", { visible: true, focused: true });
    expect(pool.holds("y")).toBe(true);
    pool.lost("y");
    expect(pool.holds("y")).toBe(false);
    vi.advanceTimersByTime(2_000);
    expect(pool.holds("y")).toBe(true);
    pool.unregister("y");
    expect(pool.liveContexts()).toBe(0);
  });

  it("a remounted view keeps its entry when the old view unmounts late", () => {
    const pool = new WebglPool();
    const old = fake();
    pool.register("p", old);
    pool.update("p", { visible: true, focused: true });
    const fresh = fake();
    pool.register("p", fresh);
    expect(old.attached).toBe(false);
    pool.update("p", { visible: true, focused: true });
    pool.unregister("p", old);
    expect(pool.holds("p")).toBe(true);
    expect(fresh.attached).toBe(true);
  });
});
