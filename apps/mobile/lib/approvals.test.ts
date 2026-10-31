import { describe, expect, it } from "vitest";
import type { PhoneApproval } from "@tandem/protocol";
import { SETTLE_MS, STAMP_WINDOW_MS, allowTakesTap, answerSettles, approvalActions, pressedAt } from "./approvals";

const base: PhoneApproval = {
  id: "ap_1",
  summary: "Run `pnpm test`",
  detail: "pnpm test",
  source: "Thread",
  queued: false,
  deskOnly: false,
  createdAt: 1,
};

describe("F-TL-12.11 the phone's approval actions", () => {
  it("offers Allow and Deny for a thread's approval", () => {
    expect(approvalActions(base, true)).toEqual({ allow: true, deskOnly: false, deny: true });
  });

  it("offers no Allow for a pane's run, only Deny and Answer on your computer", () => {
    expect(approvalActions({ ...base, deskOnly: true }, true)).toEqual({
      allow: false,
      deskOnly: true,
      deny: true,
    });
  });

  it("offers nothing without the Approvals grant", () => {
    expect(approvalActions(base, false)).toMatchObject({ allow: false, deny: false });
  });
});

describe("F-TL-12.21 the phone's Allow waits until its card has held still", () => {
  const since = 10_000;
  it("takes no tap until the card has been still for 500 ms", () => {
    expect(SETTLE_MS).toBe(500);
    expect(allowTakesTap(since, since + 100, since + 200)).toBe(false);
    expect(allowTakesTap(since, since + SETTLE_MS, since + SETTLE_MS + 50)).toBe(true);
  });

  it("ignores a tap that began before the card held still, even when it ends after", () => {
    // Pressed on what was there, released once the new card had settled.
    expect(allowTakesTap(since, since + 100, since + 2_000)).toBe(false);
    expect(allowTakesTap(since, since - 1, since + 2_000)).toBe(false);
  });

  it("with no press seen, a settled card takes the tap", () => {
    expect(allowTakesTap(since, null, since + SETTLE_MS)).toBe(true);
    expect(allowTakesTap(since, null, since + SETTLE_MS - 1)).toBe(false);
  });
});

describe("F-TL-12.21 a tap's own time decides, not when the app got to it", () => {
  it("a tap begun before the card held still is refused even when its handler ran late", () => {
    const since = 10_000;
    // The touch began 100 ms after the move; the busy thread handled it at
    // +900 ms, which alone looks late enough.
    expect(allowTakesTap(since, pressedAt(since + 900, since + 100), since + 950)).toBe(false);
    expect(allowTakesTap(since, pressedAt(since + 900, since + 600), since + 950)).toBe(true);
  });

  it("a tap begun before the card held still is refused after a stall of more than a second too", () => {
    // Taken as begun when it was handled, a tap 200 ms after the move
    // passed once the thread had stalled 1.5 s (the review of b3ff3696,
    // S-2).
    const since = 10_000;
    expect(allowTakesTap(since, pressedAt(since + 1_500, since + 200), since + 1_501)).toBe(false);
    expect(allowTakesTap(since, pressedAt(since + 30_000, since + 200), since + 30_001)).toBe(false);
    expect(allowTakesTap(since, pressedAt(since + 1_500, since + 900), since + 1_501)).toBe(true);
  });

  it("a touch time on another clock refuses the tap, and never makes it later", () => {
    expect(pressedAt(5_000, 4_500)).toBe(4_500);
    expect(pressedAt(5_000, 1_000)).toBe(1_000);
    // Ahead of the handler, or further behind than any stall: another
    // clock, and the tap allows nothing.
    expect(pressedAt(5_000, 5_200)).toBe(-Infinity);
    expect(pressedAt(100_000, 100_000 - STAMP_WINDOW_MS - 1)).toBe(-Infinity);
    expect(allowTakesTap(0, pressedAt(100_000, 1_000), 100_001)).toBe(false);
    // A few ms ahead is the two reads' rounding.
    expect(pressedAt(5_000, 5_003)).toBe(5_000);
    // No stamp: when it was handled.
    expect(pressedAt(5_000, undefined)).toBe(5_000);
    expect(pressedAt(5_000, Number.NaN)).toBe(5_000);
  });
});

describe("F-TL-12.21 an answer the computer only queued doesn't settle the card", () => {
  it("a queued Allow can still be dropped there, so the card takes an answer again", () => {
    // Marked answered, a queued Allow the computer dropped left the card's
    // Allow and Deny doing nothing (the review of b3ff3696, S-3).
    expect(answerSettles({ ok: true, data: { state: "queued" } })).toBe(false);
    expect(answerSettles({ ok: true, data: {} })).toBe(true);
    expect(answerSettles({ ok: true, data: { state: "allowed" } })).toBe(true);
    expect(answerSettles({ ok: false })).toBe(false);
  });
});
