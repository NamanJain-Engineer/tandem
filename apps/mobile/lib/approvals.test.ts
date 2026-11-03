import { describe, expect, it } from "vitest";
import type { PhoneApproval } from "@tandem/protocol";
import {
  NO_ANSWERS,
  SETTLE_MS,
  STAMP_WINDOW_MS,
  afterReply,
  afterSnapshot,
  allowTakesTap,
  approvalActions,
  pressedAt,
  takesAnswer,
} from "./approvals";

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

describe("F-TL-12.21 each answer from the phone is sent once, and a dropped queued Allow asks again", () => {
  // The desktop's replies to approval.decide (phone.rs): `applied` (Deny,
  // or Allow while armed) and `queued` (Allow while not armed).
  const applied = { ok: true, data: { state: "applied" } } as const;
  const queuedReply = { ok: true, data: { state: "queued" } } as const;
  const listed = (queued: boolean) => [
    { ...base, queued },
    { ...base, id: "ap_2" },
  ];

  it("a queued Allow takes no second tap until the computer drops it, and then takes one", () => {
    let s = NO_ANSWERS;
    expect(takesAnswer(s, "ap_1")).toBe(true);
    // 1. The computer queues the Allow. The card keeps its buttons until
    // the next snapshot, a round trip away.
    s = afterReply(s, "ap_1", queuedReply);
    // 2. A second tap in that window sent a second Allow, a second row in
    // the computer's queue (the review of 8f35e5d0, S-3): it's ignored.
    expect(takesAnswer(s, "ap_1")).toBe(false);
    // 3. Shown queued, then not queued: the computer dropped it.
    s = afterSnapshot(s, listed(true));
    expect(takesAnswer(s, "ap_1")).toBe(false);
    s = afterSnapshot(s, listed(false));
    // 4. The card asks again, and a tap is taken (the review of b3ff3696,
    // S-3). Another card was never touched.
    expect(takesAnswer(s, "ap_1")).toBe(true);
    expect(takesAnswer(s, "ap_2")).toBe(true);
    s = afterReply(s, "ap_1", queuedReply);
    expect(takesAnswer(s, "ap_1")).toBe(false);
  });

  it("a snapshot taken before the queue had the Allow doesn't give the card back", () => {
    let s = afterReply(NO_ANSWERS, "ap_1", queuedReply);
    // An earlier refresh lands after the reply: not queued yet.
    s = afterSnapshot(s, listed(false));
    expect(takesAnswer(s, "ap_1")).toBe(false);
    s = afterSnapshot(s, listed(true));
    s = afterSnapshot(s, listed(true));
    expect(takesAnswer(s, "ap_1")).toBe(false);
    // Confirmed on the computer: the card leaves the list, still answered.
    s = afterSnapshot(s, []);
    expect(takesAnswer(s, "ap_1")).toBe(false);
  });

  it("an applied answer stays answered whatever the list shows, and a refused one sends nothing", () => {
    let s = afterReply(NO_ANSWERS, "ap_1", applied);
    expect(takesAnswer(s, "ap_1")).toBe(false);
    s = afterSnapshot(s, listed(true));
    s = afterSnapshot(s, listed(false));
    expect(takesAnswer(s, "ap_1")).toBe(false);
    // Refused (not found, desk only, failed): nothing went, so the card
    // still takes an answer.
    const refused = afterReply(NO_ANSWERS, "ap_2", { ok: false });
    expect(takesAnswer(refused, "ap_2")).toBe(true);
  });
});
