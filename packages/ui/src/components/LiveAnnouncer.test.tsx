import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ANNOUNCE_INTERVAL_MS, LiveAnnouncer, announce, resetAnnouncer } from "./LiveAnnouncer";

const region = () => screen.getByTestId("live-announcer");

describe("§5.4 live region announcements", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetAnnouncer();
  });
  afterEach(() => {
    resetAnnouncer();
    vi.useRealTimers();
  });

  it("§5.4 the region is polite and visually hidden", () => {
    render(<LiveAnnouncer />);
    expect(region().getAttribute("aria-live")).toBe("polite");
    expect(region().getAttribute("role")).toBe("status");
    expect(region().className).toContain("sr-only");
  });

  it("§5.4 announces at most once per 2 s, folding what arrives in between into the next one", () => {
    render(<LiveAnnouncer />);
    act(() => announce("Claude Code is waiting for you"));
    expect(region().textContent).toBe("Claude Code is waiting for you");

    act(() => {
      vi.advanceTimersByTime(500);
      announce("Codex finished");
      announce("Gemini CLI finished");
    });
    // Still the first message: the next one waits for the interval.
    expect(region().textContent).toBe("Claude Code is waiting for you");

    act(() => vi.advanceTimersByTime(ANNOUNCE_INTERVAL_MS - 500 - 1));
    expect(region().textContent).toBe("Claude Code is waiting for you");
    act(() => vi.advanceTimersByTime(1));
    expect(region().textContent).toBe("Codex finished. Gemini CLI finished");

    // Quiet for longer than the interval: the next one is immediate.
    act(() => vi.advanceTimersByTime(ANNOUNCE_INTERVAL_MS * 2));
    act(() => announce("Approval requested"));
    expect(region().textContent).toBe("Approval requested");
  });

  it("§5.4 a burst is capped: duplicates collapse and the rest is counted", () => {
    render(<LiveAnnouncer />);
    act(() => announce("A done"));
    act(() => {
      for (const m of ["B done", "B done", "C done", "D done", "E done", "F done"]) announce(m);
      vi.advanceTimersByTime(ANNOUNCE_INTERVAL_MS);
    });
    expect(region().textContent).toBe("B done. C done. D done. And 2 more");
  });
});
