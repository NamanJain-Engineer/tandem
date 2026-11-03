import type { Status } from "@tandem/ui/components/StatusDot";
import type { ReactNode } from "react";
import { SAMPLE, SENDS_BACK, t } from "../../../lib/scenes";
import { AgentsSegment, AppWindow, Changes, Dot, StatusBar, fx, type Agents } from "./parts";

/** A task run's states, as the plan's list shows them (F-TK-07). */
type RunState = "queued" | "running" | "reviewing" | "in_review";
type Verdict = "approve" | "changes_requested";
/** A run from a time on: its state and its review's verdict, as an attempt
 * carries them (`state`, `reviewVerdict`), which PlanWithLead's runLabel and
 * kindOf read. */
type Step = { at: number; state: RunState; verdict: Verdict | null };
/** How a run counts in the plan's line, in the line's order, and the
 * StatusDot shape for each (PlanWithLead's KINDS and SHAPE). */
type Kind = "queued" | "running" | "approved" | "changes";
const KINDS: readonly Kind[] = ["queued", "running", "approved", "changes"];
const SHAPE: Record<Kind, Status> = { queued: "idle", running: "working", approved: "waiting", changes: "waiting" };
const COUNT = {
  queued: "plan.count.queued",
  running: "plan.count.running",
  approved: "plan.count.approved",
  changes: "plan.count.changes",
} as const;

/** A run that starts at `running`, is handed to its independent review at
 * `reviewing` and is ready to review, with the review's `verdict`, at `ready`. */
function run(running: number, reviewing: number, ready: number, verdict: Verdict): Step[] {
  return [
    { at: 0, state: "queued", verdict: null },
    { at: running, state: "running", verdict: null },
    { at: reviewing, state: "reviewing", verdict: null },
    { at: ready, state: "in_review", verdict },
  ];
}

/** A run whose review asks for changes and goes back to it as its next
 * message (F-TK-09). The verdict is written at `asks` while the run is still
 * Reviewing (tasks_exec.rs records it first), so its row says Reviewing ·
 * Changes requested; at `back` the plan sends the review back, its rounds
 * line shows, and the run is Running again with no verdict. It's reviewed
 * again at `again` and is ready to review, approved, at `ready`: never Ready
 * to review in between. */
function sentBack(running: number, reviewing: number, asks: number, back: number, again: number, ready: number): Step[] {
  return [
    { at: 0, state: "queued", verdict: null },
    { at: running, state: "running", verdict: null },
    { at: reviewing, state: "reviewing", verdict: null },
    { at: asks, state: "reviewing", verdict: "changes_requested" },
    { at: back, state: "running", verdict: null },
    { at: again, state: "reviewing", verdict: null },
    { at: ready, state: "in_review", verdict: "approve" },
  ];
}

const [balances, redeem, cart] = SAMPLE.plan.tasks;
if (!balances || !redeem || !cart || SAMPLE.plan.tasks.length !== 3) throw new Error("scenes: the plan scene runs story.json's plan, which needs three tasks");

/** The plan's three runs, in seconds from the scene's start. Execute all
 * queued them together (at most four run at a time); each is reviewed by
 * another engine, which approves two and asks for changes on the third.
 * From 0.14.0 that review goes back to its run (`sentBack`, when its rounds
 * line shows), and the run's next turn is approved. The send-back has a
 * moment of its own: the request holds 0.4 s first, and each change after
 * it comes half a second after the one before, so the second run's review
 * starts later than 0.13.0's. Before 0.14.0 the review waited for you at
 * Changes requested. */
export const PLAN_RUNS: ReadonlyArray<{
  task: { id: string; title: string; engine: string };
  steps: readonly Step[];
  sentBack: number | null;
}> = SENDS_BACK
  ? [
      { task: balances, steps: run(0.9, 3.3, 4.5, "approve"), sentBack: null },
      { task: redeem, steps: run(0.9, 6.8, 7.8, "approve"), sentBack: null },
      { task: cart, steps: sentBack(0.9, 4.8, 5.9, 6.3, 7.3, 8.3), sentBack: 6.3 },
    ]
  : [
      { task: balances, steps: run(0.9, 3.3, 4.5, "approve"), sentBack: null },
      { task: redeem, steps: run(0.9, 6.1, 7.3, "approve"), sentBack: null },
      { task: cart, steps: run(0.9, 4.8, 5.9, "changes_requested"), sentBack: null },
    ];

type Run = (typeof PLAN_RUNS)[number];

function stepAt(r: Run, seconds: number): Step {
  let step = r.steps[0] as Step;
  for (const s of r.steps) if (s.at <= seconds) step = s;
  return step;
}

function kindOf(s: Step): Kind {
  if (s.state === "queued") return "queued";
  if (s.state === "in_review") return s.verdict === "approve" ? "approved" : "changes";
  return "running";
}

/** "Ready to review · Approved", "Reviewing · Changes requested",
 * "Running": PlanWithLead's runLabel, which adds a run's verdict to its
 * state while it has one. */
function runLabel(s: Step): string {
  const label = t(`tasks.runs.state.${s.state}`);
  if (s.verdict === null) return label;
  return `${label} · ${t(s.verdict === "approve" ? "plan.verdict.approve" : "plan.verdict.changes")}`;
}

/** The plan at `seconds`: its count line, how many runs are approved (what
 * "Land N approved…" counts) and the header chip's shape, all from the same
 * runs, so the line and the button always agree. */
export function planAt(seconds: number): { summary: string; approved: number; chip: Status } {
  const kinds = PLAN_RUNS.map((r) => kindOf(stepAt(r, seconds)));
  const count = (k: Kind) => kinds.filter((x) => x === k).length;
  return {
    summary: KINDS.filter((k) => count(k) > 0)
      .map((k) => t(COUNT[k], { n: count(k) }))
      .join(" · "),
    approved: count("approved"),
    // Working while a run is queued or going, then waiting on you (startedStatus).
    chip: kinds.some((k) => k === "queued" || k === "running") ? "working" : "waiting",
  };
}

/** Every moment something in the plan changes. */
export function planTimes(): number[] {
  return [...new Set(PLAN_RUNS.flatMap((r) => r.steps.map((s) => s.at)))].sort((a, b) => a - b);
}

/** A value's changes over the scene, from `value` at each moment. */
function over<T>(value: (seconds: number) => T): Array<readonly [number, T]> {
  const out: Array<readonly [number, T]> = [];
  for (const at of planTimes()) {
    const v = value(at);
    if (out.at(-1)?.[1] !== v) out.push([at, v]);
  }
  return out;
}

/** The agents waiting and at work from each time on, as the status bar
 * counts them: the story's (one waits, one works), and a plan's runs, which
 * are threads, as their reviews are, whose status the app counts as it does
 * a pane's. So it's the story's, and the runs running or being reviewed,
 * which the plan's line counts as running (D-3003, D-3004). */
export const PLAN_AGENTS: ReadonlyArray<readonly [number, Agents]> = over((at) => PLAN_RUNS.filter((r) => kindOf(stepAt(r, at)) === "running").length).map(
  ([at, running]) => [at, { ...SAMPLE.agents, working: SAMPLE.agents.working + running }] as const,
);

const dots = (steps: Array<readonly [number, Status]>) =>
  steps.map(([at, status]) => [at, <Dot key={at} status={status} />] as const);

/** "Follow a plan through" (D-1888, 0.12.0's F-TK-07): Plan with Lead's
 * dialog on the Tasks page after Execute all. The three runs go from
 * queued to running, through their independent review, to Approved or
 * Changes requested (from 0.14.0, with the dialog's Send reviews back
 * switch on, a Changes requested goes back to its run, which is reviewed
 * again and approved); the count line follows them, and "Land N approved…"
 * ends ready to land the approved work in the plan's order. About 8 s (8.5
 * from 0.14.0), played once. */
export function PlanScene() {
  const { goal, tasks } = SAMPLE.plan;
  const lands = over((at) => planAt(at).approved);
  const firstApproved = lands.find(([, n]) => n > 0)?.[0] ?? 0;
  const land: ReadonlyArray<readonly [number, ReactNode]> = lands.map(([at, n]) => [
    at,
    n === 0 ? <span key={n} className="sc-btn-none" /> : <span key={n} className="sc-btn sc-btn-primary">{t("plan.land", { n })}</span>,
  ]);
  return (
    <AppWindow mode="agent" className="sc-plan">
      <div className="sc-plan-body">
        <div className="sc-tasks-head">
          <span className="sc-tasks-title">{t("tasks.title")}</span>
          <span className="sc-plan-chip">
            <Changes steps={dots(over((at) => planAt(at).chip))} />
            <span className="sc-plan-chip-goal">{t("plan.chip", { goal })}</span>
          </span>
          <span className="sc-btn sc-hide-narrow">{t("plan.open")}</span>
          <span className="sc-btn sc-btn-ghost sc-hide-narrow">{t("tasks.refresh")}</span>
        </div>
        <div className="sc-tasks-add">
          <span className="sc-input sc-tasks-input">{t("tasks.quickAddPlaceholder")}</span>
          <span className="sc-check sc-hide-narrow">
            <span className="sc-checkbox" />
            {t("tasks.showArchived")}
          </span>
        </div>
        <span className="sc-tasks-list">
          {tasks.map((x) => (
            <span key={x.id} className="sc-task-row">
              <span className="sc-mono sc-task-id">{x.id}</span>
              <span className="sc-task-title">{x.title}</span>
            </span>
          ))}
        </span>
      </div>
      <StatusBar branch={SAMPLE.base} segments={<AgentsSegment steps={PLAN_AGENTS} />} />
      {/* Over the whole window, as the app's overlay is (fixed, inset 0). */}
      <div className="sc-plan-layer">
        <div className="sc-dialog sc-plan-dialog" {...fx("rise", 0.2)}>
          <span className="sc-dash-title">{t("plan.titleOf", { goal })}</span>
          <span className="sc-plan-line">
            <Dot status="done" /> {t("plan.started", { n: tasks.length })}
          </span>
          {SENDS_BACK && (
            // The switch that sends a review back (SendBackSwitch), under
            // that line as the started dialog draws it: the app's Switch
            // after its label, on, as Execute all leaves it by default, and
            // its hint under it. The scene leaves out Lead's switch above
            // it (SuperviseSwitch), as the 0.13.0 rendering always has: no
            // question comes up here for Lead to answer, its hint would
            // double the settings above the runs, and a phone's frame has
            // no room for that hint.
            <span className="sc-plan-switch">
              <span className="sc-switch-row">
                <span className="sc-switch-label">{t("plan.sendBack.switch")}</span>
                <span className="sc-switch">
                  <span className="sc-switch-thumb" />
                </span>
              </span>
              <span className="sc-plan-switch-hint">{t("plan.sendBack.switchHint")}</span>
            </span>
          )}
          <span className="sc-plan-summary">
            <Changes steps={over((at) => planAt(at).summary)} />
          </span>
          <span className="sc-runs">
            {PLAN_RUNS.map((r) => (
              <span key={r.task.id} className="sc-run">
                <Changes steps={dots(over((at) => SHAPE[kindOf(stepAt(r, at))]))} />
                <span className="sc-run-title">
                  <span className="sc-mono sc-run-id">{r.task.id}</span> {r.task.title}
                </span>
                <span className="sc-run-label">
                  <Changes steps={r.steps.map((s) => [s.at, `${r.task.engine} · ${runLabel(s)}`] as const)} />
                </span>
                {r.sentBack !== null && (
                  // PlanWithLead's rounds line under the row (plan-run-rounds),
                  // from when the review goes back.
                  <span className="sc-run-note" {...fx("enter", r.sentBack)}>
                    {t("plan.review.rounds", { n: 1 })}
                  </span>
                )}
              </span>
            ))}
          </span>
          <span className="sc-dialog-foot sc-plan-foot">
            <span className="sc-btn sc-btn-ghost">{t("plan.removePlan")}</span>
            <Changes
              steps={[
                [0, <span key="primary" className="sc-btn sc-btn-primary">{t("common.close")}</span>],
                [firstApproved, <span key="ghost" className="sc-btn sc-btn-ghost">{t("common.close")}</span>],
              ]}
            />
            <Changes steps={land} />
          </span>
        </div>
      </div>
    </AppWindow>
  );
}

/** A count as the caption and the text say it. */
const word = (n: number) => ["none", "one", "two", "three"][n] ?? String(n);

/** How the plan ends, as the scene's last frame draws it. */
const END = planAt(Infinity);

export const PLAN_CAPTION = SENDS_BACK
  ? `A plan's tasks run in their own worktrees, each reviewed by another engine; from 0.14.0, a review that asks for changes goes back to its run, and all ${word(END.approved)} are ready to land.`
  : `A plan's tasks run in their own worktrees, each reviewed by another engine, and the approved ${word(END.approved)} are ready to land.`;

/** What happens in the scene, for people who can't see it: its numbers from
 * the runs, and from 0.14.0 each review's verdict in the order it comes. */
export function planSceneText(): string {
  const { goal, tasks } = SAMPLE.plan;
  const [a, b, c] = tasks;
  const intro = `Plan with Lead follows the plan "${goal}" through. Its three tasks run at once, each in its own worktree: ${a.engine} adds gift card balances, ${b.engine} redeems them at checkout and ${c.engine} shows them in the cart.`;
  const land = t("plan.land", { n: END.approved });
  if (!SENDS_BACK) {
    const changes = PLAN_RUNS.filter((r) => r.steps.at(-1)?.verdict === "changes_requested").length;
    return `${intro} Another engine reviews each run: ${word(END.approved)} come back Approved and ${word(changes)} Changes requested, the plan's line reads ${END.summary.split(" · ").join(" and ")}, and "${land}" is ready to land the approved ${word(END.approved)} in the plan's order.`;
  }
  const verdicts = PLAN_RUNS.flatMap((r) => r.steps.flatMap((s) => (s.verdict === null ? [] : [{ r, s, verdict: s.verdict }])))
    .sort((x, y) => x.s.at - y.s.at)
    .map(({ r, s, verdict }) => {
      const engine = r.task.engine;
      if (verdict === "changes_requested")
        return r.sentBack === null
          ? `The review of ${engine}'s run asks for changes.`
          : `The review of ${engine}'s run asks for changes, so from 0.14.0 it goes back to that run as its next message, and its row says "${t("plan.review.rounds", { n: 1 })}".`;
      return r.sentBack !== null && s.at > r.sentBack ? `${engine}'s run fixes its work and is reviewed again: Approved.` : `${engine}'s run comes back Approved.`;
    });
  return `${intro} The dialog's switch "${t("plan.sendBack.switch")}" is on, as it is by default. Another engine reviews each run. ${verdicts.join(" ")} The plan's line reads ${END.summary}, and "${land}" is ready to land all ${word(END.approved)} in the plan's order.`;
}
