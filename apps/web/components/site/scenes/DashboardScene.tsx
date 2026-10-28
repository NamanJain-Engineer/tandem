import type { ReactNode } from "react";
import { SAMPLE, t } from "../../../lib/scenes";
import { AppWindow, Changes, Dot, DotChange, Glyph, StatusBar, fx } from "./parts";

const { finished, working, waiting } = SAMPLE.worktrees;

/** When the waiting worktree's agent asks, and when the finished one is
 * done: the cards, the filter counts and the Review count change then. */
export const DASH_T = { ask: 2, done: 4.6 } as const;

/** An agent's card as the Dashboard draws it (F-DB-01): its engine's tile,
 * its first name (cardTitle) and, under it, its engine and its worktree's
 * name (cardMeta: a worktree's workspace is its own, and its branch,
 * tandem/<name>, is shown as the name, so once: D-1898); its latest line,
 * and its state. The story gives no account, so none shows. */
function Card({ lane, status, line }: { lane: typeof finished; status: ReactNode; line: ReactNode }) {
  return (
    <span className="sc-card">
      <Glyph engine={lane.engine} />
      <span className="sc-card-main">
        <span className="sc-card-title">{lane.name}</span>
        <span className="sc-card-meta">{[lane.engine, lane.branch].join(" · ")}</span>
      </span>
      <span className="sc-card-line">{line}</span>
      <span className="sc-card-status">{status}</span>
    </span>
  );
}

/** A line that changes once: `from` until `seconds`, then `to`. */
function Line({ from, to, seconds }: { from: string; to: string; seconds: number }) {
  return (
    <span className="sc-swap">
      <span {...fx("out", seconds)}>{from}</span>
      <span {...fx("in", seconds)}>{to}</span>
    </span>
  );
}

/** A pane's line as a card shows it: without the terminal's indent. */
const last = (lines: readonly string[], back = 1) => (lines.at(-back) ?? "").trim();

/** "Who needs you" (D-1880): the Dashboard over the story's repository,
 * grouped by project, each worktree's agent under its branch, as the app
 * groups a repository's worktrees. The waiting one's engine asks to run
 * its tests: its card turns to Waiting for you, with the question as its
 * line; the finished one finishes; the working one stays at work; the
 * filter counts follow. About 7 s, played once, resting with the waiting
 * agent on you. */
export function DashboardScene() {
  const n = SAMPLE.order.length;
  const card = (branch: string) => {
    if (branch === waiting.branch)
      return (
        <span className="sc-card-wrap sc-card-ask">
          {/* Its engine's own question, as the card's line: there's no
              Vibezeo approval to answer here, so no buttons (D-3001). */}
          <Card
            lane={waiting}
            status={<DotChange from="working" to="waiting" seconds={DASH_T.ask} />}
            line={<Line from={last(SAMPLE.lines.waiting)} to={SAMPLE.question.preview} seconds={DASH_T.ask} />}
          />
          <span className="sc-ring" {...fx("pop", DASH_T.ask)} />
        </span>
      );
    if (branch === finished.branch)
      return (
        <Card
          lane={finished}
          status={<DotChange from="working" to="done" seconds={DASH_T.done} />}
          line={<Line from={last(SAMPLE.lines.finished, 2)} to={last(SAMPLE.lines.finished)} seconds={DASH_T.done} />}
        />
      );
    return <Card lane={working} status={<Dot status="working" />} line={last(SAMPLE.lines.working)} />;
  };
  return (
    <AppWindow className="sc-dashboard">
      <div className="sc-dash">
        <div className="sc-dash-head">
          <span className="sc-dash-title">{t("dashboard.count", { n })}</span>
          <span className="sc-seg">
            <span className="sc-mode sc-mode-on">{t("review.viewAgents", { n })}</span>
            <span className="sc-mode">
              <Changes steps={[[0, t("review.viewReview", { n: 0 })], [DASH_T.done, t("review.viewReview", { n: 1 })]]} />
            </span>
          </span>
          <span className="sc-dash-group">
            {t("dashboard.groupBy")} <span className="sc-mode sc-mode-on">{t("dashboard.byProject")}</span>
          </span>
        </div>
        <div className="sc-chips sc-dash-chips">
          <span className="sc-filter sc-filter-on">{t("dashboard.chip.all", { n })}</span>
          <span className="sc-filter">
            <Changes steps={[[0, t("dashboard.chip.waiting", { n: 0 })], [DASH_T.ask, t("dashboard.chip.waiting", { n: 1 })]]} />
          </span>
          <span className="sc-filter">
            <Changes
              steps={[
                [0, t("dashboard.chip.working", { n })],
                [DASH_T.ask, t("dashboard.chip.working", { n: n - 1 })],
                [DASH_T.done, t("dashboard.chip.working", { n: n - 2 })],
              ]}
            />
          </span>
          <span className="sc-filter">
            <Changes steps={[[0, t("dashboard.chip.done", { n: 0 })], [DASH_T.done, t("dashboard.chip.done", { n: 1 })]]} />
          </span>
        </div>

        {/* The repository's group, and in it each worktree's, by branch. */}
        <div className="sc-group" {...fx("in", 0.2)}>
          <span className="sc-group-head">{`${SAMPLE.workspace} · ${n}`}</span>
          <span className="sc-group-lanes">
            {SAMPLE.order.map((w) => (
              <span key={w.branch} className="sc-group">
                <span className="sc-group-head">{`${w.branch} · 1`}</span>
                {card(w.branch)}
              </span>
            ))}
          </span>
        </div>
      </div>
      <StatusBar branch={SAMPLE.base} />
    </AppWindow>
  );
}

export const DASHBOARD_CAPTION = "Every agent across your workspaces, and the one waiting on you.";

export function dashboardSceneText(): string {
  return `The Dashboard shows ${SAMPLE.order.length} agents in ${SAMPLE.workspace}, grouped by project, each under its worktree's branch. ${waiting.name}, in ${waiting.engine}, asks in its terminal to run ${SAMPLE.ask}: the card turns to Waiting for you and shows the question. ${finished.name}, in ${finished.engine}, finishes its tests and turns Done, and ${working.name}, in ${working.engine}, keeps working. The filter counts change to 1 waiting, 1 working and 1 done.`;
}
