import { describe, expect, it } from "vitest";
import type { PhoneApproval } from "@tandem/protocol";
import { SETTLE_MS, allowTakesTap, approvalActions, pressedAt } from "./approvals";

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

  it("a touch time on another clock is ignored, and never makes a tap later", () => {
    expect(pressedAt(5_000, 4_500)).toBe(4_500);
    // Ahead of the handler, or more than a second behind it: another clock.
    expect(pressedAt(5_000, 5_200)).toBe(5_000);
    expect(pressedAt(5_000, 1_000)).toBe(5_000);
    expect(pressedAt(5_000, undefined)).toBe(5_000);
    expect(pressedAt(5_000, Number.NaN)).toBe(5_000);
  });
});
