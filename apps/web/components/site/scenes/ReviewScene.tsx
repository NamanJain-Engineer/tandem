import type { ReactNode } from "react";
import { SAMPLE, t } from "../../../lib/scenes";
import { AgentsSegment, AppWindow, DiffRows, ReviewCard, ReviewQueue, StatusBar, Toast, fx, stint, type Agents, type DiffRow } from "./parts";

/** A second opinion's line comment, in the story's words. */
type Comment = { file: string; line: number; text: string };

/** The file the scene shows: the one the second opinion comments on, or,
 * with no comments, the first one the agent edited. */
export function reviewFile(comments: ReadonlyArray<{ file: string }>, edited: readonly string[]): string {
  return comments[0]?.file ?? edited[0] ?? "";
}
export const REVIEW_FILE = reviewFile(SAMPLE.comments, SAMPLE.edited);

/** The diff the scene reviews: the finished worktree's change to
 * REVIEW_FILE, the checkout function's hunk with its old and new line
 * numbers, as DiffView shows them. It's a run of git's own diff of the demo's
 * sample files (the base branch's and the worktree's; a test checks). */
export const REVIEW_DIFF: readonly DiffRow[] = [
  [" ", "export async function checkout(cart: Cart, req: Request) {", 8, 19],
  ["−", "  const session = await getSession(req);", 9, null],
  ["−", "  if (!session) return redirect(\"/login\");", 10, null],
  ["+", "  const session = await refreshSession(req);", null, 20],
  ["+", "  if (!session) {", null, 21],
  ["+", "    return redirect(`/login?next=${encodeURIComponent(req.url)}`);", null, 22],
  ["+", "  }", null, 23],
  [" ", "  const total = applyCoupon(cart, session.coupon);", 11, 24],
  ["+", "  if (cart.expiresAt < Date.now()) return expired(cart);", null, 25],
  [" ", "  return charge(session.customer, total);", 12, 26],
  [" ", "}", 13, 27],
];

/** Which rows of `rows` (a hunk of `file`) the comments sit under, each
 * row's in order. A comment on another file, or on a line the hunk doesn't
 * show, fails the build rather than be drawn somewhere else. */
export function notesFor(
  file: string,
  rows: ReadonlyArray<readonly [string, string, number | null, number | null]>,
  comments: ReadonlyArray<{ file: string; line: number; text: string }>,
): Map<number, string[]> {
  const out = new Map<number, string[]>();
  for (const c of comments) {
    const row = rows.findIndex(([, , , newNo]) => newNo === c.line);
    if (c.file !== file || row < 0) throw new Error(`scenes: the Review scene's diff of ${file} has nowhere for a comment on ${c.file} line ${c.line}`);
    out.set(row, [...(out.get(row) ?? []), c.text]);
  }
  return out;
}

/** When Second opinion is pressed, when the reviewer starts, and when it's
 * done: its comments arrive together then, as `review:second_opinion` adds
 * them in the app (D-3000), and its toast says so. Send waits for the toast
 * to go, as it covers the bar. `finished` is how many minutes ago the work
 * finished, which the card's meta ends with. */
export const REVIEW_T = { ask: 1.9, reviewing: 2.1, done: 3.8, send: 5.4, finished: 4 } as const;
const { done: DONE } = REVIEW_T;

/** The agents waiting and at work from each time on, as the status bar
 * counts them: the story's (one waits, one works; the finished worktree's
 * agent is done), and the second opinion, a thread ("Second opinion:
 * {branch}"), whose status the app counts as it does a pane's, while it
 * reviews (D-3003, D-3004). */
export const REVIEW_AGENTS: ReadonlyArray<readonly [number, Agents]> = [
  [0, SAMPLE.agents],
  [REVIEW_T.reviewing, { ...SAMPLE.agents, working: SAMPLE.agents.working + 1 }],
  [DONE, SAMPLE.agents],
];

/** Where the comments land: under the rows at their lines, each as
 * DiffView's annotation row, two on one line both drawn. They're the
 * review's own, to edit or remove, and only they fade in. */
function notes(comments: readonly Comment[]): Map<number, ReactNode> {
  return new Map(
    [...notesFor(REVIEW_FILE, REVIEW_DIFF, comments)].map(([row, texts]) => [
      row,
      texts.map((text, k) => (
        <span key={k} className="sc-diff-note" {...fx("in", DONE)}>
          <span className="sc-diff-note-text">{text}</span>
          <span className="sc-btn sc-btn-ghost">{t("worktree.review.edit")}</span>
          <span className="sc-btn sc-btn-ghost">{t("worktree.review.remove")}</span>
        </span>
      )),
    ]),
  );
}

/** "Review and a second opinion" (D-1880): the review queue (F-DB-04) with
 * the finished worktree's fix, the one piece of finished work, sorted by
 * risk; Second opinion asks another engine, which reads the diff without
 * editing anything (F-WT-05) while the review bar says so; its findings land
 * together as line comments under their lines, with a toast, and the bar
 * sends them to the agent. About 6 s, played once. The comments are the
 * story's; a test draws it with none, too. */
export function ReviewScene({ comments = SAMPLE.comments }: { comments?: readonly Comment[] }) {
  const done = SAMPLE.worktrees.finished;
  const reviewer = SAMPLE.reviewer;
  const n = comments.length;
  return (
    <AppWindow className="sc-review">
      <div className="sc-rev">
        <div className="sc-rev-list">
          {/* The queue as ReviewQueue draws it, the card as its ReviewCard:
              Second opinion is pressed on it. */}
          <ReviewQueue count={1}>
            <span {...fx("slide", 0.2)}>
              <ReviewCard
                lane={done}
                minutes={REVIEW_T.finished}
                closing={done.closing}
                diff={SAMPLE.diff}
                risks={[t("review.rule.auth")]}
                count={0.5}
                opinion={REVIEW_T.ask}
              />
            </span>
          </ReviewQueue>
        </div>

        <div className="sc-diff" {...fx("in", 0.4)}>
          <div className="sc-diff-head">
            <span className="sc-branch">{REVIEW_FILE}</span>
          </div>
          {/* The comments go in under their lines in one go: the rows below
              move down at once, and only the comments fade in (DiffRows). */}
          <DiffRows rows={REVIEW_DIFF} notes={notes(comments)} at={DONE} />
          {/* WorktreeReview's bar: the reviewer at work, with Open thread,
              then the comments' count with Copy review and Send to the
              worktree's one agent, by its name. The app re-renders it, so
              the reviewer's state is gone at once when the comments come,
              and with none the bar goes at once when the review's done. */}
          <span className="sc-review-bar" {...(n > 0 ? fx("in", REVIEW_T.reviewing) : stint(REVIEW_T.reviewing, DONE))}>
            <span className="sc-bar-states">
              <span className="sc-bar-state" {...(n > 0 ? stint(REVIEW_T.reviewing, DONE) : {})}>
                <span className="sc-spin" />
                <span className="sc-review-doing">{t("worktree.opinion.reviewing", { engine: reviewer })}</span>
                <span className="sc-btn sc-btn-sm sc-btn-ghost">{t("worktree.opinion.openThread")}</span>
              </span>
              {n > 0 && (
                <span className="sc-bar-state" {...fx("in", DONE)}>
                  <span className="sc-review-count">{t("worktree.review.count", { n })}</span>
                  <span className="sc-btn sc-btn-sm">{t("worktree.review.copy")}</span>
                  <span className="sc-btn sc-btn-sm sc-btn-primary" {...fx("enter", REVIEW_T.send)}>
                    {t("worktree.review.sendTo", { name: done.name })}
                  </span>
                </span>
              )}
            </span>
          </span>
        </div>
        {/* The reviewer's done toast (SecondOpinion.tsx), a success with
            Show diff, or its no-comments one, which has the default tone. */}
        {n > 0 ? (
          <Toast tone="success" title={t("worktree.opinion.done", { engine: reviewer, n })} action={t("worktree.opinion.showDiff")} at={DONE} stay={REVIEW_T.send - 0.1 - DONE} />
        ) : (
          <Toast title={t("worktree.opinion.none", { engine: reviewer })} at={DONE} />
        )}
      </div>
      <StatusBar branch={done.branch} segments={<AgentsSegment steps={REVIEW_AGENTS} />} />
    </AppWindow>
  );
}

export const REVIEW_CAPTION = "Finished work waits in one queue. Another engine reads it and leaves line comments.";

export function reviewSceneText(comments: readonly Comment[] = SAMPLE.comments): string {
  const done = SAMPLE.worktrees.finished;
  const n = comments.length;
  const found = n === 0 ? "has no comments" : `leaves ${n === 1 ? "a line comment" : `${n} line comments`}, ready to send to ${done.name}`;
  return `The review queue, sorted by risk, with ${done.name}'s fix in ${done.engine}: ${SAMPLE.diff.files} files, +${SAMPLE.diff.adds} −${SAMPLE.diff.dels}, labelled "Touches auth paths" with its check passed. Second opinion asks ${SAMPLE.reviewer}, which reads the diff without editing anything and ${found}.`;
}
