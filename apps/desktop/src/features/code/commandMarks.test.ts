import { describe, expect, it } from "vitest";
import { CommandMarks, IDLE_AFTER_MS, type Activity } from "./commandMarks";

function setup() {
  let clock = 0;
  let line = 0;
  const timers: Array<{ at: number; fn: () => void; live: boolean }> = [];
  const decorated: Array<[number, boolean]> = [];
  const released: number[] = [];
  const activity: Activity[] = [];
  const marks = new CommandMarks<number>({
    marker: () => line,
    release: (m) => released.push(m),
    decorate: (m, ok) => decorated.push([m, ok]),
    activity: (s) => activity.push(s),
    now: () => clock,
    setTimer: (fn, ms) => {
      const t = { at: clock + ms, fn, live: true };
      timers.push(t);
      return t;
    },
    clearTimer: (t) => {
      if (t) (t as { live: boolean }).live = false;
    },
  });
  const advance = (ms: number) => {
    const until = clock + ms;
    for (;;) {
      const next = timers.filter((t) => t.live && t.at <= until).sort((a, b) => a.at - b.at)[0];
      if (!next) break;
      clock = next.at;
      next.live = false;
      next.fn();
    }
    clock = until;
  };
  return {
    marks,
    decorated,
    released,
    activity,
    advance,
    at: (l: number) => (line = l),
  };
}

describe("command marks (F-CD-10)", () => {
  it("F-CD-10 a command gets ✓ or ✗ beside its prompt line", () => {
    const s = setup();
    s.at(3);
    s.marks.mark("A");
    s.marks.mark("B");
    s.marks.mark("C");
    s.at(5);
    s.marks.mark("D;0");
    s.marks.mark("A");
    s.marks.mark("C");
    s.marks.mark("D;2");
    expect(s.decorated).toEqual([
      [3, true],
      [5, false],
    ]);
    // A D without a C, or without an exit code, marks nothing.
    s.marks.mark("D;1");
    s.marks.mark("A");
    s.marks.mark("C");
    s.marks.mark("D");
    expect(s.decorated).toHaveLength(2);
  });

  it("prompts that ran nothing release their marker", () => {
    const s = setup();
    s.at(1);
    s.marks.mark("A");
    s.at(2);
    s.marks.mark("A");
    expect(s.released).toEqual([1]);
    s.marks.mark("C");
    s.marks.mark("D;0");
    s.at(3);
    s.marks.mark("A");
    expect(s.released).toEqual([1]);
  });

  it("F-CD-10 a plain shell is working between C and D, idle 2 s after the last output", () => {
    const s = setup();
    s.marks.mark("A");
    s.marks.mark("C");
    expect(s.activity).toEqual(["working"]);
    s.marks.output();
    s.marks.mark("D;0");
    s.advance(1_500);
    s.marks.output(); // the next prompt printing
    s.advance(IDLE_AFTER_MS - 1);
    expect(s.activity).toEqual(["working"]);
    s.advance(1);
    expect(s.activity).toEqual(["working", "idle"]);
    // A new command cancels a pending idle.
    s.marks.mark("C");
    s.marks.mark("D;0");
    s.marks.mark("C");
    s.advance(IDLE_AFTER_MS * 2);
    expect(s.activity).toEqual(["working", "idle", "working", "working"]);
  });

  it("NFR-5.1 F-CD-10 parking reads the open prompt and the running command's prompt", () => {
    const s = setup();
    expect(s.marks.state()).toEqual({ prompt: undefined, running: undefined });
    s.at(3);
    s.marks.mark("A");
    expect(s.marks.state()).toEqual({ prompt: 3, running: undefined });
    s.marks.mark("C");
    expect(s.marks.state()).toEqual({ prompt: undefined, running: 3 });
    s.at(9);
    s.marks.mark("D;0");
    expect(s.marks.state()).toEqual({ prompt: undefined, running: undefined });
    s.marks.mark("A");
    expect(s.marks.state()).toEqual({ prompt: 9, running: undefined });
  });
});
