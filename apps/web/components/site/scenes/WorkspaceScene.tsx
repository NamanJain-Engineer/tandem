import { ChevronDown, ClipboardCheck, Cloud, Folder, GitBranch, LayoutDashboard, LayoutGrid, Maximize2, Pin, Plus, X } from "lucide-react";
import { Fragment, type ReactNode } from "react";
import { cx } from "../../../lib/cx";
import { RADAR, SAMPLE, SINCE_START, t, tabTitle } from "../../../lib/scenes";
import { AgentsSegment, AppIcon, AppWindow, Badge, Changes, Dot, DotChange, Lines, Pane, ReviewCard, ReviewQueue, StatusBar, Toast, fx, pasted } from "./parts";

const { finished, working, waiting } = SAMPLE.worktrees;

/** The hero's timeline (D-2702), in seconds from its start: when the
 * waiting worktree's engine asks, and its rail row turns to Needs you; when
 * the intent radar marks the two worktrees that change one symbol, and its
 * notice shows, and when Warn both agents is pressed (from 0.13.0: D-2704);
 * when the finished one's agent is done, and its card arrives in the review
 * queue; and when the repository's check passes on it. */
export const HERO_T = { ask: 2.2, mark: 3.4, notice: 3.8, warn: 4.5, done: 6.4, passed: 8.4 } as const;
/** When the finished worktree's agent is done: its dot, its rail row, the
 * status bar and the review queue change then. */
export const HERO_DONE = HERO_T.done;
/** When the waiting worktree's engine asks to run its tests: it waits on
 * you from then on (D-3001). */
export const HERO_ASK = HERO_T.ask;
/** A press runs 260 ms, and the app acts on the click that ends it: the
 * note is typed then, and the toast it dismisses leaves over 180 ms, the
 * visit's fade. "Warned both agents" shows once that one has gone, for the
 * 5 s a toast stays: one toast at a time in the same place. */
const CLICK = HERO_T.warn + 0.26;
const TYPED = CLICK + 0.1;
const WARNED = CLICK + 0.18;
const TOAST_STAY = 5;

/** The panes open in each worktree, its rail row's count: its agent's, and
 * the shell beside the working one, as the demo opens them (backend.ts's
 * PANES). */
const live = (branch: string) => (branch === working.branch ? 2 : 1);

/** The agents at work and waiting across the repository, from each time
 * on: all of them, then the one that asks waits, then the finished one is
 * done. */
const n = SAMPLE.order.length;
export const HERO_AGENTS = [
  [0, { waiting: 0, working: n }],
  [HERO_T.ask, { waiting: 1, working: n - 1 }],
  [HERO_T.done, { waiting: 1, working: n - 2 }],
] as const;

/** A worktree row's slot (WorkspaceSlot) while its agent works: a working
 * dot and how many work. */
function Working() {
  return (
    <span className="sc-slot-working">
      <Dot status="working" />
      {t("rail.working", { n: 1 })}
    </span>
  );
}

/** What a worktree's row trails its name with, from each time on (the last
 * the final frame): its slot, the radar's mark, its count of panes. Each
 * set is laid out as it is and swapped in at once, as a re-render swaps it,
 * so the row's parts move over when the app's do. */
function Trail({ steps }: { steps: ReadonlyArray<readonly [number, ReactNode]> }) {
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
 * site shows it for the branch, tandem/<name>: D-1898), then its slot
 * (WorkspaceSlot: its agent at work, "Needs you" in amber once it waits,
 * nothing once it's done), the radar's mark once the radar sees it share a
 * symbol (from 0.13.0, D-2704), its count of panes, and room for its menu,
 * which shows on hover but takes its room throughout (0.12.0's WorktreeRow).
 * The worktree in view is the selected one. */
function WorktreeRow({ branch, mark }: { branch: string; mark?: number }) {
  const count = <Badge>{live(branch)}</Badge>;
  const radar = mark !== undefined && (
    <span className="sc-wtrow-radar" {...fx("enter", mark)}>
      <Badge variant="waiting">{t("radar.mark.one", { symbol: SAMPLE.symbol })}</Badge>
    </span>
  );
  const asks = (
    <span className="sc-wtrow-ask" {...fx("enter", HERO_T.ask)}>
      <Badge variant="waiting" quiet>
        {t("rail.waiting")}
      </Badge>
    </span>
  );
  const steps: Array<readonly [number, ReactNode]> = [[0, [<Working key="slot" />, <Fragment key="n">{count}</Fragment>]]];
  if (branch === waiting.branch) steps.push([HERO_T.ask, [<Fragment key="slot">{asks}</Fragment>, <Fragment key="n">{count}</Fragment>]]);
  if (radar) steps.push([mark ?? 0, [<Working key="slot" />, <Fragment key="mark">{radar}</Fragment>, <Fragment key="n">{count}</Fragment>]]);
  if (branch === finished.branch) steps.push([HERO_T.done, [<Fragment key="mark">{radar}</Fragment>, <Fragment key="n">{count}</Fragment>]]);
  steps.sort(([a], [b]) => a - b);
  return (
    <span className={cx("sc-wtrow", branch === finished.branch && "sc-wtrow-on")}>
      <AppIcon icon={GitBranch} className="sc-wtrow-icon" />
      <span className="sc-wtrow-name">{branch}</span>
      <Trail steps={steps} />
      <span className="sc-wtrow-menu" />
    </span>
  );
}

/** The rail as CodeRail draws it (UI_SPEC §3.3): WORKSPACES and +, the
 * repository's row (its folder, its name and, with no agent of its own at
 * work, its branch), under it "No open panes", since it has none of its
 * own, then its worktrees' rows; Remote hosts at the foot. On a phone only
 * the worktrees' rows show, as a strip over the pane. */
function Rail() {
  return (
    <nav className="sc-crail">
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
          {SAMPLE.order.map((w) => {
            const i = SAMPLE.radar.between.indexOf(w.branch);
            return <WorktreeRow key={w.branch} branch={w.branch} mark={RADAR && i >= 0 ? HERO_T.mark + i * 0.1 : undefined} />;
          })}
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

/** The Dashboard tab's pill in the sidebar's header (Sidebar's Pill): its
 * icon, its name, how many agents wait when one does, and room for its ×,
 * which shows on hover. */
function Pill({ waiting: count = 0 }: { waiting?: number }) {
  return (
    <span className="sc-pill">
      <AppIcon icon={LayoutDashboard} className="sc-pill-icon" />
      {t("sidebar.dashboard")}
      {count > 0 && (
        <span className="sc-pill-count" {...fx("enter", HERO_T.ask)}>
          <Badge variant="waiting">{count}</Badge>
        </span>
      )}
      <span className="sc-pill-close" />
    </span>
  );
}

/** The sidebar as Sidebar draws it, on its Dashboard tab: the tab's pill
 * (its waiting count once an agent waits), +, Full screen and Close; then
 * DashboardPanel on its Review view (F-DB-04): "Nothing to review" until the
 * finished worktree's agent is done, then the queue with its card, the
 * check running on it until it passes. Below 1,100 px the app floats it over the panes, as a phone's
 * frame does here. */
function Sidebar() {
  return (
    <aside className="sc-side">
      <span className="sc-side-head">
        {/* The pill without its count, then with it once an agent waits,
            swapped at once as the app re-renders it. */}
        <span className="sc-pills">
          <span {...fx("cut", HERO_T.ask)}>
            <Pill />
          </span>
          <span {...fx("show", HERO_T.ask)}>
            <Pill waiting={1} />
          </span>
        </span>
        {/* +, then Pin while it floats (on a phone's frame), Full screen
            and Close (Sidebar's header). */}
        <span className="sc-side-tools">
          <AppIcon icon={Plus} />
          <AppIcon icon={Pin} className="sc-side-pin" />
          <AppIcon icon={Maximize2} />
          <AppIcon icon={X} />
        </span>
      </span>
      <span className="sc-side-body">
        <span className="sc-seg sc-seg-sm">
          <span className="sc-mode">{t("review.viewAgents", { n })}</span>
          <span className="sc-mode sc-mode-on">
            <Changes
              steps={[
                [0, t("review.viewReview", { n: 0 })],
                [HERO_T.done, t("review.viewReview", { n: 1 })],
              ]}
            />
          </span>
        </span>
        <span className="sc-rq-views">
          <span className="sc-rq-empty" {...fx("cut", HERO_T.done)}>
            <span className="sc-rq-empty-icon">
              <AppIcon icon={ClipboardCheck} />
            </span>
            <span className="sc-rq-empty-title">{t("review.emptyTitle")}</span>
            <span className="sc-rq-empty-body">{t("review.emptyBody")}</span>
          </span>
          <ReviewQueue count={1} {...fx("show", HERO_T.done)}>
            <span {...fx("in", HERO_T.done)}>
              <ReviewCard lane={finished} minutes={0} closing={finished.closing} diff={SAMPLE.diff} risks={[t("review.rule.auth")]} passed={HERO_T.passed} />
            </span>
          </ReviewQueue>
        </span>
      </span>
    </aside>
  );
}

/** The hero's scene (D-1880, D-2702): one Code-mode window as the app lays
 * it out, with one worktree in view. The rail holds the repository and its
 * three worktrees, each with its own agent; Code mode is on the finished
 * one's tab, with its agent's pane alone; the sidebar's Dashboard is on
 * Review. The waiting worktree's engine asks to run its tests, so its row
 * turns to Needs you; from 0.13.0 the intent radar marks the two worktrees
 * that change the same function, and its notice warns both agents: the
 * core's note is typed into each (D-2704). The finished one's agent is done, and its
 * work arrives in the review queue, where the repository's check runs and
 * passes. About 10 s, played once, resting on the last frame. */
export function WorkspaceScene() {
  const radar = RADAR;
  const lines = SAMPLE.lines.finished;
  // The agent's work up to its tests, the radar's note typed in (from 0.13.0), then the tests and its closing words.
  const before = lines.slice(0, -2);
  const tests = lines.slice(-2);
  return (
    <AppWindow className="sc-workspace" workspace={finished.branch}>
      <div className="sc-code">
        <Rail />
        <div className="sc-code-main">
          <div className="sc-tabs">
            <span className="sc-tab">{tabTitle(finished.task)}</span>
            <AppIcon icon={Plus} className="sc-tabs-add" />
            <span className="sc-btn sc-btn-sm sc-btn-ghost sc-tabs-arrange">
              <AppIcon icon={LayoutGrid} className="sc-tabs-arrange-icon" />
              {t("code.arrange")}
            </span>
          </div>
          <Pane
            engine={finished.engine}
            name={finished.name}
            title={finished.title}
            branch={finished.branch}
            focused
            solo
            elapsed={
              <span className="sc-pane-elapsed" {...fx("cut", HERO_T.done)}>
                {t("time.minutes", { n: SINCE_START })}
              </span>
            }
            status={<DotChange from="working" to="done" seconds={HERO_T.done} />}
          >
            <Lines lines={before} start={0.5} end={radar ? 4.2 : 4.6} />
            {/* The note as its terminal echoes a paste; a phone's pane has no room for it. */}
            {radar && <Lines lines={pasted(SAMPLE.radar.note)} start={TYPED} step={0.08} className="sc-line-wrap" fold={() => true} />}
            <Lines lines={tests} start={radar ? 5.5 : 5.2} step={0.5} />
            <Lines lines={[`  ${finished.closing}`]} start={HERO_T.done} className="sc-line-wrap" />
          </Pane>
        </div>
        <Sidebar />
        {radar && (
          <>
            {/* The radar's notice (IntentRadar): a warning toast; its action
                types the core's note into both agents and dismisses it, and
                a success toast says so. */}
            <Toast
              tone="warning"
              title={t("radar.notice.title", { a: finished.branch, b: working.branch, symbols: SAMPLE.symbol })}
              body={t("radar.notice.body")}
              action={t("radar.notice.warn")}
              at={HERO_T.notice}
              stay={WARNED - HERO_T.notice}
              press={HERO_T.warn}
            />
            <Toast tone="success" title={t("radar.notice.warned")} at={WARNED} stay={TOAST_STAY} />
          </>
        )}
      </div>
      <StatusBar branch={finished.branch} segments={<AgentsSegment steps={HERO_AGENTS} />} />
    </AppWindow>
  );
}

/** The scene's short visible caption. */
export const WORKSPACE_CAPTION = "Three agents work in one repository, each in its own worktree, and the rail shows the one that needs you.";

/** What the scene shows, for people who can't see it (its figure's caption). */
export function workspaceSceneText(): string {
  const radar = RADAR
    ? ` The intent radar marks ${finished.branch} and ${working.branch}, which both change ${SAMPLE.symbol}; Warn both agents types a note about it into each agent, ${finished.name}'s too.`
    : "";
  return `Code mode on ${finished.branch}, one of ${n} worktrees of one repository, each with its own agent, and ${finished.name}'s pane alone in its tab. ${finished.name}, in ${finished.engine}, fixes checkout. ${working.name}, in ${working.engine}, keeps working in ${working.branch}, and ${waiting.name}, in ${waiting.engine}, asks in its terminal to run ${SAMPLE.ask} and waits for you: the rail shows ${waiting.branch} as Needs you.${radar} ${finished.name} finishes, and ${finished.name}'s change arrives in the review queue in the sidebar: ${SAMPLE.diff.files} files, +${SAMPLE.diff.adds} −${SAMPLE.diff.dels}, where the repository's check runs and passes.`;
}
