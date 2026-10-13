import { describe, expect, it } from "vitest";
import { HOLD_MS, MountQueue } from "./mountQueue";

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
});
