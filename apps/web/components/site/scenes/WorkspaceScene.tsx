import { LANDS, SAMPLE, SCENE_BUILD, t } from "../../../lib/scenes";
import { AppWindow, Count, Dot, DotChange, Glyph, Lines, Pane, fx, visit } from "./parts";

const { claude, codex, gemini } = SAMPLE.worktrees;

/** The hero's scene (D-1880): Code mode over one repository, four live panes,
 * each agent in its own worktree. Claude Code fixes
 * checkout and finishes; Gemini CLI asks before running its tests and the
 * approval is answered; on 0.9.0 the intent radar sees Claude Code and Codex
 * change the same function and both are warned; the finished work arrives
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
            <DotChange from="working" to="done" seconds={6.4} />
            <span className="sc-lane-name">{claude.branch}</span>
            {radar && (
              <span className="sc-radar" {...fx("pop", 3.4)}>
                {t("radar.mark.one", { symbol: SAMPLE.symbol })}
              </span>
            )}
          </span>
          <span className="sc-lane">
            <Dot status="working" />
            <span className="sc-lane-name">{codex.branch}</span>
            {radar && (
              <span className="sc-radar" {...fx("pop", 3.5)}>
                {t("radar.mark.one", { symbol: SAMPLE.symbol })}
              </span>
            )}
          </span>
          <span className="sc-lane">
            <GeminiStatus />
            <span className="sc-lane-name">{gemini.branch}</span>
          </span>
          <span className="sc-rail-foot">{t("rail.live", { n: 4 })}</span>
        </aside>

        <div className="sc-grid">
          <Pane
            engine={claude.engine}
            title={t("pane.engine", { engine: claude.engine, n: 1 })}
            branch={claude.branch}
            focused
            doneAt={6.4}
            status={<DotChange from="working" to="done" seconds={6.4} />}
          >
            <Lines lines={SAMPLE.lines.claude} start={0.5} step={0.8} />
          </Pane>
          <Pane engine={codex.engine} title={t("pane.engine", { engine: codex.engine, n: 2 })} branch={codex.branch} status={<Dot status="working" />}>
            <Lines lines={SAMPLE.lines.codex} start={0.9} step={1} />
            <span className="sc-line sc-caret" {...fx("line", 5.9)} />
          </Pane>
          <Pane
            engine={gemini.engine}
            title={t("pane.engine", { engine: gemini.engine, n: 3 })}
            branch={gemini.branch}
            className="sc-hide-narrow"
            status={
              <GeminiStatus />
            }
          >
            <Lines lines={SAMPLE.lines.gemini} start={0.7} step={0.7} />
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
          <Pane title={t("pane.shell", { n: 1 })} className="sc-hide-narrow" status={<Dot status="idle" />}>
            <Lines lines={SAMPLE.lines.shell} start={0.3} step={1.3} />
          </Pane>
        </div>

        {radar && (
          <div className="sc-toast sc-toast-top" {...fx("slide", 3.8)}>
            <span className="sc-toast-title">
              {t("radar.notice.title", { a: claude.branch, b: codex.branch, symbols: SAMPLE.symbol })}
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
          <span className="sc-toast-row">
            <Glyph engine={claude.engine} />
            <span className="sc-toast-title">{t("review.diffTitle", { name: claude.engine })}</span>
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
      <div className="sc-statusbar">
        <span>{t("status.branch", { name: SAMPLE.base, counts: "" }).split(",")[0]}</span>
        <span className="sc-statusbar-right">
          <span>{t("status.online")}</span>
          <span>{t("status.version", { version: SCENE_BUILD })}</span>
        </span>
      </div>
    </AppWindow>
  );
}

/** Gemini CLI's status: working, then waiting on its approval, then
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
    ? ` The intent radar marks ${claude.branch} and ${codex.branch}, which both change ${SAMPLE.symbol}, and both agents are warned.`
    : "";
  return `Code mode with four agents in their own worktrees of one repository. Claude Code fixes checkout and turns Done; Gemini CLI asks to run its tests, and the request is allowed once.${radar} Claude Code's change arrives in the review queue: ${SAMPLE.diff.files} files, +${SAMPLE.diff.adds} −${SAMPLE.diff.dels}, with its check passed.`;
}
