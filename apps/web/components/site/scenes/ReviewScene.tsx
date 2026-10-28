import type { ReactNode } from "react";
import { SAMPLE, t } from "../../../lib/scenes";
import { AppWindow, Count, DiffRows, Glyph, StatusBar, Toast, fx, visit, type DiffRow } from "./parts";

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
          <div className="sc-dash-head">
            <span className="sc-dash-title">{t("review.toReview")}</span>
            <span className="sc-dash-group">
              {t("review.sortBy")} <span className="sc-mode sc-mode-on">{t("review.byRisk")}</span>
            </span>
          </div>
          <div className="sc-item sc-item-on" {...fx("slide", 0.2)}>
            <span className="sc-toast-row">
              <Glyph engine={done.engine} focused />
              {/* ReviewQueue's card: the agent's first name, then its engine,
                  its workspace (a worktree's is its own, named for it) and
                  branch (tandem/<name>, which the site shows as the name, so
                  once: D-1898), and how long ago it finished. */}
              <span className="sc-card-main">
                <span className="sc-card-title">{done.name}</span>
                <span className="sc-card-meta">{[done.engine, done.branch, t("time.minutes", { n: REVIEW_T.finished })].join(" · ")}</span>
              </span>
            </span>
            <span className="sc-stat">
              {t("review.stat", { n: SAMPLE.diff.files, adds: 0, dels: 0 }).split(" · ")[0]} ·{" "}
              <span className="sc-add">
                <Count to={SAMPLE.diff.adds} seconds={0.5} prefix="+" />
              </span>{" "}
              <span className="sc-del">
                <Count to={SAMPLE.diff.dels} seconds={0.5} prefix="−" />
              </span>
            </span>
            <span className="sc-chips">
              <span className="sc-chip" {...fx("pop", 1)}>
                {t("review.rule.auth")}
              </span>
              <span className="sc-chip sc-chip-ok" {...fx("pop", 1.2)}>
                {t("review.check.passed")}
              </span>
            </span>
            {/* ReviewQueue's actions, as it styles them: Approve the primary,
                Open diff secondary, the worktree's own ghosts. */}
            <span className="sc-ask-actions sc-item-actions">
              <span className="sc-btn sc-btn-sm sc-btn-primary">{t("review.approve")}</span>
              <span className="sc-btn sc-btn-sm">{t("review.openDiff")}</span>
              <span className="sc-btn sc-btn-sm sc-btn-ghost">{t("review.sendBack")}</span>
              <span className="sc-btn sc-btn-sm sc-btn-ghost" {...fx("press", REVIEW_T.ask)}>
                {t("review.secondOpinion")}
              </span>
              <span className="sc-btn sc-btn-sm sc-btn-ghost">{t("review.merge")}</span>
            </span>
          </div>
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
              worktree's one agent, by its name. With no comments it goes
              when the review's done, as the app's does. */}
          <span className="sc-review-bar" {...(n > 0 ? fx("in", REVIEW_T.reviewing) : visit(REVIEW_T.reviewing, DONE - REVIEW_T.reviewing))}>
            <span className="sc-bar-states">
              <span className="sc-bar-state" {...(n > 0 ? visit(REVIEW_T.reviewing, DONE - REVIEW_T.reviewing) : {})}>
                <span className="sc-spin" />
                <span className="sc-review-doing">{t("worktree.opinion.reviewing", { engine: reviewer })}</span>
                <span className="sc-btn sc-btn-sm sc-btn-ghost">{t("worktree.opinion.openThread")}</span>
              </span>
              {n > 0 && (
                <span className="sc-bar-state" {...fx("in", DONE)}>
                  <span className="sc-review-count">{t("worktree.review.count", { n })}</span>
                  <span className="sc-btn sc-btn-sm">{t("worktree.review.copy")}</span>
                  <span className="sc-btn sc-btn-sm sc-btn-primary" {...fx("pop", REVIEW_T.send)}>
                    {t("worktree.review.sendTo", { name: done.name })}
                  </span>
                </span>
              )}
            </span>
          </span>
        </div>
        {/* The reviewer's done toast (SecondOpinion.tsx), with Show diff, or
            its no-comments one. */}
        {n > 0 ? (
          <Toast title={t("worktree.opinion.done", { engine: reviewer, n })} action={t("worktree.opinion.showDiff")} at={DONE} stay={REVIEW_T.send - 0.1 - DONE} />
        ) : (
          <Toast title={t("worktree.opinion.none", { engine: reviewer })} at={DONE} />
        )}
      </div>
      <StatusBar branch={done.branch} />
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
