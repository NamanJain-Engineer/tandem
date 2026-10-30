import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SETTLE_MS, useSeenEnd, useSettled, waitReason } from "./ApprovalGuard";

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
    expect(result.current[1]).toBe(true);
    rerender({ k: "B" });
    rerender({ k: "A" });
    // Back at A, but its end hasn't been in view since.
    expect(result.current[1]).toBe(false);
    act(() => seen.at(-1)?.([{ isIntersecting: true, intersectionRatio: 1 }]));
    expect(result.current[1]).toBe(true);
  });

  it("F-TL-12.15 useSeenEnd counts the end seen only when all of its marker was on screen", () => {
    const seen = observed();
    const { result } = renderHook(() => useSeenEnd("A"));
    act(() => result.current[0](document.createElement("span")));
    // A sliver of the line's last line, or most of it, isn't its end seen.
    act(() => seen.at(-1)?.([{ isIntersecting: true, intersectionRatio: 0.1 }]));
    act(() => seen.at(-1)?.([{ isIntersecting: true, intersectionRatio: 0.8 }]));
    expect(result.current[1]).toBe(false);
    act(() => seen.at(-1)?.([{ isIntersecting: true, intersectionRatio: 1 }]));
    expect(result.current[1]).toBe(true);
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
