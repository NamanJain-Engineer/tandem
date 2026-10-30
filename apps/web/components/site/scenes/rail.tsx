import { Check, ChevronDown, Cloud, Folder, GitBranch, Plus, Radar, XCircle } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "../../../lib/cx";
import { RAIL_FOLDS, SAMPLE, t } from "../../../lib/scenes";
import { AppIcon, Badge, Dot, fx, type Motion } from "./parts";

// The Code rail as CodeRail draws it: the hero's, and from 0.13.0 the CI
// scene's too (D-2702, D-3006). Its rules are rail.css's, which only the
// pages that play those scenes inline.

/** A worktree row's slot (WorkspaceSlot) while its agent works: a working
 * dot and how many work. */
export function Working() {
  return (
    <span className="sc-slot-working">
      <Dot status="working" />
      {t("rail.working", { n: 1 })}
    </span>
  );
}

/** What a worktree's row trails its name with, from each time on (the last
 * the final frame): its slot, its marks, its count of panes. Each set is
 * laid out as it is and swapped in at once, as a re-render swaps it, so the
 * row's parts move over when the app's do. */
export function Trail({ steps }: { steps: ReadonlyArray<readonly [number, ReactNode]> }) {
  return (
    <span className="sc-wtrow-trail">
      {steps.map(([at, parts], i) => {
        const next = steps[i + 1]?.[0];
        return (
          <span key={at} className="sc-wtrow-step" {...(i === 0 ? {} : fx("show", at))}>
            <span className="sc-wtrow-parts" {...(next === undefined ? {} : fx("cut", next))}>
              {parts}
            </span>
          </span>
        );
      })}
    </span>
  );
}

/** A worktree's row as WorktreeRow draws it: its branch icon, its name (the
 * site shows it for the branch, tandem/<name>: D-1898, and from 0.13.0 the
 * app shows it too: D-2611), then what trails it from each time on
 * (`steps`): its slot, its marks and its count of panes. 0.12.0's row keeps
 * room at its end for its menu, which shows on hover; from 0.13.0 the menu
 * covers the row's end only while it's hovered, so the row keeps none
 * (D-2611). `on` is the worktree in view. */
export function WorktreeRow({ branch, on = false, steps }: { branch: string; on?: boolean; steps: ReadonlyArray<readonly [number, ReactNode]> }) {
  return (
    <span className={cx("sc-wtrow", on && "sc-wtrow-on")}>
      <AppIcon icon={GitBranch} className="sc-wtrow-icon" />
      <span className="sc-wtrow-name">{branch}</span>
      <Trail steps={steps} />
      {!RAIL_FOLDS && <span className="sc-wtrow-menu" />}
    </span>
  );
}

/** A row's count of open panes (WorktreeRow's live count): a neutral Badge. */
export function PaneCount({ n }: { n: number }) {
  return <Badge>{n}</Badge>;
}

/** The panes open in each worktree, its row's count: its agent's, and the
 * shell beside the working one, as the demo opens them (backend.ts's
 * PANES). */
export const panesIn = (branch: string) => (branch === SAMPLE.worktrees.working.branch ? 2 : 1);

/** The radar's mark from 0.13.0 (RadarMark; D-2613, D-2700): a waiting
 * Badge that leads with the radar, then the symbol in code type. Beside the
 * row's status, or folded while the row is short of room (D-2701), it's the
 * radar alone. */
export function RadarMark({ compact = false, ...motion }: { compact?: boolean } & Motion) {
  return (
    <span className={cx("sc-wtrow-radar", compact && "sc-wtrow-compact")} {...motion}>
      <Badge variant="waiting">
        <AppIcon icon={Radar} className="sc-wtrow-radar-icon" />
        {!compact && <span className="sc-wtrow-symbol">{SAMPLE.symbol}</span>}
      </Badge>
    </span>
  );
}

/** What a pull request's checks are, as the checks badge says it. */
export type ChecksState = "failed" | "running" | "passed";

/** The checks badge from 0.13.0 (ChecksBadge; D-2614, D-2615, D-2700): its
 * icon (a crossed circle, a spinner, a check mark), then the status bar's
 * words, whole. Beside the row's status, or folded, it's the icon alone. */
export function ChecksMark({ state, compact = false, ...motion }: { state: ChecksState; compact?: boolean } & Motion) {
  return (
    <span className={cx("sc-wtrow-checks", `sc-wtrow-checks-${state}`, compact && "sc-wtrow-compact")} {...motion}>
      {state === "running" ? <span className="sc-spin" /> : <AppIcon icon={state === "failed" ? XCircle : Check} className="sc-wtrow-checks-icon" />}
      {!compact && t(state === "failed" ? "checks.badge.failed" : state === "running" ? "status.ciRunning" : "status.ciPassed")}
    </span>
  );
}

/** The rail as CodeRail draws it (UI_SPEC §3.3): WORKSPACES and +, the
 * repository's row (its folder, its name and, with no agent of its own at
 * work, its branch), under it "No open panes", since it has none of its
 * own, then its worktrees' rows (`children`); Remote hosts at the foot.
 * `className` is a scene's own, to place it. */
export function Rail({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <nav className={cx("sc-crail", className)}>
      <span className="sc-crail-head">
        <span className="sc-crail-title">{t("rail.workspacesHeader")}</span>
        <AppIcon icon={Plus} className="sc-crail-add" />
      </span>
      <span className="sc-crail-list">
        <span className="sc-wsrow">
          <AppIcon icon={ChevronDown} className="sc-wsrow-chevron" />
          <AppIcon icon={Folder} className="sc-wsrow-icon" />
          <span className="sc-wsrow-name">{SAMPLE.workspace}</span>
          <span className="sc-bchip">
            <AppIcon icon={GitBranch} className="sc-bchip-icon" />
            {SAMPLE.base}
          </span>
        </span>
        <span className="sc-crail-group">
          <span className="sc-crail-none">{t("rail.noSessions")}</span>
          {children}
        </span>
      </span>
      <span className="sc-crail-foot">
        <span className="sc-btn sc-btn-ghost sc-crail-remote">
          <AppIcon icon={Cloud} className="sc-crail-remote-icon" />
          {t("rail.remoteHosts")}
        </span>
      </span>
    </nav>
  );
}
