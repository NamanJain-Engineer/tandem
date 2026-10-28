import { Fragment } from "react";
import { SAMPLE, t } from "../../../lib/scenes";
import { AppWindow, Count, Dot, Glyph, StatusBar, fx, visit } from "./parts";

/** The diff the scene reviews: checkout-fix's change to src/cart/checkout.ts,
 * the checkout function's hunk with its old and new line numbers, as
 * DiffView shows them. Each row is the demo's sample file's line at that
 * number (shop/main and shop/checkout-fix, a test checks). */
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
/** Where the second opinion's comments land: under the rows at their lines.
 * A comment on a line the diff doesn't show fails the build. */
const COMMENTED = SAMPLE.comments.map((c) => {
  const row = REVIEW_DIFF.findIndex(([, , , newNo]) => newNo === c.line);
  if (row < 0) throw new Error(`scenes: the Review scene's diff has no line ${c.line} to comment on`);
  return row;
});
/** The first commented row: the rows under it move down as the comments arrive. */
const FIRST = Math.min(...COMMENTED);
/** When Codex is done. Its comments arrive together then, with its done line,
 * as `review:second_opinion` adds them in the app (D-3000). */
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
      {comments && COMMENTED.includes(from + k) && <Note row={from + k} />}
    </Fragment>
  ));
}

/** A comment under its line, as DiffView's annotation row: the second
 * opinion's comments are the review's own, to edit or remove. */
function Note({ row }: { row: number }) {
  return (
    <span className="sc-diff-note">
      <span className="sc-diff-note-text">{SAMPLE.comments[COMMENTED.indexOf(row)]?.text}</span>
      <span className="sc-btn sc-btn-ghost">{t("worktree.review.edit")}</span>
      <span className="sc-btn sc-btn-ghost">{t("worktree.review.remove")}</span>
    </span>
  );
}

/** "Review and a second opinion" (D-1880): the review queue (F-DB-04) with
 * Claude Code's finished fix on top, sorted by risk; Second opinion asks
 * Codex, which reads the diff without editing anything (F-WT-05); its
 * findings land together as line comments under their lines, ready to send
 * to the agent. About 5 s, played once. */
export function ReviewScene() {
  const claude = SAMPLE.worktrees.claude;
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
              <Glyph engine={claude.engine} focused />
              <span className="sc-card-main">
                <span className="sc-card-title">{t("review.diffTitle", { name: claude.engine })}</span>
                <span className="sc-card-meta">
                  {SAMPLE.workspace} · {claude.branch}
                </span>
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
          <div className="sc-item" {...fx("slide", 0.35)}>
            <span className="sc-toast-row">
              <Glyph engine="Cursor Agent" />
              <span className="sc-card-main">
                <span className="sc-card-title">{t("review.diffTitle", { name: "Cursor Agent" })}</span>
                <span className="sc-card-meta">api · rate-limits</span>
              </span>
            </span>
            <span className="sc-stat">
              {t("review.stat", { n: 2, adds: 0, dels: 0 }).split(" · ")[0]} · <span className="sc-add">+24</span>{" "}
              <span className="sc-del">−3</span>
            </span>
          </div>
        </div>

        <div className="sc-diff" {...fx("in", 0.4)}>
          <div className="sc-diff-head">
            <span className="sc-branch">src/cart/checkout.ts</span>
            <span className="sc-swap sc-diff-state">
              <span {...fx("out", 2.1)} />
              {/* Reviewing until it's done. */}
              <span {...visit(2.1, 1.7)}>
                <Dot status="working" /> {t("worktree.opinion.reviewing", { engine: "Codex" })}
              </span>
              <span {...fx("pop", DONE)}>
                <Dot status="done" /> {t("worktree.opinion.done", { engine: "Codex", n: COMMENTED.length })}
              </span>
            </span>
          </div>
          <div className="sc-diff-body">
            <Rows from={0} to={FIRST + 1} />
            {/* Under the first commented line: the rows as they were until
                Codex is done, then the same rows with its comments, in one
                cell, so nothing waits for the comments as a gap. */}
            <span className="sc-diff-swap">
              <span className="sc-diff-rows" {...fx("out", DONE)}>
                <Rows from={FIRST + 1} to={REVIEW_DIFF.length} />
              </span>
              <span className="sc-diff-rows" {...fx("in", DONE)}>
                <Note row={FIRST} />
                <Rows from={FIRST + 1} to={REVIEW_DIFF.length} comments />
              </span>
            </span>
          </div>
          <div className="sc-diff-foot">
            <span className="sc-btn sc-btn-primary" {...fx("pop", DONE + 0.5)}>
              {t("worktree.review.sendTo", { name: claude.engine })}
            </span>
          </div>
        </div>
      </div>
      <StatusBar branch={claude.branch} />
    </AppWindow>
  );
}

export const REVIEW_CAPTION = "Finished work waits in one queue. Another engine reads it and leaves line comments.";

export function reviewSceneText(): string {
  return `The review queue, sorted by risk: Claude Code's fix on top, ${SAMPLE.diff.files} files, +${SAMPLE.diff.adds} −${SAMPLE.diff.dels}, labelled "Touches auth paths" with its check passed. Second opinion asks Codex, which reads the diff without editing anything and leaves two line comments, ready to send to the agent.`;
}
