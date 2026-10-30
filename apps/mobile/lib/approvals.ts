// F-RM-05 / F-TL-12: what the Approvals tab offers for one approval. Kept
// out of the screen so it runs in tests without React Native.
import type { PhoneApproval } from "@tandem/protocol";

export type ApprovalActions = {
  /** Allow is offered (a pane's `vibezeo run` is answered on the computer). */
  allow: boolean;
  /** "Answer on your computer" shows in Allow's place. */
  deskOnly: boolean;
  deny: boolean;
};

export function approvalActions(a: PhoneApproval, granted: boolean): ApprovalActions {
  return { allow: granted && !a.deskOnly, deskOnly: a.deskOnly, deny: granted };
}

/** How long the phone's Allow ignores taps after its card arrives, changes
 * or moves (F-TL-12 AC 21), as the computer's Allow does. */
export const SETTLE_MS = 500;

/** Whether Allow takes the tap now ending. `since` is when the card's
 * request, its place in the list or the list's scroll last changed, and
 * `pressedAt` when the tap began (null: none seen). A tap begun before the
 * card had held still was aimed at what was there before: it allows
 * nothing. */
export function allowTakesTap(since: number, pressedAt: number | null, now: number): boolean {
  const still = since + SETTLE_MS;
  return now >= still && (pressedAt === null || pressedAt >= still);
}

/** One clock for every time the card keeps: monotonic where the runtime
 * has one, so a wall clock set back can't hold Allow off, or set forward
 * pass for a wait (the review of ce0d5815). */
export function now(): number {
  return typeof performance !== "undefined" && typeof performance.now === "function" ? performance.now() : Date.now();
}

/** When a tap began: the touch's own time when it reads on the same clock
 * (at most 1 s before the handler runs), else when the handler runs. A
 * thread busy with the very refresh that moved the card ran the handler
 * late, and a tap begun too early looked late enough (the review of
 * ce0d5815, SF-2). Only ever the earlier of the two, so a touch time on
 * another clock can't make a tap look later than it was handled. */
export function pressedAt(handled: number, stamp: number | undefined): number {
  if (stamp === undefined || !Number.isFinite(stamp)) return handled;
  return stamp <= handled && stamp >= handled - 1_000 ? stamp : handled;
}
