import { describe, expect, it } from "vitest";
import { HOLD_MS, MountQueue, SHOWN_HOLD_MS } from "./mountQueue";

function setup() {
  const frames: Array<() => void> = [];
  const timers: Array<{ ms: number; fn: () => void }> = [];
  const q = new MountQueue(
    (fn) => frames.push(fn),
    (ms, fn) => timers.push({ ms, fn }),
  );
  const opened: string[] = [];
  const closed: string[] = [];
  const job = (id: string) => () => {
    opened.push(id);
    return () => closed.push(id);
  };
  const frame = () => frames.shift()?.();
  return { q, opened, closed, job, frame, timers };
}

describe("mount queue (F-CD-04)", () => {
  it("F-CD-04 six new views open one per frame, the focused one first", () => {
    const { q, opened, job, frame } = setup();
    for (const id of ["a", "b", "c", "d", "e"]) q.add(job(id));
    q.add(job("f"), true);
    // The first opens at once; the rest wait for frames.
    expect(opened).toEqual(["a"]);
    frame();
    expect(opened).toEqual(["a", "f"]);
    for (let i = 0; i < 4; i++) frame();
    expect(opened).toEqual(["a", "f", "b", "c", "d", "e"]);
    frame();
    expect(q.pending()).toBe(0);
  });

  it("F-CD-04 views on screen open before queued views of a tab that was hidden", () => {
    const { q, opened, job, frame } = setup();
    // A tab of shells is still opening when another tab is shown.
    const shown = new Set(["s1", "s2", "s3", "s4"]);
    const on = (id: string) => () => shown.has(id);
    for (const id of ["s1", "s2", "s3", "s4"]) q.add(job(id), false, on(id));
    expect(opened).toEqual(["s1"]);
    for (const id of ["s1", "s2", "s3", "s4"]) shown.delete(id);
    for (const id of ["a1", "a2", "a3"]) shown.add(id);
    q.add(job("a1"), true, on("a1"));
    q.add(job("a2"), false, on("a2"));
    q.add(job("a3"), false, on("a3"));
    for (let i = 0; i < 3; i++) frame();
    expect(opened).toEqual(["s1", "a1", "a2", "a3"]);
    // The hidden tab's views still open after, in order.
    frame();
    frame();
    frame();
    expect(opened).toEqual(["s1", "a1", "a2", "a3", "s2", "s3", "s4"]);
  });

  it("F-CD-04 a queued view shown before its turn opens with the views on screen", () => {
    const { q, opened, job, frame } = setup();
    const shown = new Set(["a"]);
    const on = (id: string) => () => shown.has(id);
    for (const id of ["a", "b", "c", "d"]) q.add(job(id), false, on(id));
    shown.add("d");
    frame();
    expect(opened).toEqual(["a", "d"]);
  });

  it("a view unmounted before it opened never opens; an opened one is cleaned up", () => {
    const { q, opened, closed, job, frame } = setup();
    const a = q.add(job("a"));
    const b = q.add(job("b"));
    b();
    frame();
    expect(opened).toEqual(["a"]);
    a();
    expect(closed).toEqual(["a"]);
    // Idle again: the next view opens at once.
    frame();
    q.add(job("c"));
    expect(opened).toEqual(["a", "c"]);
  });

  it("F-SH-07.7 the focused view holds the queue until it has connected", async () => {
    const { q, opened, job, frame } = setup();
    let connected!: () => void;
    q.add(() => {
      opened.push("focused");
      q.hold(new Promise<void>((r) => (connected = r)));
      return () => undefined;
    }, true);
    for (const id of ["b", "c"]) q.add(job(id));
    frame();
    await Promise.resolve();
    // Its attach and spawn replies don't wait behind the next view's open.
    expect(opened).toEqual(["focused"]);
    connected();
    await Promise.resolve();
    await Promise.resolve();
    expect(opened).toEqual(["focused", "b"]);
    frame();
    expect(opened).toEqual(["focused", "b", "c"]);
  });

  it("F-SH-07.7 a hold lasts at most HOLD_MS", () => {
    const { q, opened, job, frame, timers } = setup();
    q.add(() => {
      opened.push("focused");
      q.hold(new Promise<void>(() => undefined));
      return () => undefined;
    }, true);
    q.add(job("b"));
    frame();
    expect(opened).toEqual(["focused"]);
    expect(timers.map((t) => t.ms)).toEqual([HOLD_MS]);
    timers[0]?.fn();
    expect(opened).toEqual(["focused", "b"]);
  });

  it("F-CD-03.2 views not on screen wait while one on screen hasn't shown its first output", async () => {
    const { q, opened, job, frame } = setup();
    const shown = new Set(["a"]);
    const on = (id: string) => () => shown.has(id);
    let first!: () => void;
    q.add(
      () => {
        opened.push("a");
        q.holdHidden(new Promise<void>((r) => (first = r)));
        return () => undefined;
      },
      false,
      on("a"),
    );
    q.add(job("h1"), false, on("h1"));
    q.add(job("h2"), false, on("h2"));
    frame();
    await Promise.resolve();
    // Their opens would come between the view and its first output.
    expect(opened).toEqual(["a"]);
    first();
    await Promise.resolve();
    await Promise.resolve();
    expect(opened).toEqual(["a", "h1"]);
    frame();
    expect(opened).toEqual(["a", "h1", "h2"]);
  });

  it("F-CD-03.2 a view on screen still opens while hidden ones wait", async () => {
    const { q, opened, job, frame } = setup();
    const shown = new Set(["a", "b"]);
    const on = (id: string) => () => shown.has(id);
    q.add(
      () => {
        opened.push("a");
        q.holdHidden(new Promise<void>(() => undefined));
        return () => undefined;
      },
      false,
      on("a"),
    );
    q.add(job("h"), false, on("h"));
    q.add(job("b"), false, on("b"));
    frame();
    expect(opened).toEqual(["a", "b"]);
    frame();
    expect(opened).toEqual(["a", "b"]);
    // A view added on screen meanwhile opens at once.
    shown.add("c");
    q.add(job("c"), false, on("c"));
    expect(opened).toEqual(["a", "b", "c"]);
  });

  it("F-CD-03.2 the wait for a first output lasts at most SHOWN_HOLD_MS", () => {
    const { q, opened, job, frame, timers } = setup();
    q.add(
      () => {
        opened.push("a");
        q.holdHidden(new Promise<void>(() => undefined));
        return () => undefined;
      },
      false,
      () => true,
    );
    q.add(job("h"), false, () => false);
    frame();
    expect(opened).toEqual(["a"]);
    expect(timers.map((t) => t.ms)).toEqual([SHOWN_HOLD_MS]);
    timers[0]?.fn();
    expect(opened).toEqual(["a", "h"]);
  });

  it("F-CD-03.2 a view on screen would wait only behind an open or other views on screen", async () => {
    const { q, frame } = setup();
    expect(q.wouldWait(true)).toBe(false);
    expect(q.wouldWait(false)).toBe(false);
    let first!: () => void;
    q.add(
      () => {
        q.holdHidden(new Promise<void>((r) => (first = r)));
        return () => undefined;
      },
      false,
      () => true,
    );
    // Its open takes this frame.
    expect(q.wouldWait(true)).toBe(true);
    frame();
    expect(q.wouldWait(true)).toBe(false);
    // A hidden one waits for that first output.
    expect(q.wouldWait(false)).toBe(true);
    first();
    await Promise.resolve();
    expect(q.wouldWait(false)).toBe(false);
  });
});
