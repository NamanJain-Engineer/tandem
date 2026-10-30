import { describe, expect, it } from "vitest";
import type { PhoneApproval } from "@tandem/protocol";
import { SETTLE_MS, allowTakesTap, approvalActions } from "./approvals";

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
