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

/** How far before its handler a touch's own time still reads as on the
 * handler's clock: far less than any two clocks differ (boot, app-start
 * and wall clocks are hours to decades apart), far more than a busy thread
 * delays a handler. */
export const STAMP_WINDOW_MS = 60_000;

/** When a tap began: the touch's own time (React Native stamps touches on
 * `performance.now()`'s clock). A thread busy with the very refresh that
 * moved the card ran the handler late, and a tap begun too early looked
 * late enough (the review of ce0d5815, SF-2). A stamp outside the window,
 * or ahead of the handler, is on another clock: the tap is refused, never
 * taken as begun when it was handled, which a thread stalled over a
 * second let through (the review of b3ff3696, S-2). With no stamp at all,
 * when it was handled. */
export function pressedAt(handled: number, stamp: number | undefined): number {
  if (stamp === undefined || !Number.isFinite(stamp)) return handled;
  // A few ms ahead is rounding between the two reads, not another clock.
  if (stamp > handled) return stamp - handled <= 5 ? handled : -Infinity;
  return stamp >= handled - STAMP_WINDOW_MS ? stamp : -Infinity;
}

/** Whether the computer's answer settles the card: an Allow it only
 * queued (not armed for it) can still be dropped there, and the card then
 * asks again; marked answered, it took no answer until the tab came back
 * (the review of b3ff3696, S-3). While queued, the card offers no
 * buttons, so nothing goes twice. */
export function answerSettles(r: { ok: boolean; data?: { state?: string } }): boolean {
  return r.ok && r.data?.state !== "queued";
}
