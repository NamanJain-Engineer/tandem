import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SETTLE_MS, useSeenEnd, useSettled } from "./ApprovalGuard";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

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

  it("useSeenEnd starts over on every change too", () => {
    const seen: Array<(entries: Array<{ isIntersecting: boolean }>) => void> = [];
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(cb: (entries: Array<{ isIntersecting: boolean }>) => void) {
          seen.push(cb);
        }
        observe() {}
        disconnect() {}
      },
    );
    const { result, rerender } = renderHook(({ k }: { k: string | null }) => useSeenEnd(k), {
      initialProps: { k: "A" } as { k: string | null },
    });
    act(() => result.current[0](document.createElement("span")));
    act(() => seen.at(-1)?.([{ isIntersecting: true }]));
    expect(result.current[1]).toBe(true);
    rerender({ k: "B" });
    rerender({ k: "A" });
    // Back at A, but its end hasn't been in view since.
    expect(result.current[1]).toBe(false);
    act(() => seen.at(-1)?.([{ isIntersecting: true }]));
    expect(result.current[1]).toBe(true);
    vi.unstubAllGlobals();
  });
});
