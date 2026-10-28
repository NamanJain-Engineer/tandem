import { Fragment } from "react";
import { SAMPLE, t } from "../../../lib/scenes";
import { AppWindow, Count, Dot, Glyph, StatusBar, fx, visit } from "./parts";

/** The file the second opinion comments on: the scene shows its diff. */
export const REVIEW_FILE = SAMPLE.comments[0]?.file ?? "";

/** The diff the scene reviews: the finished worktree's change to
 * REVIEW_FILE, the checkout function's hunk with its old and new line
 * numbers, as DiffView shows them. It's a run of git's own diff of the demo's
 * sample files (the base branch's and the worktree's; a test checks). */
export const REVIEW_DIFF: ReadonlyArray<readonly [" " | "+" | "−", string, number | null, number | null]> = [
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

/** Where the second opinion's comments land: under the rows at their lines. */
const NOTES = notesFor(REVIEW_FILE, REVIEW_DIFF, SAMPLE.comments);
/** The first commented row: the rows under it move down as the comments
 * arrive. With no comments, nothing does. */
const FIRST = NOTES.size > 0 ? Math.min(...NOTES.keys()) : REVIEW_DIFF.length;
/** When the reviewer is done. Its comments arrive together then, with its
 * done line, as `review:second_opinion` adds them in the app (D-3000). */
const DONE = 3.8;

/** Diff rows `from` to `to`, each commented one with its comment under it
 * when `comments` is on. */
function Rows({ from, to, comments = false }: { from: number; to: number; comments?: boolean }) {
  return REVIEW_DIFF.slice(from, to).map(([mark, code, oldNo, newNo], k) => (
    <Fragment key={code}>
      <span className={`sc-diff-line sc-diff-${mark === "+" ? "add" : mark === "−" ? "del" : "ctx"}`}>
        <span className="sc-diff-no">{oldNo ?? ""}</span>
        <span className="sc-diff-no">{newNo ?? ""}</span>
        <span className="sc-diff-mark">{mark}</span>
        <span className="sc-diff-code">{code}</span>
      </span>
      {comments && <Notes row={from + k} />}
    </Fragment>
  ));
}

/** A row's comments under it, each as DiffView's annotation row: the
 * second opinion's comments are the review's own, to edit or remove. Only
 * they fade in (D-3000). */
function Notes({ row }: { row: number }) {
  return (NOTES.get(row) ?? []).map((text) => (
    <span key={text} className="sc-diff-note" {...fx("in", DONE)}>
      <span className="sc-diff-note-text">{text}</span>
      <span className="sc-btn sc-btn-ghost">{t("worktree.review.edit")}</span>
      <span className="sc-btn sc-btn-ghost">{t("worktree.review.remove")}</span>
    </span>
  ));
}

/** "Review and a second opinion" (D-1880): the review queue (F-DB-04) with
 * the finished worktree's fix, the one piece of finished work, sorted by
 * risk; Second opinion asks another engine, which reads the diff without
 * editing anything (F-WT-05); its findings land together as line comments
 * under their lines, ready to send to the agent. About 5 s, played once. */
export function ReviewScene() {
  const done = SAMPLE.worktrees.finished;
  const reviewer = SAMPLE.reviewer;
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
              {/* ReviewQueue's card: the agent's first name, then its engine, workspace and branch. */}
              <span className="sc-card-main">
                <span className="sc-card-title">{done.name}</span>
                <span className="sc-card-meta">{[done.engine, SAMPLE.workspace, done.branch].join(" · ")}</span>
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
            <span className="sc-ask-actions">
              <span className="sc-btn">{t("review.approve")}</span>
              <span className="sc-btn">{t("review.openDiff")}</span>
              <span className="sc-btn sc-btn-primary" {...fx("press", 1.9)}>
                {t("review.secondOpinion")}
              </span>
            </span>
          </div>
        </div>

        <div className="sc-diff" {...fx("in", 0.4)}>
          <div className="sc-diff-head">
            <span className="sc-branch">{REVIEW_FILE}</span>
            <span className="sc-swap sc-diff-state">
              <span {...fx("out", 2.1)} />
              {/* Reviewing until it's done. */}
              <span {...visit(2.1, 1.7)}>
                <Dot status="working" /> {t("worktree.opinion.reviewing", { engine: reviewer })}
              </span>
              <span {...fx("pop", DONE)}>
                <Dot status="done" />{" "}
                {SAMPLE.comments.length > 0 ? t("worktree.opinion.done", { engine: reviewer, n: SAMPLE.comments.length }) : t("worktree.opinion.none", { engine: reviewer })}
              </span>
            </span>
          </div>
          <div className="sc-diff-body">
            <Rows from={0} to={FIRST + 1} />
            {/* Under the first commented line: the rows as they were until
                the reviewer is done, then the same rows with its comments, in
                one cell, so nothing waits for the comments as a gap. The app
                inserts them in one go and the rows below just move down, so
                the stacks swap at once and only the comments fade in. */}
            {NOTES.size > 0 && (
              <span className="sc-diff-swap">
                <span className="sc-diff-rows" {...fx("cut", DONE)}>
                  <Rows from={FIRST + 1} to={REVIEW_DIFF.length} />
                </span>
                <span className="sc-diff-rows" {...fx("show", DONE)}>
                  <Notes row={FIRST} />
                  <Rows from={FIRST + 1} to={REVIEW_DIFF.length} comments />
                </span>
              </span>
            )}
          </div>
          <div className="sc-diff-foot">
            <span className="sc-btn sc-btn-primary" {...fx("pop", DONE + 0.5)}>
              {t("worktree.review.sendTo", { name: done.name })}
            </span>
          </div>
        </div>
      </div>
      <StatusBar branch={done.branch} />
    </AppWindow>
  );
}

export const REVIEW_CAPTION = "Finished work waits in one queue. Another engine reads it and leaves line comments.";

export function reviewSceneText(): string {
  const done = SAMPLE.worktrees.finished;
  const n = SAMPLE.comments.length;
  return `The review queue, sorted by risk, with ${done.name}'s fix in ${done.engine}: ${SAMPLE.diff.files} files, +${SAMPLE.diff.adds} −${SAMPLE.diff.dels}, labelled "Touches auth paths" with its check passed. Second opinion asks ${SAMPLE.reviewer}, which reads the diff without editing anything and leaves ${n === 1 ? "a line comment" : `${n} line comments`}, ready to send to ${done.name}.`;
}
