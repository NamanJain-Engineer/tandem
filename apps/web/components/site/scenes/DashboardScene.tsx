import type { ReactNode } from "react";
import { NAMES_TOOLS, SAMPLE, SINCE_START, hookTool, t } from "../../../lib/scenes";
import { AgentsSegment, AppWindow, Changes, Dot, DotChange, Glyph, StatusBar, fx, type Agents } from "./parts";

const { finished, working, waiting } = SAMPLE.worktrees;

/** When the waiting worktree's agent asks; when the finished one's agent
 * goes from its edits to its tests; and when it's done: the cards, the
 * filter counts and the Review count change then. */
export const DASH_T = { ask: 2, run: 3.2, done: 4.6 } as const;

export { SINCE_START };

/** Each state's word on a card (AgentCard's STATUS_LABEL). */
const STATE_LABEL = { working: "agentStatus.working", waiting: "agentStatus.waiting", done: "agentStatus.done" } as const;

/** An agent's state as its card says it, with how long it has held it
 * (AgentCard's label and its elapsed): "Working · 42m". */
export function cardState(status: keyof typeof STATE_LABEL, minutes: number): string {
  return `${t(STATE_LABEL[status])} · ${t("time.minutes", { n: minutes })}`;
}

/** A card's title as cardTitle writes it (model.ts): the agent's first name
 * and its work, its pane's title ("{name} · {work}"), cut to the card's
 * width as AgentCard's is. */
export const cardTitle = (lane: typeof finished) => `${lane.name} · ${lane.title}`;

/** An agent's card as AgentCard draws it (F-DB-01): the title row (its
 * status, its engine's tile, its title, at the right its state and how long
 * it has held it, then the room of its ⋯ and ×, which show on hover), then
 * cardMeta: its engine and its worktree's name (a worktree's workspace is
 * its own, and its branch, tandem/<name>, is shown as the name, so once:
 * D-1898). Then its line, when it has one: dashboard.rs draws its status's
 * preview, else its tool. A pane's status has no preview, and only one
 * engine's hooks name a tool (NAMES_TOOLS), while its agent works (D-3002,
 * D-3003). The story gives no account, so none shows. */
function Card({ lane, dot, state, line }: { lane: typeof finished; dot: ReactNode; state: ReactNode; line?: ReactNode }) {
  return (
    <span className="sc-card">
      <span className="sc-card-head">
        {dot}
        <Glyph engine={lane.engine} />
        <span className="sc-card-title">{cardTitle(lane)}</span>
        <span className="sc-card-state">{state}</span>
        <span className="sc-card-menu" />
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

/** Whether the finished worktree's agent's card has a line until it's done,
 * so the card gets a line shorter then; and where its group is. */
const LINED = DASH_TOOLS.length > 0;
const AT = SAMPLE.order.findIndex((w) => w.branch === finished.branch);

/** The finished worktree's agent's card: at work, its line the tool its
 * hooks name; then done, when the app draws the card again with no line.
 * Both cards are in one place and swap at once (D-3003). The place is the
 * done card's: the card at work hangs over what's below by its line, which
 * waits that much lower and moves up when the line goes, as the app's
 * re-render moves it (D-3004). */
function Finishing() {
  return (
    <span className="sc-card-swap">
      <span className={LINED ? "sc-card-lined" : undefined} {...fx("cut", DASH_T.done)}>
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
        <span className="sc-card-wrap">
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
          {/* AgentCard keeps its surface and draws a waiting agent's
              border in the waiting colour, from when it's drawn again. */}
          <span className="sc-card-waiting" {...fx("show", DASH_T.ask)} />
        </span>
      );
    if (branch === finished.branch) return <Finishing />;
    return <Card lane={working} dot={<Dot status="working" />} state={cardState("working", SINCE_START)} />;
  };
  return (
    <AppWindow className="sc-dashboard">
      <div className="sc-dash">
        {/* DashboardPanel's switch, Agents or Review, over the Dashboard,
            whose header starts with its count. */}
        <span className="sc-seg sc-dash-switch">
          <span className="sc-mode sc-mode-on">{t("review.viewAgents", { n })}</span>
          <span className="sc-mode">
            <Changes steps={[[0, t("review.viewReview", { n: 0 })], [DASH_T.done, t("review.viewReview", { n: 1 })]]} />
          </span>
        </span>
        <div className="sc-dash-head">
          <span className="sc-dash-title">{t("dashboard.count", { n })}</span>
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

        {/* The repository's group, and in it each worktree's, by branch.
            When the finished one's card gets a line shorter, the groups
            below it move up that much, eased (§4.7's move), and its rule's
            tail goes as they do (D-3004). */}
        <div className="sc-group" {...fx("in", 0.2)}>
          <span className="sc-group-head">{`${SAMPLE.workspace} · ${n}`}</span>
          <span className="sc-group-lanes">
            {SAMPLE.order.map((w, i) => (
              <span key={w.branch} className="sc-group" {...(LINED && i > AT ? fx("close", DASH_T.done) : {})}>
                <span className="sc-group-head">{`${w.branch} · 1`}</span>
                {card(w.branch)}
              </span>
            ))}
            {LINED && <span className="sc-lanes-tail" {...fx("close", DASH_T.done)} />}
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
