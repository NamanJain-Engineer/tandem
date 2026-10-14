import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ATTACH_GAP_MS, WebglPool, type Renderer } from "./webglPool";

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

/** Lets queued rebalances and staggered attaches run. */
const settle = () => vi.advanceTimersByTime(ATTACH_GAP_MS * 20);

describe("WebGL context pool (F-CD-04)", () => {
  it("F-CD-04.2 at most 8 of 16 visible panes hold a context; the focused one always does", () => {
    const pool = new WebglPool();
    const panes = Array.from({ length: 16 }, (_, i) => ({ id: `p${i}`, r: fake() }));
    for (const p of panes) pool.register(p.id, p.r);
    for (const p of panes) pool.update(p.id, { visible: true, focused: false });
    settle();
    expect(pool.liveContexts()).toBe(8);
    // Shown together: pane order decides.
    expect(panes.slice(0, 8).every((p) => p.r.attached)).toBe(true);
    // Focusing a DOM-rendered pane moves a context to it.
    pool.update("p12", { visible: true, focused: true });
    settle();
    expect(panes[12]?.r.attached).toBe(true);
    expect(pool.liveContexts()).toBe(8);
    // Panes shown later win over those shown earlier.
    pool.update("p12", { visible: true, focused: false });
    for (const p of panes.slice(13)) pool.update(p.id, { visible: false, focused: false });
    settle();
    for (const p of panes.slice(13)) pool.update(p.id, { visible: true, focused: false });
    settle();
    expect(panes.slice(13).every((p) => p.r.attached)).toBe(true);
    expect(pool.liveContexts()).toBe(8);
  });

  it("F-CD-04.2 showing 16 panes again doesn't churn contexts: holders keep theirs", () => {
    const pool = new WebglPool();
    const panes = Array.from({ length: 16 }, (_, i) => ({ id: `p${i}`, r: fake() }));
    for (const p of panes) pool.register(p.id, p.r);
    for (const p of panes) pool.update(p.id, { visible: true, focused: p.id === "p9" });
    settle();
    const held = panes.filter((p) => p.r.attached).map((p) => p.id);
    expect(held).toHaveLength(8);
    expect(held).toContain("p9");
    // A mode switch hides every pane, then shows them all in one commit.
    for (const p of panes) pool.update(p.id, { visible: false, focused: false });
    settle();
    for (const p of panes) pool.update(p.id, { visible: true, focused: p.id === "p9" });
    settle();
    expect(panes.filter((p) => p.r.attached).map((p) => p.id)).toEqual(held);
    expect(panes.reduce((n, p) => n + p.r.attaches, 0)).toBe(8);
  });

  it("F-SH-07.7 F-CD-04.2 16 panes opening one per frame keep the first 8 contexts: none is detached", () => {
    // The mount queue opens one view per frame, so each registers in its own
    // batch; they were all shown together, so none is newer than the rest.
    const pool = new WebglPool();
    let detaches = 0;
    const panes = Array.from({ length: 16 }, (_, i) => {
      const r = fake();
      const detach = r.detach;
      r.detach = () => {
        detaches++;
        detach();
      };
      return { id: `p${i}`, r };
    });
    for (const p of panes) {
      pool.register(p.id, p.r, { visible: true, focused: p.id === "p0" });
      settle();
    }
    expect(pool.liveContexts()).toBe(8);
    expect(panes.slice(0, 8).every((p) => p.r.attached)).toBe(true);
    expect(detaches).toBe(0);
    expect(panes.reduce((n, p) => n + p.r.attaches, 0)).toBe(8);
  });

  it("F-CD-04.2 contexts are attached one per frame, the focused pane first, after the switch", () => {
    const pool = new WebglPool();
    const panes = Array.from({ length: 4 }, (_, i) => ({ id: `p${i}`, r: fake() }));
    for (const p of panes) pool.register(p.id, p.r);
    for (const p of panes) pool.update(p.id, { visible: true, focused: p.id === "p3" });
    // Nothing happens inside the commit itself.
    expect(pool.liveContexts()).toBe(0);
    vi.advanceTimersByTime(0);
    expect(pool.liveContexts()).toBe(1);
    expect(panes[3]?.r.attached).toBe(true);
    vi.advanceTimersByTime(ATTACH_GAP_MS);
    expect(pool.liveContexts()).toBe(2);
    settle();
    expect(pool.liveContexts()).toBe(4);
  });

  it("F-CD-04.3 a pane hidden for 8 s gives its context back and gets one again on show", () => {
    const pool = new WebglPool();
    const r = fake();
    pool.register("a", r);
    pool.update("a", { visible: true, focused: true });
    settle();
    expect(pool.liveContexts()).toBe(1);
    pool.update("a", { visible: false, focused: false });
    settle();
    vi.advanceTimersByTime(7_999 - ATTACH_GAP_MS * 20);
    expect(pool.holds("a")).toBe(true);
    vi.advanceTimersByTime(1);
    expect(pool.holds("a")).toBe(false);
    expect(pool.liveContexts()).toBe(0);
    pool.update("a", { visible: true, focused: false });
    settle();
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
    settle();
    pool.update("a", { visible: false, focused: false });
    pool.update("c", { visible: true, focused: false });
    settle();
    expect(a.attached).toBe(false);
    expect(b.attached && c.attached).toBe(true);
  });

  it("without WebGL, panes stay on the DOM renderer; a lost context retries after 2 s", () => {
    const pool = new WebglPool();
    const none = fake(false);
    pool.register("x", none);
    pool.update("x", { visible: true, focused: true });
    settle();
    expect(pool.liveContexts()).toBe(0);
    // A failed attach isn't retried every frame.
    expect(none.attaches).toBe(1);
    const ok = fake();
    pool.register("y", ok);
    pool.update("x", { visible: true, focused: false });
    pool.update("y", { visible: true, focused: true });
    settle();
    expect(pool.holds("y")).toBe(true);
    pool.lost("y");
    expect(pool.holds("y")).toBe(false);
    vi.advanceTimersByTime(2_000);
    settle();
    expect(pool.holds("y")).toBe(true);
    pool.unregister("y");
    expect(pool.liveContexts()).toBe(0);
  });

  it("turning GPU rendering off and on again starts every pane over", () => {
    const pool = new WebglPool();
    let gpu = true;
    const r = { attached: false, attach: () => (r.attached = gpu), detach: () => void (r.attached = false) };
    pool.register("a", r);
    pool.update("a", { visible: true, focused: true });
    settle();
    expect(pool.holds("a")).toBe(true);
    gpu = false;
    pool.reset();
    settle();
    expect(pool.holds("a")).toBe(false);
    gpu = true;
    pool.reset();
    settle();
    expect(pool.holds("a")).toBe(true);
  });

  it("a remounted view keeps its entry when the old view unmounts late", () => {
    const pool = new WebglPool();
    const old = fake();
    pool.register("p", old);
    pool.update("p", { visible: true, focused: true });
    settle();
    const fresh = fake();
    pool.register("p", fresh);
    expect(old.attached).toBe(false);
    pool.update("p", { visible: true, focused: true });
    pool.unregister("p", old);
    settle();
    expect(pool.holds("p")).toBe(true);
    expect(fresh.attached).toBe(true);
  });
});
