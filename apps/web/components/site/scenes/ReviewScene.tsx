import { SAMPLE, t } from "../../../lib/scenes";
import { AppWindow, Count, Dot, Glyph, fx } from "./parts";

/** The diff the scene reviews: src/cart/checkout.ts, as sample code. */
const DIFF: ReadonlyArray<readonly [" " | "+" | "-", string]> = [
  [" ", "export async function checkout(cart: Cart, req: Request) {"],
  ["-", "  const session = await getSession(req);"],
  ["-", "  if (!session) return redirect(\"/login\");"],
  ["+", "  const session = await refreshSession(req);"],
  ["+", "  if (!session) {"],
  ["+", "    return redirect(`/login?next=${encodeURIComponent(req.url)}`);"],
  ["+", "  }"],
  [" ", "  const total = applyCoupon(cart, session.coupon);"],
  ["+", "  if (cart.expiresAt < Date.now()) return expired(cart);"],
  [" ", "  return charge(session.customer, total);"],
  [" ", "}"],
];
/** Where the reviewer's comments land: under these diff rows. */
const COMMENTED = [5, 8];

/** "Review and a second opinion" (D-1880): the review queue (F-DB-04) with
 * Claude Code's finished fix on top, sorted by risk; Second opinion asks
 * Codex, which reads the diff without editing anything (F-WT-05); its
 * findings land as line comments, ready to send to the agent. About 7 s,
 * played once. */
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
              <span data-fx-visit="" style={{ "--at": "2.1s", "--stay": "2.1s" } as React.CSSProperties}>
                <Dot status="working" /> {t("worktree.opinion.reviewing", { engine: "Codex" })}
              </span>
              <span {...fx("pop", 4.2)}>
                <Dot status="done" /> {t("worktree.opinion.done", { engine: "Codex", n: COMMENTED.length })}
              </span>
            </span>
          </div>
          <div className="sc-diff-body">
            {DIFF.map(([mark, code], i) => (
              <span key={code} className={`sc-diff-line sc-diff-${mark === "+" ? "add" : mark === "-" ? "del" : "ctx"}`}>
                <span className="sc-diff-no">{i + 11}</span>
                <span className="sc-diff-mark">{mark}</span>
                <span className="sc-diff-code">{code}</span>
                {COMMENTED.includes(i) && (
                  <span className="sc-comment" {...fx("pop", COMMENTED.indexOf(i) === 0 ? 3.4 : 3.9)}>
                    <Glyph engine="Codex" />
                    <span>{SAMPLE.comments[COMMENTED.indexOf(i)]?.text}</span>
                  </span>
                )}
              </span>
            ))}
          </div>
          <div className="sc-diff-foot">
            <span className="sc-btn sc-btn-primary" {...fx("pop", 4.6)}>
              {t("worktree.review.send")}
            </span>
          </div>
        </div>
      </div>
      <div className="sc-statusbar">
        <span>{t("status.branch", { name: claude.branch, counts: "" }).split(",")[0]}</span>
        <span className="sc-statusbar-right">
          <span>{t("status.online")}</span>
        </span>
      </div>
    </AppWindow>
  );
}

export const REVIEW_CAPTION = "Finished work waits in one queue. Another engine reads it and leaves line comments.";

export function reviewSceneText(): string {
  return `The review queue, sorted by risk: Claude Code's fix on top, ${SAMPLE.diff.files} files, +${SAMPLE.diff.adds} −${SAMPLE.diff.dels}, labelled "Touches auth paths" with its check passed. Second opinion asks Codex, which reads the diff without editing anything and leaves two line comments, ready to send to the agent.`;
}
