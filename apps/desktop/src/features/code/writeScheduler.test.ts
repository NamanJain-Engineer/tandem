import { describe, expect, it } from "vitest";
import { COALESCE_MS, TICK_MS, WriteScheduler } from "./writeScheduler";

function setup(budget = 100) {
  let now = 0;
  const timers: Array<{ at: number; fn: () => void }> = [];
  const s = new WriteScheduler(
    {
      now: () => now,
      setTimer: (fn, ms) => {
        timers.push({ at: now + ms, fn });
        return timers.length;
      },
    },
    budget,
  );
  const advance = (ms: number) => {
    const until = now + ms;
    for (;;) {
      timers.sort((a, b) => a.at - b.at);
      const t = timers[0];
      if (!t || t.at > until) break;
      timers.shift();
      now = t.at;
      t.fn();
    }
    now = until;
  };
  /** A pane whose xterm parses each write when `parse()` is called. */
  const pane = (id: string) => {
    const got: number[] = [];
    const waiting: Array<() => void> = [];
    s.register(id, (data, parsed) => {
      got.push(data.byteLength);
      waiting.push(parsed);
    });
    return { got, parse: () => waiting.splice(0).forEach((f) => f()) };
  };
  return { s, advance, pane };
}

const bytes = (n: number) => new Uint8Array(n);

describe("write scheduler (F-CD-04)", () => {
  it("F-CD-04.1 background output reaches xterm within a per-tick budget, round-robin", () => {
    const { s, advance, pane } = setup(100);
    const a = pane("a");
    const b = pane("b");
    s.push("a", bytes(150));
    s.push("b", bytes(80));
    advance(TICK_MS);
    // 100 bytes this tick: all to `a` (first in turn).
    expect(a.got).toEqual([100]);
    expect(b.got).toEqual([]);
    a.parse();
    advance(TICK_MS);
    // `a` was visited < 33 ms ago; `b` gets the tick.
    expect(b.got).toEqual([80]);
    b.parse();
    advance(COALESCE_MS);
    expect(a.got).toEqual([100, 50]);
  });

  it("F-CD-04.1 nothing more is handed over until the last tick has been parsed", () => {
    const { s, advance, pane } = setup(100);
    const a = pane("a");
    s.push("a", bytes(1_000));
    advance(TICK_MS);
    expect(a.got).toEqual([100]);
    advance(TICK_MS * 10);
    expect(a.got).toEqual([100]);
    a.parse();
    advance(COALESCE_MS);
    expect(a.got).toEqual([100, 100]);
  });

  it("flushing a pane (focused, exited) writes all it has at once", () => {
    const { s, pane } = setup(100);
    const a = pane("a");
    s.push("a", bytes(40));
    s.push("a", bytes(500));
    s.flush("a");
    expect(a.got).toEqual([540]);
    expect(s.pending("a")).toBe(0);
  });

  it("chunks are split and merged byte-exactly", () => {
    const got: Uint8Array[] = [];
    let now = 0;
    const timers: Array<() => void> = [];
    const s = new WriteScheduler({ now: () => (now += 100), setTimer: (fn) => timers.push(fn) }, 3);
    s.register("a", (d, parsed) => {
      got.push(d.slice());
      parsed();
    });
    s.push("a", new Uint8Array([1, 2]));
    s.push("a", new Uint8Array([3, 4, 5]));
    while (timers.length) timers.shift()?.();
    expect(got.map((d) => [...d])).toEqual([
      [1, 2, 3],
      [4, 5],
    ]);
  });
});
