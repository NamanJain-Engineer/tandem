import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DWELL_MS, SETTLE_MS, useSeenEnd, useSettled, waitReason } from "./ApprovalGuard";

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("useSettled (F-TL-12.12)", () => {
  it("starts over on every change, even back to a key it settled on before", () => {
    const { result, rerender } = renderHook(({ k }: { k: string | null }) => useSettled(k), {
      initialProps: { k: "A" } as { k: string | null },
    });
    expect(result.current).toBe(false);
    act(() => vi.advanceTimersByTime(SETTLE_MS));
    expect(result.current).toBe(true);
    // A → B → A within the settle time: not settled at A's return.
    rerender({ k: "B" });
    expect(result.current).toBe(false);
    act(() => vi.advanceTimersByTime(SETTLE_MS - 100));
    rerender({ k: "A" });
    expect(result.current).toBe(false);
    act(() => vi.advanceTimersByTime(SETTLE_MS - 1));
    expect(result.current).toBe(false);
    act(() => vi.advanceTimersByTime(1));
    expect(result.current).toBe(true);
    rerender({ k: null });
    expect(result.current).toBe(false);
  });

  type Entry = { isIntersecting: boolean; intersectionRatio: number };
  function observed(): Array<(entries: Entry[]) => void> {
    const seen: Array<(entries: Entry[]) => void> = [];
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(cb: (entries: Entry[]) => void) {
          seen.push(cb);
        }
        observe() {}
        disconnect() {}
      },
    );
    return seen;
  }

  it("useSeenEnd starts over on every change too", () => {
    const seen = observed();
    const { result, rerender } = renderHook(({ k }: { k: string | null }) => useSeenEnd(k), {
      initialProps: { k: "A" } as { k: string | null },
    });
    act(() => result.current[0](document.createElement("span")));
    act(() => seen.at(-1)?.([{ isIntersecting: true, intersectionRatio: 1 }]));
    act(() => vi.advanceTimersByTime(DWELL_MS));
    expect(result.current[1]).toBe(true);
    rerender({ k: "B" });
    rerender({ k: "A" });
    // Back at A, but its end hasn't been in view since.
    expect(result.current[1]).toBe(false);
    act(() => seen.at(-1)?.([{ isIntersecting: true, intersectionRatio: 1 }]));
    act(() => vi.advanceTimersByTime(DWELL_MS));
    expect(result.current[1]).toBe(true);
  });

  it("F-TL-12.15 useSeenEnd counts the end seen only when all of its marker was on screen", () => {
    const seen = observed();
    const { result } = renderHook(() => useSeenEnd("A"));
    act(() => result.current[0](document.createElement("span")));
    // A sliver of the line's last line, or most of it, isn't its end seen,
    // however long it shows.
    act(() => seen.at(-1)?.([{ isIntersecting: true, intersectionRatio: 0.1 }]));
    act(() => seen.at(-1)?.([{ isIntersecting: true, intersectionRatio: 0.8 }]));
    act(() => vi.advanceTimersByTime(DWELL_MS * 3));
    expect(result.current[1]).toBe(false);
    act(() => seen.at(-1)?.([{ isIntersecting: true, intersectionRatio: 1 }]));
    act(() => vi.advanceTimersByTime(DWELL_MS));
    expect(result.current[1]).toBe(true);
  });

  it("F-TL-12.20 useSeenEnd counts the end seen only once all of its marker has stayed on screen for the dwell", () => {
    const seen = observed();
    const { result } = renderHook(() => useSeenEnd("A"));
    act(() => result.current[0](document.createElement("span")));
    const report = (...ratios: number[]) =>
      act(() => seen.at(-1)?.(ratios.map((r) => ({ isIntersecting: r > 0, intersectionRatio: r }))));
    expect(DWELL_MS).toBe(400);
    // An animated End shows the line for a frame on its way past: that
    // isn't its end seen (the review of 8f35e5d0, S-1).
    report(1);
    act(() => vi.advanceTimersByTime(50));
    report(0.5);
    act(() => vi.advanceTimersByTime(DWELL_MS * 3));
    expect(result.current[1]).toBe(false);
    // Two crossings told at once: the last is where the line is now.
    report(1, 0);
    act(() => vi.advanceTimersByTime(DWELL_MS * 3));
    expect(result.current[1]).toBe(false);
    // Held in view: seen once the dwell is over, not before. A rounding
    // that keeps it whole doesn't start the dwell again.
    report(1);
    act(() => vi.advanceTimersByTime(DWELL_MS - 100));
    report(0.995);
    act(() => vi.advanceTimersByTime(99));
    expect(result.current[1]).toBe(false);
    act(() => vi.advanceTimersByTime(1));
    expect(result.current[1]).toBe(true);
  });

  it("F-TL-12.20 a request that changes mid-dwell starts its own, and leaves no timer behind", () => {
    const seen = observed();
    const { result, rerender, unmount } = renderHook(({ k }: { k: string | null }) => useSeenEnd(k), {
      initialProps: { k: "A" } as { k: string | null },
    });
    act(() => result.current[0](document.createElement("span")));
    const whole = () => act(() => seen.at(-1)?.([{ isIntersecting: true, intersectionRatio: 1 }]));
    whole();
    act(() => vi.advanceTimersByTime(DWELL_MS - 100));
    // B replaces A with A's end 300 ms into its dwell; B's own end is in
    // view at once. A's dwell is gone, and B's starts now.
    rerender({ k: "B" });
    expect(vi.getTimerCount()).toBe(0);
    whole();
    act(() => vi.advanceTimersByTime(100));
    expect(result.current[1]).toBe(false);
    act(() => vi.advanceTimersByTime(DWELL_MS - 101));
    expect(result.current[1]).toBe(false);
    act(() => vi.advanceTimersByTime(1));
    expect(result.current[1]).toBe(true);
    // A dwell under way when the card goes is stopped with it.
    rerender({ k: "C" });
    whole();
    expect(vi.getTimerCount()).toBe(1);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("waitReason names the first reason Allow waits", () => {
    expect(waitReason({ seenEnd: false, settled: false, steady: false })).toBe(
      "Scroll to the end of the command first.",
    );
    expect(waitReason({ seenEnd: true, settled: false, steady: false })).toBe(
      "This question just changed: Allow turns on in a moment.",
    );
    expect(waitReason({ seenEnd: true, settled: true, steady: false })).toBe(
      "Allow turns on once this has been still on screen for a moment.",
    );
    expect(waitReason({ seenEnd: true, settled: true, steady: true })).toBeUndefined();
    // Gone past the line's end without showing it: back up (the review of
    // ce0d5815, SF-1).
    expect(waitReason({ seenEnd: false, settled: true, steady: true, passed: true })).toBe(
      "Scroll up a little to see the end of the command.",
    );
  });
});
