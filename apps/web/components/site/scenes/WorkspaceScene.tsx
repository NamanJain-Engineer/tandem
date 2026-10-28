import { LANDS, SAMPLE, t } from "../../../lib/scenes";
import { AppWindow, Count, Dot, DotChange, Glyph, Lines, Pane, StatusBar, fx, visit } from "./parts";

const { finished, working, waiting } = SAMPLE.worktrees;

/** When the finished worktree's agent is done: its dot, its rail row and its
 * pane's header all change then. */
export const HERO_DONE = 6.4;

/** The hero's scene (D-1880): Code mode over one repository, four live panes,
 * each agent in its own worktree. The finished one fixes checkout; the
 * waiting one asks before running its tests and the approval is answered;
 * on 0.9.0 the intent radar sees the finished and the working one change
 * the same function and both are warned; the finished work arrives
 * in the review queue with its diff counting up. About 11 s, played once,
 * resting on the last frame. */
export function WorkspaceScene() {
  const radar = LANDS;
  return (
    <AppWindow className="sc-workspace">
      <div className="sc-body">
        <aside className="sc-rail">
          <span className="sc-rail-head">{t("rail.workspacesHeader")}</span>
          <span className="sc-ws">
            <span className="sc-ws-name">{SAMPLE.workspace}</span>
            <span className="sc-ws-count">
              <Dot status="working" />2
            </span>
          </span>
          <span className="sc-lane">
            <DotChange from="working" to="done" seconds={HERO_DONE} />
            <span className="sc-lane-name">{finished.branch}</span>
            {radar && (
              <span className="sc-radar" {...fx("pop", 3.4)}>
                {t("radar.mark.one", { symbol: SAMPLE.symbol })}
              </span>
            )}
          </span>
          <span className="sc-lane">
            <Dot status="working" />
            <span className="sc-lane-name">{working.branch}</span>
            {radar && (
              <span className="sc-radar" {...fx("pop", 3.5)}>
                {t("radar.mark.one", { symbol: SAMPLE.symbol })}
              </span>
            )}
          </span>
          <span className="sc-lane">
            <GeminiStatus />
            <span className="sc-lane-name">{waiting.branch}</span>
          </span>
          <span className="sc-rail-foot">{t("rail.live", { n: 4 })}</span>
        </aside>

        <div className="sc-grid">
          <Pane
            engine={finished.engine}
            name={finished.name}
            title={finished.title}
            branch={finished.branch}
            focused
            doneAt={HERO_DONE}
            status={<DotChange from="working" to="done" seconds={HERO_DONE} />}
          >
            <Lines lines={SAMPLE.lines.finished} start={0.5} end={5.3} />
          </Pane>
          <Pane engine={working.engine} name={working.name} title={working.title} branch={working.branch} status={<Dot status="working" />}>
            <Lines lines={SAMPLE.lines.working} start={0.9} end={4.9} />
            <span className="sc-line sc-caret" {...fx("line", 5.9)} />
          </Pane>
          <Pane
            engine={waiting.engine}
            name={waiting.name}
            title={waiting.title}
            branch={waiting.branch}
            className="sc-hide-narrow"
            status={
              <GeminiStatus />
            }
          >
            <Lines lines={SAMPLE.lines.waiting} start={0.7} end={2.1} />
            {/* The approval, answered: it arrives, Allow once is pressed, and it goes. */}
            <span className="sc-ask" {...visit(2.2, 3.1)}>
              <span className="sc-ask-text">{t("dashboard.wants", { summary: `run ${SAMPLE.ask}` })}</span>
              <span className="sc-ask-actions">
                <span className="sc-btn sc-btn-primary" {...fx("press", 4.7)}>
                  {t("dashboard.allowOnce")}
                </span>
                <span className="sc-btn">{t("dashboard.deny")}</span>
              </span>
            </span>
            <span className="sc-line" {...fx("line", 5.6)}>
              {`  $ ${SAMPLE.ask}`}
            </span>
          </Pane>
          <Pane title={t("pane.shellIn", { folder: SAMPLE.workspace })} className="sc-hide-narrow" status={<Dot status="idle" />}>
            <Lines lines={SAMPLE.lines.shell} start={0.3} step={1.3} />
          </Pane>
        </div>

        {radar && (
          <div className="sc-toast sc-toast-top" {...fx("slide", 3.8)}>
            <span className="sc-toast-title">
              {t("radar.notice.title", { a: finished.branch, b: working.branch, symbols: SAMPLE.symbol })}
            </span>
            <span className="sc-toast-body">{t("radar.notice.body")}</span>
            <span className="sc-swap">
              <span {...fx("out", 4.75)}>
                <span className="sc-btn" {...fx("press", 4.5)}>
                  {t("radar.notice.warn")}
                </span>
              </span>
              <span className="sc-status" {...fx("pop", 4.75)}>
                <Dot status="done" /> {t("radar.notice.warned")}
              </span>
            </span>
          </div>
        )}

        <div className="sc-toast sc-toast-bottom" {...fx("slide", 7.2)}>
          {/* The review queue's card (ReviewQueue): the agent's first name, then
              its engine, workspace and branch. */}
          <span className="sc-toast-row">
            <Glyph engine={finished.engine} />
            <span className="sc-card-main">
              <span className="sc-toast-title">{finished.name}</span>
              <span className="sc-card-meta">{[finished.engine, SAMPLE.workspace, finished.branch].join(" · ")}</span>
            </span>
          </span>
          <span className="sc-stat">
            {t("review.stat", { n: SAMPLE.diff.files, adds: 0, dels: 0 }).split(" · ")[0]} ·{" "}
            <span className="sc-add">
              <Count to={SAMPLE.diff.adds} seconds={7.6} prefix="+" />
            </span>{" "}
            <span className="sc-del">
              <Count to={SAMPLE.diff.dels} seconds={7.6} prefix="−" />
            </span>
          </span>
          <span className="sc-chips">
            <span className="sc-chip" {...fx("pop", 8.4)}>
              {t("review.rule.auth")}
            </span>
            <span className="sc-chip sc-chip-ok" {...fx("pop", 8.8)}>
              {t("review.check.passed")}
            </span>
          </span>
        </div>
      </div>
      <StatusBar branch={SAMPLE.base} />
    </AppWindow>
  );
}

/** The waiting agent's status: working, then waiting on its approval, then
 * working again once it's allowed. */
function GeminiStatus() {
  return (
    <span className="sc-dot-swap">
      <span {...fx("out", 2.2)}>
        <Dot status="working" />
      </span>
      <span {...visit(2.2, 3)}>
        <Dot status="waiting" />
      </span>
      <span {...fx("pop", 5.2)}>
        <Dot status="working" />
      </span>
    </span>
  );
}

/** What the scene shows, for people who can't see it (its figure's caption). */
/** The scene's short visible caption. */
export const WORKSPACE_CAPTION = "Four agents in one repository, each in its own worktree, and the one that needs you.";

export function workspaceSceneText(): string {
  const radar = LANDS
    ? ` The intent radar marks ${finished.branch} and ${working.branch}, which both change ${SAMPLE.symbol}, and both agents are warned.`
    : "";
  return `Code mode with four agents in their own worktrees of one repository. ${finished.name}, in ${finished.engine}, fixes checkout and turns Done; ${waiting.name}, in ${waiting.engine}, asks to run its tests, and the request is allowed once.${radar} ${finished.name}'s change arrives in the review queue: ${SAMPLE.diff.files} files, +${SAMPLE.diff.adds} −${SAMPLE.diff.dels}, with its check passed.`;
}
