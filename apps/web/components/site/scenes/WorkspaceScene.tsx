import { LANDS, SAMPLE, t } from "../../../lib/scenes";
import { AppWindow, Changes, Count, Dot, DotChange, Glyph, Lines, Pane, StatusBar, fx } from "./parts";

const { finished, working, waiting } = SAMPLE.worktrees;

/** When the finished worktree's agent is done: its dot, its rail row and its
 * pane's header all change then. */
export const HERO_DONE = 6.4;
/** When the waiting worktree's engine asks to run its tests: it waits on
 * you from then on, with its question in its pane (D-3001). */
export const HERO_ASK = 2.2;

/** The hero's scene (D-1880): Code mode over one repository, four live
 * panes, each agent in its own worktree, in the states story.json gives
 * them. The finished one fixes checkout; the waiting one's engine asks in
 * its terminal before running its tests and waits on you; the working one
 * stays at work. On 0.9.0 the intent radar sees the finished and the
 * working one change the same function and both are warned; the finished
 * work arrives in the review queue with its diff counting up. About 11 s,
 * played once, resting on the last frame. */
export function WorkspaceScene() {
  const radar = LANDS;
  return (
    <AppWindow className="sc-workspace">
      <div className="sc-body">
        <aside className="sc-rail">
          <span className="sc-rail-head">{t("rail.workspacesHeader")}</span>
          <span className="sc-ws">
            <span className="sc-ws-name">{SAMPLE.workspace}</span>
            {/* Its agents at work: all three, then less the one that asks and the one that finishes. */}
            <span className="sc-ws-count">
              <Dot status="working" />
              <Changes
                steps={[
                  [0, String(SAMPLE.order.length)],
                  [HERO_ASK, String(SAMPLE.order.length - 1)],
                  [HERO_DONE, String(SAMPLE.order.length - 2)],
                ]}
              />
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
            <DotChange from="working" to="waiting" seconds={HERO_ASK} />
            <span className="sc-lane-name">{waiting.branch}</span>
          </span>
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
            status={<DotChange from="working" to="waiting" seconds={HERO_ASK} />}
          >
            <Lines lines={SAMPLE.lines.waiting} start={0.7} end={HERO_ASK - 0.1} />
            {/* Its engine asks in its own terminal and waits for you: the
                engine's question, which Vibezeo leaves to you (D-3001). */}
            <span className="sc-line sc-line-ask" {...fx("line", HERO_ASK)}>
              {SAMPLE.question.terminal}
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
          {/* The review queue's card (ReviewQueue), as the Review scene's: the
              agent's first name, then its engine, its worktree's name (its
              workspace and branch, shown once: D-1898) and how long ago it
              finished, which is just now. */}
          <span className="sc-toast-row">
            <Glyph engine={finished.engine} />
            <span className="sc-card-main">
              <span className="sc-toast-title">{finished.name}</span>
              <span className="sc-card-meta">{[finished.engine, finished.branch, t("time.minutes", { n: 0 })].join(" · ")}</span>
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

/** The scene's short visible caption. */
export const WORKSPACE_CAPTION = "Three agents in one repository, each in its own worktree, and the one that needs you.";

/** What the scene shows, for people who can't see it (its figure's caption). */
export function workspaceSceneText(): string {
  const radar = LANDS
    ? ` The intent radar marks ${finished.branch} and ${working.branch}, which both change ${SAMPLE.symbol}, and both agents are warned.`
    : "";
  return `Code mode with ${SAMPLE.order.length} agents in their own worktrees of one repository, and a shell. ${finished.name}, in ${finished.engine}, fixes checkout and turns Done; ${working.name}, in ${working.engine}, keeps working; ${waiting.name}, in ${waiting.engine}, asks in its terminal to run ${SAMPLE.ask} and waits for you.${radar} ${finished.name}'s change arrives in the review queue: ${SAMPLE.diff.files} files, +${SAMPLE.diff.adds} −${SAMPLE.diff.dels}, with its check passed.`;
}
