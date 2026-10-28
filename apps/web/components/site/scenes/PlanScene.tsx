import type { Status } from "@tandem/ui/components/StatusDot";
import type { ReactNode } from "react";
import { SAMPLE, t } from "../../../lib/scenes";
import { AppWindow, Changes, Dot, StatusBar, fx } from "./parts";

/** A task run's states, as the plan's list shows them (F-TK-07). */
type RunState = "queued" | "running" | "reviewing" | "in_review";
type Verdict = "approve" | "changes_requested";
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
 * `reviewing` and is ready to review, with the review's verdict, at `ready`. */
function run(running: number, reviewing: number, ready: number) {
  return [
    { at: 0, state: "queued" },
    { at: running, state: "running" },
    { at: reviewing, state: "reviewing" },
    { at: ready, state: "in_review" },
  ] as const satisfies ReadonlyArray<{ at: number; state: RunState }>;
}

const [balances, redeem, cart] = SAMPLE.plan.tasks;

/** The plan's three runs, in seconds from the scene's start. Execute all
 * queued them together (at most four run at a time); each is reviewed by
 * another engine, which approves two and sends one back. */
export const PLAN_RUNS = [
  { task: balances, verdict: "approve", steps: run(0.9, 3.3, 4.5) },
  { task: redeem, verdict: "approve", steps: run(0.9, 6.1, 7.3) },
  { task: cart, verdict: "changes_requested", steps: run(0.9, 4.8, 5.9) },
] as const satisfies ReadonlyArray<{ task: { id: string; title: string; engine: string }; verdict: Verdict; steps: unknown }>;

type Run = (typeof PLAN_RUNS)[number];

function stateAt(r: Run, seconds: number): RunState {
  let state: RunState = "queued";
  for (const s of r.steps) if (s.at <= seconds) state = s.state;
  return state;
}

function kindOf(state: RunState, verdict: Verdict): Kind {
  if (state === "queued") return "queued";
  if (state === "in_review") return verdict === "approve" ? "approved" : "changes";
  return "running";
}

/** "Ready to review · Approved", "Reviewing": PlanWithLead's runLabel. */
function runLabel(state: RunState, verdict: Verdict): string {
  const label = t(`tasks.runs.state.${state}`);
  if (state !== "in_review") return label;
  return `${label} · ${t(verdict === "approve" ? "plan.verdict.approve" : "plan.verdict.changes")}`;
}

/** The plan at `seconds`: its count line, how many runs are approved (what
 * "Land N approved…" counts) and the header chip's shape, all from the same
 * runs, so the line and the button always agree. */
export function planAt(seconds: number): { summary: string; approved: number; chip: Status } {
  const kinds = PLAN_RUNS.map((r) => kindOf(stateAt(r, seconds), r.verdict));
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

const dots = (steps: Array<readonly [number, Status]>) =>
  steps.map(([at, status]) => [at, <Dot key={at} status={status} />] as const);

/** "Follow a plan through" (D-1888, 0.12.0's F-TK-07): Plan with Lead's
 * dialog on the Tasks page after Execute all. The three runs go from
 * queued to running, through their independent review, to Approved or
 * Changes requested; the count line follows them, and "Land 2 approved…"
 * ends ready to land the approved work in the plan's order. About 8 s,
 * played once. */
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
        <div className="sc-plan-layer">
          <div className="sc-dialog sc-plan-dialog" {...fx("rise", 0.2)}>
            <span className="sc-dash-title">{t("plan.titleOf", { goal })}</span>
            <span className="sc-plan-line">
              <Dot status="done" /> {t("plan.started", { n: tasks.length })}
            </span>
            <span className="sc-plan-summary">
              <Changes steps={over((at) => planAt(at).summary)} />
            </span>
            <span className="sc-runs">
              {PLAN_RUNS.map((r) => (
                <span key={r.task.id} className="sc-run">
                  <Changes steps={dots(over((at) => SHAPE[kindOf(stateAt(r, at), r.verdict)]))} />
                  <span className="sc-run-title">
                    <span className="sc-mono sc-run-id">{r.task.id}</span> {r.task.title}
                  </span>
                  <span className="sc-run-label">
                    <Changes steps={r.steps.map((s) => [s.at, `${r.task.engine} · ${runLabel(s.state, r.verdict)}`] as const)} />
                  </span>
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
      </div>
      <StatusBar branch={SAMPLE.base} />
    </AppWindow>
  );
}

export const PLAN_CAPTION = "A plan's tasks run in their own worktrees, each reviewed by another engine, and the approved two are ready to land.";

export function planSceneText(): string {
  const { goal, tasks } = SAMPLE.plan;
  const [a, b, c] = tasks;
  return `Plan with Lead follows the plan "${goal}" through. Its three tasks run at once, each in its own worktree: ${a.engine} adds gift card balances, ${b.engine} redeems them at checkout and ${c.engine} shows them in the cart. Another engine reviews each run: two come back Approved and one Changes requested, the plan's line reads 2 approved and 1 changes requested, and "Land 2 approved…" is ready to land the approved two in the plan's order.`;
}
