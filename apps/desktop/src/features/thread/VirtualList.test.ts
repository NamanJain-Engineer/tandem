import { describe, expect, it } from "vitest";
import { nextRange, UNSET } from "./VirtualList";

// 100 rows, 100 px each; the overscan is 800 px.
const offsets = Array.from({ length: 101 }, (_, i) => i * 100);

describe("VirtualList ranges (UI-21)", () => {
  it("UI-21 an unset range covers the viewport by 1.5 overscans", () => {
    expect(nextRange(UNSET, offsets, 5_000, 1_000)).toEqual({ start: 38, end: 73 });
  });

  it("UI-21 scrolling inside the rendered rows keeps the range (no render)", () => {
    const r = nextRange(UNSET, offsets, 5_000, 1_000);
    expect(nextRange(r, offsets, 5_500, 1_000)).toBe(r);
    expect(nextRange(r, offsets, 4_300, 1_000)).toBe(r);
  });

  it("UI-21 nearing an edge replaces the range a screenful at a time", () => {
    const r = nextRange(UNSET, offsets, 5_000, 1_000);
    expect(nextRange(r, offsets, 6_200, 1_000)).toEqual({ start: 50, end: 85 });
    expect(nextRange(r, offsets, 3_000, 1_000)).toEqual({ start: 18, end: 53 });
  });

  it("UI-21 the range shrinks with the list and stops at both ends", () => {
    const fewer = offsets.slice(0, 61);
    expect(nextRange({ start: 50, end: 85 }, fewer, 5_000, 1_000)).toEqual({ start: 38, end: 60 });
    expect(nextRange(UNSET, offsets, 0, 1_000)).toEqual({ start: 0, end: 23 });
    expect(nextRange(UNSET, offsets, 9_000, 1_000)).toEqual({ start: 78, end: 100 });
  });
});
