import type { ReactNode } from "react";
import { NAMES_TOOLS, SAMPLE, hookTool, t } from "../../../lib/scenes";
import { AgentsSegment, AppWindow, Changes, Dot, DotChange, Glyph, StatusBar, fx, type Agents } from "./parts";

const { finished, working, waiting } = SAMPLE.worktrees;

/** When the waiting worktree's agent asks; when the finished one's agent
 * goes from its edits to its tests; and when it's done: the cards, the
 * filter counts and the Review count change then. */
export const DASH_T = { ask: 2, run: 3.2, done: 4.6 } as const;

/** How long an agent has been in its state when the scene opens: the demo
 * starts every pane 42 minutes back (backend.ts's START), and a change of
 * state starts the clock again. */
export const SINCE_START = 42;

/** Each state's word on a card (AgentCard's STATUS_LABEL). */
const STATE_LABEL = { working: "agentStatus.working", waiting: "agentStatus.waiting", done: "agentStatus.done" } as const;

/** An agent's state as its card says it, with how long it has held it
 * (AgentCard's label and its elapsed): "Working · 42m". */
export function cardState(status: keyof typeof STATE_LABEL, minutes: number): string {
  return `${t(STATE_LABEL[status])} · ${t("time.minutes", { n: minutes })}`;
}

/** An agent's card as AgentCard draws it (F-DB-01): the title row (its
 * status, its engine's tile, its first name, and at the right its state and
 * how long it has held it), then cardMeta: its engine and its worktree's
 * name (a worktree's workspace is its own, and its branch, tandem/<name>,
 * is shown as the name, so once: D-1898). Then its line, when it has one:
 * dashboard.rs draws its status's preview, else its tool. A pane's status
 * has no preview, and only one engine's hooks name a tool (NAMES_TOOLS),
 * while its agent works (D-3002, D-3003). The story gives no account, so
 * none shows. */
function Card({ lane, dot, state, line }: { lane: typeof finished; dot: ReactNode; state: ReactNode; line?: ReactNode }) {
  return (
    <span className="sc-card">
      <span className="sc-card-head">
        {dot}
        <Glyph engine={lane.engine} />
        <span className="sc-card-title">{lane.name}</span>
        <span className="sc-card-state">{state}</span>
      </span>
      <span className="sc-card-meta">{[lane.engine, lane.branch].join(" · ")}</span>
      {line !== undefined && <span className="sc-card-line">{line}</span>}
    </span>
  );
}

/** The tool the finished worktree's agent's card names from each time on,
 * while it works: its hooks report the tool of the step it's at, its edits
 * and then its tests (Edit, then Bash). An engine whose hooks name no tool
 * gives a card no line, and a done report clears the tool (D-3003). */
export const DASH_TOOLS: ReadonlyArray<readonly [number, string]> = NAMES_TOOLS.has(finished.engineId)
  ? finished.steps.slice(-2).map((step, i) => [i === 0 ? 0 : DASH_T.run, hookTool(step)] as const)
  : [];
for (const w of [working, waiting])
  if (NAMES_TOOLS.has(w.engineId)) throw new Error(`scenes: ${w.name}'s Dashboard card would name its tool, in ${w.engine}, and the scene draws none`);

/** The finished worktree's agent's card: at work, its line the tool its
 * hooks name; then done, when the app draws the card again with no line.
 * Both cards are in one place, each its own size, and swap at once, so the
 * card loses its line without moving what's under it (D-3003). */
function Finishing() {
  return (
    <span className="sc-card-swap">
      <span {...fx("cut", DASH_T.done)}>
        <Card
          lane={finished}
          dot={<Dot status="working" />}
          state={cardState("working", SINCE_START)}
          line={DASH_TOOLS.length > 0 ? <Changes steps={DASH_TOOLS} /> : undefined}
        />
      </span>
      <span {...fx("show", DASH_T.done)}>
        <Card
          lane={finished}
          dot={<DotChange from="working" to="done" seconds={DASH_T.done} />}
          state={<span {...fx("enter", DASH_T.done)}>{cardState("done", 0)}</span>}
        />
      </span>
    </span>
  );
}

/** The agents waiting on you and at work, across the repository, from each
 * time on: all three at work, then the one that asks waits, then the
 * finished one is done (the status bar's agents segment). */
export const DASH_AGENTS: ReadonlyArray<readonly [number, Agents]> = [
  [0, { waiting: 0, working: SAMPLE.order.length }],
  [DASH_T.ask, { waiting: 1, working: SAMPLE.order.length - 1 }],
  [DASH_T.done, { waiting: 1, working: SAMPLE.order.length - 2 }],
];

/** "Who needs you" (D-1880): the Dashboard over the story's repository,
 * grouped by project, each worktree's agent under its branch, as the app
 * groups a repository's worktrees. The waiting one's engine asks to run
 * its tests: its card turns to Waiting for you; the finished one's card
 * names its tools, then turns Done; the working one stays at work; the
 * filter counts and the status bar follow. About 7 s, played once,
 * resting with the waiting agent on you. */
export function DashboardScene() {
  const n = SAMPLE.order.length;
  const card = (branch: string) => {
    if (branch === waiting.branch)
      return (
        <span className="sc-card-wrap sc-card-ask">
          {/* Its engine asks in its own terminal: no line here, and no
              Vibezeo approval to answer, so no buttons (D-3001, D-3002). */}
          <Card
            lane={waiting}
            dot={<DotChange from="working" to="waiting" seconds={DASH_T.ask} />}
            state={
              <Changes
                steps={[
                  [0, cardState("working", SINCE_START)],
                  [DASH_T.ask, cardState("waiting", 0)],
                ]}
              />
            }
          />
          <span className="sc-ring" {...fx("pop", DASH_T.ask)} />
        </span>
      );
    if (branch === finished.branch) return <Finishing />;
    return <Card lane={working} dot={<Dot status="working" />} state={cardState("working", SINCE_START)} />;
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
      <StatusBar branch={SAMPLE.base} segments={<AgentsSegment steps={DASH_AGENTS} />} />
    </AppWindow>
  );
}

export const DASHBOARD_CAPTION = "Every agent across your workspaces, and the one waiting on you.";

export function dashboardSceneText(): string {
  const tools = DASH_TOOLS.map(([, tool]) => tool);
  const named = tools.length > 0 ? `, and its card names the tool its hooks report until then: ${tools.join(", then ")}` : "";
  return `The Dashboard shows ${SAMPLE.order.length} agents in ${SAMPLE.workspace}, grouped by project, each under its worktree's branch. ${waiting.name}, in ${waiting.engine}, asks in its terminal to run ${SAMPLE.ask}: the card turns to Waiting for you. ${finished.name}, in ${finished.engine}, turns Done${named}. ${working.name}, in ${working.engine}, keeps working. The filter counts change to 1 waiting, 1 working and 1 done.`;
}
