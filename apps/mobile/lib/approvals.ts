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

/** What the Approvals tab keeps of its answers, so each is sent once
 * (F-TL-12 AC 21). `answered`: the cards an answer went out for, which take
 * no more taps. `queued`: of those, the Allows the computer only queued (it
 * wasn't armed), each with whether a snapshot has shown it queued yet. */
export type Answers = {
  readonly answered: ReadonlySet<string>;
  readonly queued: ReadonlyMap<string, boolean>;
};

export const NO_ANSWERS: Answers = { answered: new Set(), queued: new Map() };

/** Whether a tap on this card may send an answer. */
export function takesAnswer(s: Answers, id: string): boolean {
  return !s.answered.has(id);
}

/** After the computer's reply (`applied` or `queued`). Any reply that took
 * the answer marks the card answered: the card keeps its buttons until the
 * next snapshot, a round trip later, and a second tap meanwhile sent a
 * second Allow (the review of 8f35e5d0, S-3). A queued Allow is remembered
 * too, since the computer can still drop it. */
export function afterReply(s: Answers, id: string, r: { ok: boolean; data?: { state?: string } }): Answers {
  if (!r.ok) return s;
  const queued = new Map(s.queued);
  if (r.data?.state === "queued") queued.set(id, false);
  else queued.delete(id);
  return { answered: new Set(s.answered).add(id), queued };
}

/** After a snapshot. A queued Allow that one snapshot showed queued and a
 * later one shows not queued was dropped on the computer: its card takes an
 * answer again (the review of b3ff3696, S-3). Shown not queued before any
 * snapshot showed it queued, it may be a snapshot taken before the queue
 * had it, so that changes nothing. */
export function afterSnapshot(
  s: Answers,
  approvals: ReadonlyArray<Pick<PhoneApproval, "id" | "queued">>,
): Answers {
  if (s.queued.size === 0) return s;
  const listed = new Map(approvals.map((a) => [a.id, a.queued]));
  const answered = new Set(s.answered);
  const queued = new Map(s.queued);
  for (const [id, shown] of s.queued) {
    const isQueued = listed.get(id);
    if (isQueued === true) queued.set(id, true);
    else if (isQueued === false && shown) {
      queued.delete(id);
      answered.delete(id);
    }
  }
  return { answered, queued };
}
