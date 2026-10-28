import type { Status } from "@tandem/ui/components/StatusDot";
import type { ReactNode } from "react";
import { SAMPLE, t } from "../../../lib/scenes";
import { AppWindow, Changes, Dot, DotChange, Glyph, StatusBar, fx } from "./parts";

function Card({
  engine,
  n,
  branch,
  status,
  line,
  wants,
}: {
  engine: string;
  n: number;
  branch: string;
  status: ReactNode;
  line: ReactNode;
  wants?: ReactNode;
}) {
  return (
    <span className="sc-card">
      <Glyph engine={engine} />
      <span className="sc-card-main">
        <span className="sc-card-title">{t("pane.engine", { engine, n })}</span>
        <span className="sc-card-meta">{branch}</span>
      </span>
      <span className="sc-card-line">{line}</span>
      <span className="sc-card-status">{status}</span>
      {wants}
    </span>
  );
}

const still = (s: Status) => <Dot status={s} />;

/** "Who needs you" (D-1880): the Dashboard across three workspaces, grouped
 * by project. Codex asks to run a migration: its card turns to Waiting for
 * you and offers the answer in place; Claude Code finishes; the filter
 * counts follow. About 7 s, played once, resting with Codex waiting on you. */
export function DashboardScene() {
  return (
    <AppWindow className="sc-dashboard">
      <div className="sc-dash">
        <div className="sc-dash-head">
          <span className="sc-dash-title">{t("dashboard.count", { n: 5 })}</span>
          <span className="sc-seg">
            <span className="sc-mode sc-mode-on">{t("review.viewAgents", { n: 5 })}</span>
            <span className="sc-mode">{t("review.viewReview", { n: 2 })}</span>
          </span>
          <span className="sc-dash-group">
            {t("dashboard.groupBy")} <span className="sc-mode sc-mode-on">{t("dashboard.byProject")}</span>
          </span>
        </div>
        <div className="sc-chips sc-dash-chips">
          <span className="sc-filter sc-filter-on">{t("dashboard.chip.all", { n: 5 })}</span>
          <span className="sc-filter">
            <Changes steps={[[0, t("dashboard.chip.waiting", { n: 0 })], [2, t("dashboard.chip.waiting", { n: 1 })]]} />
          </span>
          <span className="sc-filter">
            <Changes
              steps={[
                [0, t("dashboard.chip.working", { n: 3 })],
                [2, t("dashboard.chip.working", { n: 2 })],
                [4.6, t("dashboard.chip.working", { n: 1 })],
              ]}
            />
          </span>
          <span className="sc-filter">
            <Changes steps={[[0, t("dashboard.chip.done", { n: 2 })], [4.6, t("dashboard.chip.done", { n: 3 })]]} />
          </span>
        </div>

        <div className="sc-group" {...fx("in", 0.2)}>
          <span className="sc-group-head">{SAMPLE.workspace}</span>
          <span className="sc-card-wrap sc-card-ask">
            <Card
              engine="Codex"
              n={2}
              branch={SAMPLE.worktrees.working.branch}
              status={<DotChange from="working" to="waiting" seconds={2} />}
              line={
                <span className="sc-swap">
                  <span {...fx("out", 2)}>editing src/cart/rules.ts</span>
                  <span {...fx("in", 2)}>{t("dashboard.wants", { summary: "run pnpm db:migrate" })}</span>
                </span>
              }
              wants={
                <span className="sc-card-actions" {...fx("rise", 2.3)}>
                  <span className="sc-btn sc-btn-primary">{t("dashboard.allowOnce")}</span>
                  <span className="sc-btn">{t("dashboard.deny")}</span>
                </span>
              }
            />
            <span className="sc-ring" {...fx("pop", 2)} />
          </span>
          <Card
            engine="Claude Code"
            n={1}
            branch={SAMPLE.worktrees.finished.branch}
            status={<DotChange from="working" to="done" seconds={4.6} />}
            line={
              <span className="sc-swap">
                <span {...fx("out", 4.6)}>$ pnpm test cart</span>
                <span {...fx("in", 4.6)}>Tests 42 passed</span>
              </span>
            }
          />
          <Card engine="Gemini CLI" n={3} branch={SAMPLE.worktrees.waiting.branch} status={still("working")} line="editing src/search/results.tsx" />
        </div>
        <div className="sc-group" {...fx("in", 0.35)}>
          <span className="sc-group-head">api</span>
          <Card engine="Cursor Agent" n={1} branch="rate-limits" status={still("done")} line="All 118 tests pass" />
        </div>
        <div className="sc-group" {...fx("in", 0.5)}>
          <span className="sc-group-head">docs</span>
          <Card engine="OpenCode" n={1} branch="release-notes" status={still("done")} line="Wrote the release notes" />
        </div>
      </div>
      <StatusBar branch={SAMPLE.base} />
    </AppWindow>
  );
}

export const DASHBOARD_CAPTION = "Every agent across your workspaces, and the one waiting on you, with its answer in place.";

export function dashboardSceneText(): string {
  return "The Dashboard shows five agents across three workspaces, grouped by project. Codex asks to run a database migration: its card turns to Waiting for you, with Allow once and Deny on the card. Claude Code finishes its tests and turns Done, and the filter counts change to 1 waiting, 1 working and 3 done.";
}
