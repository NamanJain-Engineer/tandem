import { ClipboardCheck, LayoutDashboard, LayoutGrid, Maximize2, Pin, Plus, X } from "lucide-react";
import { Fragment, type ReactNode } from "react";
import { APPROVE_WAITS, RADAR, SAMPLE, SETTLE, SINCE_START, t, tabTitle } from "../../../lib/scenes";
import { AgentsSegment, AppIcon, AppWindow, Badge, Changes, DotChange, Lines, Pane, ReviewCard, ReviewQueue, StatusBar, Toast, fx, pasted } from "./parts";
import { PaneCount, RadarMark, Rail, Working, WorktreeRow, panesIn } from "./rail";

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
 * toast it dismisses is gone then, at once, and the note is typed. "Warned
 * both agents" shows once both notes are, for the 5 s a toast stays: one
 * toast at a time in the same place. */
const CLICK = HERO_T.warn + 0.26;
const TYPED = CLICK + 0.1;
const WARNED = TYPED + 0.08;
const TOAST_STAY = 5;
/** When the card's Approve turns on, from 0.13.0 (F-TL-12.12): SETTLE_MS
 * after the card arrives, since nothing moves or covers it, as in the app at
 * every window size. A phone's frame is the site's own layout, which no app
 * window makes: there the card's buttons fall to the frame's foot, under
 * "Warned both agents" until it goes, and the steady box holds Approve off
 * under a cover as after a move (steadyBox.ts), so there it turns on
 * SETTLE_MS after the toast goes (D-3008). */
export const HERO_READY = {
  at: HERO_T.done + SETTLE,
  narrow: Math.max(HERO_T.done, RADAR ? WARNED + TOAST_STAY : 0) + SETTLE,
} as const;

/** The agents at work and waiting across the repository, from each time
 * on: all of them, then the one that asks waits, then the finished one is
 * done. */
const n = SAMPLE.order.length;
export const HERO_AGENTS = [
  [0, { waiting: 0, working: n }],
  [HERO_T.ask, { waiting: 1, working: n - 1 }],
  [HERO_T.done, { waiting: 1, working: n - 2 }],
] as const;

/** A worktree's row in the hero (WorktreeRow), from each time on: its slot
 * (its agent at work, "Needs you" in amber once it waits, nothing once it's
 * done), the radar's mark once the radar sees it share a symbol (from
 * 0.13.0: D-2704, D-3006), its count of panes. The mark is the radar alone
 * beside the row's status, and the symbol too once the agent is done
 * (RadarMark, D-2613). The worktree in view is the selected one. */
function HeroRow({ branch, mark }: { branch: string; mark?: number }) {
  const count = <PaneCount n={panesIn(branch)} />;
  const radar = (compact: boolean) => mark !== undefined && <RadarMark compact={compact} {...fx("enter", mark)} />;
  const asks = (
    <span className="sc-wtrow-ask" {...fx("enter", HERO_T.ask)}>
      <Badge variant="waiting" quiet>
        {t("rail.waiting")}
      </Badge>
    </span>
  );
  const steps: Array<readonly [number, ReactNode]> = [[0, [<Working key="slot" />, <Fragment key="n">{count}</Fragment>]]];
  if (branch === waiting.branch) steps.push([HERO_T.ask, [<Fragment key="slot">{asks}</Fragment>, <Fragment key="n">{count}</Fragment>]]);
  if (mark !== undefined) steps.push([mark, [<Working key="slot" />, <Fragment key="mark">{radar(true)}</Fragment>, <Fragment key="n">{count}</Fragment>]]);
  if (branch === finished.branch) steps.push([HERO_T.done, [<Fragment key="mark">{radar(false)}</Fragment>, <Fragment key="n">{count}</Fragment>]]);
  steps.sort(([a], [b]) => a - b);
  return <WorktreeRow branch={branch} on={branch === finished.branch} steps={steps} />;
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
              {/* From 0.13.0 its Approve waits until its place has held still, uncovered (F-TL-12.12): it arrives at Done and holds still from then. */}
              <ReviewCard
                lane={finished}
                minutes={0}
                closing={finished.closing}
                diff={SAMPLE.diff}
                risks={[t("review.rule.auth")]}
                passed={HERO_T.passed}
                ready={APPROVE_WAITS ? HERO_READY.at : undefined}
                readyNarrow={APPROVE_WAITS ? HERO_READY.narrow : undefined}
              />
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
        <Rail>
          {SAMPLE.order.map((w) => {
            const i = SAMPLE.radar.between.indexOf(w.branch);
            return <HeroRow key={w.branch} branch={w.branch} mark={RADAR && i >= 0 ? HERO_T.mark + i * 0.1 : undefined} />;
          })}
        </Rail>
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
              stay={CLICK - HERO_T.notice}
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
