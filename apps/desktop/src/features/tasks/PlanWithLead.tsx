// F-TK-06 Plan with Lead: a goal goes to Lead, which plans in its own chat
// in the workspace; its plan comes back as editable rows (title,
// description, engine). Create tasks writes them; Execute all shows each
// run first, then runs them through F-TK-04's queue (D-2430). F-TK-07: the
// plan then follows each task's latest run and lands the approved ones
// through Land worktrees (F-WT-07).
import { useEffect, useState } from "react";
import { X } from "lucide-react";
import type { AttemptView, PlanEngine, PlanLaunch, PlanTask, PlanView, TaskRunBlock } from "@tandem/protocol";
import {
  Button,
  Callout,
  Dialog,
  IconButton,
  Input,
  Select,
  StatusDot,
  Textarea,
  useToast,
} from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { switchMode } from "../shell/actions";
import { usePlans } from "../../stores/plan";
import { isIpcError, useTasks } from "../../stores/tasks";
import { useTeammates } from "../../stores/teammates";
import { useWorkspaces } from "../../stores/workspaces";
import { useWorktrees } from "../../stores/worktrees";

export const GOAL_MAX = 4_000;

/** A plan's Lead chat runs in a folder on this computer. */
const REMOTE = new Set(["ssh", "wsl"]);

/** "Claude Code · 10 % used" for the engine menu. */
export function engineOption(e: PlanEngine): { value: string; label: string } {
  const used = e.accounts.map((a) => a.usedPct).filter((p): p is number => typeof p === "number");
  const label =
    used.length > 0 ? t("plan.engineUsed", { name: e.label, pct: Math.round(Math.min(...used)) }) : e.label;
  return { value: e.engineId, label };
}

/** A task's latest run: the attempt queued last (F-TK-07). */
export function latestRun(attempts: AttemptView[], issueId: string): AttemptView | null {
  let best: AttemptView | null = null;
  for (const a of attempts) {
    if (a.attempt.issueId === issueId && (!best || a.attempt.queuedAt > best.attempt.queuedAt)) best = a;
  }
  return best;
}

/** How a run counts in the plan's line, in the line's order. */
const KINDS = [
  "queued",
  "running",
  "approved",
  "changes",
  "review",
  "stopped",
  "failed",
  "done",
  "none",
] as const;
type Kind = (typeof KINDS)[number];

function kindOf(run: AttemptView | null): Kind {
  switch (run?.attempt.state) {
    case undefined:
      return "none";
    case "queued":
      return "queued";
    case "setup":
    case "running":
    case "reviewing":
      return "running";
    case "in_review":
      return run.attempt.reviewVerdict === "approve"
        ? "approved"
        : run.attempt.reviewVerdict === "changes_requested"
          ? "changes"
          : "review";
    case "stopped":
      return "stopped";
    case "failed":
      return "failed";
    case "done":
      return "done";
  }
}

const SHAPE = {
  queued: "idle",
  running: "working",
  approved: "waiting",
  changes: "waiting",
  review: "waiting",
  stopped: "idle",
  failed: "error",
  done: "done",
  none: "idle",
} as const;

/** "Ready to review · Approved", "Queued (position 2)", "Not started". */
export function runLabel(run: AttemptView | null): string {
  if (!run) return t("plan.run.none");
  const a = run.attempt;
  if (a.state === "queued" && run.queuePosition) return t("tasks.runs.queued", { n: run.queuePosition });
  const state = t(`tasks.runs.state.${a.state}` as MessageKey);
  if (a.reviewVerdict === "approve") return `${state} · ${t("plan.verdict.approve")}`;
  if (a.reviewVerdict === "changes_requested") return `${state} · ${t("plan.verdict.changes")}`;
  return state;
}

/** "1 running · 1 approved · 1 not started": the kinds there are. */
export function runSummary(runs: Array<AttemptView | null>): string {
  const counts = new Map<Kind, number>();
  for (const r of runs) counts.set(kindOf(r), (counts.get(kindOf(r)) ?? 0) + 1);
  return KINDS.filter((k) => counts.has(k))
    .map((k) => t(`plan.count.${k}` as MessageKey, { n: counts.get(k) ?? 0 }))
    .join(" · ");
}

/** The worktrees of the plan's tasks whose latest run was approved, in
 * plan order (F-TK-07 AC4). */
export function approvedLanes(plan: PlanView, attempts: AttemptView[]): string[] {
  return plan.issueIds
    .map((id) => latestRun(attempts, id))
    .filter((r): r is AttemptView => kindOf(r) === "approved" && !!r?.attempt.laneWorkspaceId)
    .map((r) => r.attempt.laneWorkspaceId ?? "");
}

function PlanRuns({ plan, attempts }: { plan: PlanView; attempts: AttemptView[] }) {
  const runs = plan.issueIds.map((id) => latestRun(attempts, id));
  const engine = (id: string) => plan.engines.find((e) => e.engineId === id)?.label ?? id;
  const openTask = (id: string) => {
    usePlans.getState().open(null);
    void useTasks.getState().select(id);
  };
  return (
    <>
      {runs.some(Boolean) && (
        <p className="text-sm text-fg-muted tabular-nums" data-testid="plan-summary">
          {runSummary(runs)}
        </p>
      )}
      <ol className="flex flex-col gap-1" data-testid="plan-runs">
        {plan.issueIds.map((id, i) => {
          const run = runs[i] ?? null;
          const task = plan.tasks[i];
          const kind = kindOf(run);
          return (
            <li
              key={id}
              className="flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-surface-2"
              data-testid="plan-run"
              data-state={run?.attempt.state ?? "none"}
              data-verdict={run?.attempt.reviewVerdict ?? undefined}
            >
              <StatusDot status={SHAPE[kind]} label={runLabel(run)} />
              <button
                type="button"
                className="min-w-0 flex-1 truncate text-left text-sm text-fg hover:underline"
                onClick={() => openTask(id)}
                data-testid="plan-run-open"
              >
                <span className="font-mono text-fg-muted">{id}</span> {task?.title ?? ""}
              </button>
              <span className="shrink-0 text-sm text-fg-muted">
                {engine(run?.attempt.engineId ?? task?.engineId ?? "")} · {runLabel(run)}
              </span>
            </li>
          );
        })}
      </ol>
    </>
  );
}

/** Why Execute all leaves a task alone. */
const SKIP: Record<TaskRunBlock, MessageKey> = {
  running: "plan.skip.running",
  archived: "plan.skip.archived",
  finished: "plan.skip.finished",
  not_to_do: "plan.skip.not_to_do",
  missing: "plan.skip.missing",
};

export function openLeadThread(plan: PlanView): void {
  switchMode("agent");
  const teammates = useTeammates.getState();
  teammates.select(plan.leadId);
  teammates.selectChat(plan.leadId, plan.threadId);
  usePlans.getState().open(null);
}

function GoalForm({ onClose }: { onClose: () => void }) {
  const folders = useWorkspaces((s) => s.list).filter((w) => !w.parentId && !w.missing);
  const local = folders.filter((w) => !REMOTE.has(w.kind));
  const [workspace, setWorkspace] = useState("");
  const [goal, setGoal] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ws = workspace || local[0]?.id || "";
  const ok = !!ws && goal.trim().length > 0 && goal.length <= GOAL_MAX && !busy;
  const start = async () => {
    setBusy(true);
    setError(null);
    const r = await usePlans.getState().start(ws, goal);
    setBusy(false);
    if (isIpcError(r)) setError(r.message);
  };
  return (
    <Dialog
      open
      size="lg"
      onOpenChange={(o) => !o && onClose()}
      title={t("plan.title")}
      description={t("plan.lead")}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button variant="primary" disabled={!ok} onClick={() => void start()} data-testid="plan-start">
            {t("plan.start")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3" data-testid="plan-goal-form">
        <Textarea
          label={t("plan.goal")}
          minRows={4}
          value={goal}
          placeholder={t("plan.goalPlaceholder")}
          error={goal.length > GOAL_MAX ? t("plan.goalTooLong", { n: GOAL_MAX }) : undefined}
          onChange={(e) => setGoal(e.target.value)}
          data-testid="plan-goal"
        />
        <Select
          label={t("plan.workspace")}
          value={ws}
          options={local.map((w) => ({ value: w.id, label: w.name }))}
          placeholder={t("tasks.exec.noWorkspace")}
          onValueChange={setWorkspace}
        />
        {folders.length > local.length && <p className="text-sm text-fg-muted">{t("plan.localOnly")}</p>}
        {error && (
          <p className="text-sm text-error" role="alert" data-testid="plan-error">
            {error}
          </p>
        )}
      </div>
    </Dialog>
  );
}

function Row({
  task,
  index,
  engines,
  canRemove,
  createdAs,
  onChange,
  onRemove,
}: {
  task: PlanTask;
  index: number;
  engines: PlanEngine[];
  canRemove: boolean;
  /** Written before Create tasks stopped: it stays as it is. */
  createdAs?: string;
  onChange: (t: PlanTask) => void;
  onRemove: () => void;
}) {
  const locked = createdAs !== undefined;
  return (
    <li
      className="flex flex-col gap-2 rounded-md border border-border bg-surface-2 p-3"
      data-testid="plan-row"
    >
      <div className="flex items-start gap-2">
        <Input
          className="flex-1"
          label={t("plan.taskTitle", { n: index + 1 })}
          value={task.title}
          maxLength={200}
          disabled={locked}
          onChange={(e) => onChange({ ...task, title: e.target.value })}
          data-testid="plan-row-title"
        />
        <Select
          className="w-56"
          label={t("plan.engine")}
          value={task.engineId}
          options={engines.map(engineOption)}
          disabled={locked}
          onValueChange={(v) => onChange({ ...task, engineId: v, fellBack: false })}
        />
        {canRemove && !locked && (
          <IconButton className="mt-6" icon={<X size={14} />} label={t("plan.remove")} onClick={onRemove} />
        )}
      </div>
      <Textarea
        label={t("plan.description")}
        hideLabel
        minRows={2}
        maxRows={6}
        value={task.description}
        placeholder={t("plan.descriptionPlaceholder")}
        disabled={locked}
        onChange={(e) => onChange({ ...task, description: e.target.value })}
      />
      {createdAs !== undefined ? (
        <p className="flex items-center gap-2 text-sm text-fg-muted" data-testid="plan-row-created">
          <StatusDot status="done" label={t("plan.createdAs", { id: createdAs })} />
          {t("plan.createdAs", { id: createdAs })}
        </p>
      ) : task.fellBack ? (
        <p className="text-sm text-fg-muted" data-testid="plan-row-fell-back">
          {t("plan.fellBack")}
        </p>
      ) : (
        task.why && <p className="text-sm text-fg-subtle">{task.why}</p>
      )}
    </li>
  );
}

/** Execute all's check: every run it starts, before any does (F-TK-06 AC4). */
function LaunchList({ launch }: { launch: PlanLaunch }) {
  const runnable = launch.runs.filter((r) => !r.skip).length;
  return (
    <div className="flex flex-col gap-3" data-testid="plan-launch">
      {launch.blocked ? (
        <Callout tone="info">
          <span data-testid="plan-execute-blocked">
            {launch.blocked} {t("plan.tasksStay")}
          </span>
        </Callout>
      ) : (
        <p className="text-sm text-fg-muted">
          {runnable > 0 ? t("plan.confirm.lead") : t("plan.confirm.none")}
        </p>
      )}
      <ol className="flex flex-col gap-2">
        {launch.runs.map((r) => (
          <li
            key={r.issueId}
            className="flex flex-col gap-1 rounded-md border border-border bg-surface-2 p-3"
            data-testid="plan-launch-run"
            data-skip={r.skip ?? ""}
          >
            <span className="text-sm text-fg">
              {t("plan.confirm.task", { id: r.issueId, title: r.title })}
            </span>
            <span className="text-sm text-fg-muted">
              {t("plan.confirm.engine", {
                engine: r.engineLabel,
                account: r.accountLabel ?? t("plan.confirm.ownSignIn"),
              })}
            </span>
            {r.skip ? (
              <span className="flex items-center gap-2 text-sm text-fg-muted">
                <StatusDot status="idle" label={t("plan.leftAlone", { why: t(SKIP[r.skip]) })} />
                {t("plan.leftAlone", { why: t(SKIP[r.skip]) })}
              </span>
            ) : (
              <span className="text-sm text-fg-muted">
                {t("plan.confirm.how", { engine: r.reviewEngineLabel })}
              </span>
            )}
          </li>
        ))}
      </ol>
    </div>
  );
}

function PlanDialog({ plan }: { plan: PlanView }) {
  const { show: toast } = useToast();
  const [tasks, setTasks] = useState<PlanTask[]>(plan.tasks);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Execute all's check, while it's shown. */
  const [launch, setLaunch] = useState<PlanLaunch | null>(null);
  // A draft arriving replaces the (empty) rows; later edits are the user's.
  useEffect(() => {
    if (plan.state === "ready") setTasks((cur) => (cur.length === 0 ? plan.tasks : cur));
  }, [plan.state, plan.tasks]);
  // The check is for a plan still waiting to start.
  useEffect(() => {
    if (plan.state !== "created") setLaunch(null);
  }, [plan.state]);
  const close = () => usePlans.getState().open(null);
  const discard = () => void usePlans.getState().discard(plan.id);
  const attempts = useTasks((s) => s.attempts);
  const following = plan.state === "created" || plan.state === "starting" || plan.state === "started";
  // The plan follows its tasks' runs; `tasks:attempt` keeps them fresh.
  useEffect(() => {
    if (following) void useTasks.getState().loadAttempts();
  }, [following]);
  const approved = following ? approvedLanes(plan, attempts) : [];
  const land = () => {
    close();
    useWorktrees.getState().open({ kind: "land", workspaceId: plan.workspaceId, laneIds: approved });
  };
  const create = async () => {
    setBusy(true);
    setError(null);
    const r = await usePlans.getState().createTasks(plan.id, tasks);
    setBusy(false);
    if (isIpcError(r)) setError(r.message);
  };
  const check = async () => {
    setBusy(true);
    setError(null);
    const r = await usePlans.getState().prepare(plan.id);
    setBusy(false);
    if (isIpcError(r)) setError(r.message);
    else setLaunch(r);
  };
  const run = async () => {
    setBusy(true);
    setError(null);
    const r = await usePlans.getState().execute(plan.id);
    setBusy(false);
    setLaunch(null);
    if (isIpcError(r)) setError(r.message);
    else toast({ tone: "success", title: t("plan.queued", { n: r.queued.length }) });
  };
  const again = async () => {
    const goal = plan.goal;
    await usePlans.getState().discard(plan.id);
    const r = await usePlans.getState().start(plan.workspaceId, goal);
    if (isIpcError(r)) toast({ tone: "error", title: r.message });
  };
  // Rows written before Create tasks stopped stay as they are.
  const made = plan.state === "ready" ? plan.issueIds.length : 0;
  const valid = tasks.length > made && tasks.every((x) => x.title.trim().length > 0);
  const shownError = error ?? (plan.state === "failed" ? null : plan.error);
  // The browser demo has no Lead to open (its plans are simulated).
  const leadButton = plan.threadId ? (
    <Button variant="ghost" onClick={() => openLeadThread(plan)} data-testid="plan-open-thread">
      {t("plan.openThread")}
    </Button>
  ) : null;
  // The plan goes; its tasks stay in the tracker.
  const removeButton = (
    <Button variant="ghost" onClick={discard} data-testid="plan-remove">
      {t("plan.removePlan")}
    </Button>
  );
  const closeButton = (variant: "primary" | "ghost") => (
    <Button variant={variant} onClick={close} data-testid="plan-close">
      {t("common.close")}
    </Button>
  );
  const runnable = launch ? launch.runs.filter((r) => !r.skip).length : 0;
  const footer = (() => {
    switch (plan.state) {
      case "planning":
        return (
          <>
            <Button variant="ghost" onClick={discard}>
              {t("plan.discard")}
            </Button>
            {leadButton}
          </>
        );
      case "failed":
        return (
          <>
            <Button variant="ghost" onClick={discard}>
              {t("plan.discard")}
            </Button>
            {leadButton}
            <Button variant="primary" onClick={() => void again()} data-testid="plan-again">
              {t("plan.again")}
            </Button>
          </>
        );
      case "ready":
        return (
          <>
            <Button variant="ghost" onClick={discard}>
              {t("plan.discard")}
            </Button>
            {leadButton}
            <Button
              variant="primary"
              disabled={!valid || busy}
              onClick={() => void create()}
              data-testid="plan-create"
            >
              {t("plan.create", { n: tasks.length - made })}
            </Button>
          </>
        );
      case "created":
        if (launch)
          return (
            <>
              <Button variant="ghost" onClick={() => setLaunch(null)} data-testid="plan-confirm-back">
                {t("plan.confirm.back")}
              </Button>
              {!launch.blocked && runnable > 0 && (
                <Button
                  variant="primary"
                  disabled={busy}
                  onClick={() => void run()}
                  data-testid="plan-confirm-run"
                >
                  {t("plan.confirm.run", { n: runnable })}
                </Button>
              )}
            </>
          );
        return (
          <>
            {removeButton}
            {closeButton("ghost")}
            {approved.length > 0 && (
              <Button variant="secondary" onClick={land} data-testid="plan-land">
                {t("plan.land", { n: approved.length })}
              </Button>
            )}
            <Button variant="primary" disabled={busy} onClick={() => void check()} data-testid="plan-execute">
              {t("plan.execute", { n: plan.issueIds.length })}
            </Button>
          </>
        );
      case "starting":
        return closeButton("primary");
      case "started":
        return (
          <>
            {removeButton}
            {closeButton(approved.length > 0 ? "ghost" : "primary")}
            {approved.length > 0 && (
              <Button variant="primary" onClick={land} data-testid="plan-land">
                {t("plan.land", { n: approved.length })}
              </Button>
            )}
          </>
        );
    }
  })();
  return (
    <Dialog
      open
      size="lg"
      onOpenChange={(o) => !o && close()}
      title={t("plan.titleOf", { goal: plan.goal.split("\n")[0] ?? "" })}
      closeLabel={t("common.close")}
      footer={footer}
    >
      <div className="flex flex-col gap-3" data-testid="plan-dialog" data-state={plan.state}>
        {plan.state === "planning" && (
          <p className="flex items-center gap-2 text-sm text-fg" data-testid="plan-planning">
            <StatusDot
              status={plan.waiting ? "waiting" : "working"}
              label={plan.waiting ? t("plan.waiting") : t("plan.planning")}
            />
            {plan.waiting ? t("plan.waiting") : t("plan.planning")}
          </p>
        )}
        {plan.state === "failed" && (
          <p className="flex items-center gap-2 text-sm text-fg" data-testid="plan-failed">
            <StatusDot status="error" label={t("plan.failedLabel")} />
            {plan.error ?? t("plan.failedLabel")}
          </p>
        )}
        {plan.state === "ready" && (
          <>
            <p className="text-sm text-fg-muted">{t("plan.review")}</p>
            <ol className="flex flex-col gap-2" data-testid="plan-rows">
              {tasks.map((task, i) => (
                <Row
                  key={i}
                  task={task}
                  index={i}
                  engines={plan.engines}
                  canRemove={tasks.length > 1}
                  createdAs={i < made ? plan.issueIds[i] : undefined}
                  onChange={(n) => setTasks((cur) => cur.map((x, j) => (j === i ? n : x)))}
                  onRemove={() => setTasks((cur) => cur.filter((_, j) => j !== i))}
                />
              ))}
            </ol>
          </>
        )}
        {plan.state === "created" && launch && <LaunchList launch={launch} />}
        {plan.state === "created" && !launch && (
          <>
            <p className="flex items-center gap-2 text-sm text-fg" data-testid="plan-created">
              <StatusDot status="done" label={t("plan.createdLabel")} />
              {t("plan.created", { n: plan.issueIds.length, ids: plan.issueIds.join(", ") })}
            </p>
            <PlanRuns plan={plan} attempts={attempts} />
          </>
        )}
        {plan.state === "starting" && (
          <>
            <p className="flex items-center gap-2 text-sm text-fg" data-testid="plan-starting">
              <StatusDot status="working" label={t("plan.starting")} />
              {t("plan.starting")}
            </p>
            <PlanRuns plan={plan} attempts={attempts} />
          </>
        )}
        {plan.state === "started" && (
          <>
            <p className="flex items-center gap-2 text-sm text-fg" data-testid="plan-created">
              <StatusDot status="done" label={t("plan.createdLabel")} />
              {t("plan.started", { n: plan.queued.length })}
            </p>
            {plan.skipped.length > 0 && (
              <p className="text-sm text-fg-muted" data-testid="plan-skipped">
                {t("plan.skipped", {
                  list: plan.skipped.map((x) => `${x.issueId} (${t(SKIP[x.reason])})`).join(", "),
                })}
              </p>
            )}
            <PlanRuns plan={plan} attempts={attempts} />
          </>
        )}
        {shownError && (
          <p className="text-sm text-error" role="alert" data-testid="plan-error">
            {shownError}
          </p>
        )}
      </div>
    </Dialog>
  );
}

/** The Tasks page's "Plan with Lead…": the goal form, or the open plan. */
export function PlanWithLead({
  asking,
  onAskingChange,
}: {
  asking: boolean;
  onAskingChange: (a: boolean) => void;
}) {
  const openId = usePlans((s) => s.openId);
  const plan = usePlans((s) => (s.openId ? s.plans[s.openId] : undefined));
  useEffect(() => {
    if (openId) onAskingChange(false);
  }, [openId, onAskingChange]);
  if (plan) return <PlanDialog key={plan.id} plan={plan} />;
  if (asking) return <GoalForm onClose={() => onAskingChange(false)} />;
  return null;
}
