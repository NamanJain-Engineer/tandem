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
