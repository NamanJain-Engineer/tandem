// F-TK-06 Plan with Lead: a goal goes to Lead, which plans in its own chat
// in the workspace; its plan comes back as editable rows (title,
// description, engine). Create tasks writes them; Execute all runs them
// through F-TK-04's queue, which needs sign-in and Pro (D-2430).
import { useEffect, useState } from "react";
import { X } from "lucide-react";
import type { PlanEngine, PlanTask, PlanView } from "@tandem/protocol";
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
import { t } from "../../i18n";
import { switchMode } from "../shell/actions";
import { usePlans } from "../../stores/plan";
import { isIpcError } from "../../stores/tasks";
import { useTeammates } from "../../stores/teammates";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { useExecuteBlock } from "./Execute";

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
  onChange,
  onRemove,
}: {
  task: PlanTask;
  index: number;
  engines: PlanEngine[];
  canRemove: boolean;
  onChange: (t: PlanTask) => void;
  onRemove: () => void;
}) {
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
          onChange={(e) => onChange({ ...task, title: e.target.value })}
          data-testid="plan-row-title"
        />
        <Select
          className="w-56"
          label={t("plan.engine")}
          value={task.engineId}
          options={engines.map(engineOption)}
          onValueChange={(v) => onChange({ ...task, engineId: v, fellBack: false })}
        />
        {canRemove && (
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
        onChange={(e) => onChange({ ...task, description: e.target.value })}
      />
      {task.fellBack ? (
        <p className="text-sm text-fg-muted" data-testid="plan-row-fell-back">
          {t("plan.fellBack")}
        </p>
      ) : (
        task.why && <p className="text-sm text-fg-subtle">{task.why}</p>
      )}
    </li>
  );
}

function PlanDialog({ plan }: { plan: PlanView }) {
  const { show: toast } = useToast();
  const block = useExecuteBlock();
  const [tasks, setTasks] = useState<PlanTask[]>(plan.tasks);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A draft arriving replaces the (empty) rows; later edits are the user's.
  useEffect(() => {
    if (plan.state === "ready") setTasks((cur) => (cur.length === 0 ? plan.tasks : cur));
  }, [plan.state, plan.tasks]);
  const close = () => usePlans.getState().open(null);
  const discard = () => void usePlans.getState().discard(plan.id);
  const create = async () => {
    setBusy(true);
    setError(null);
    const r = await usePlans.getState().createTasks(plan.id, tasks);
    setBusy(false);
    if (isIpcError(r)) setError(r.message);
  };
  const executeAll = async () => {
    setBusy(true);
    setError(null);
    const r = await usePlans.getState().execute(plan.id);
    setBusy(false);
    if (isIpcError(r)) setError(r.message);
    else toast({ tone: "success", title: t("plan.queued", { n: r.issueIds.length }) });
  };
  const again = async () => {
    const goal = plan.goal;
    await usePlans.getState().discard(plan.id);
    const r = await usePlans.getState().start(plan.workspaceId, goal);
    if (isIpcError(r)) toast({ tone: "error", title: r.message });
  };
  const valid = tasks.length > 0 && tasks.every((x) => x.title.trim().length > 0);
  // The browser demo has no Lead to open (its plans are simulated).
  const leadButton = plan.threadId ? (
    <Button variant="ghost" onClick={() => openLeadThread(plan)} data-testid="plan-open-thread">
      {t("plan.openThread")}
    </Button>
  ) : null;
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
              {t("plan.create", { n: tasks.length })}
            </Button>
          </>
        );
      case "created":
        return (
          <>
            <Button variant="ghost" onClick={close} data-testid="plan-close">
              {t("common.close")}
            </Button>
            {!block && (
              <Button
                variant="primary"
                disabled={busy}
                onClick={() => void executeAll()}
                data-testid="plan-execute"
              >
                {t("plan.execute", { n: plan.issueIds.length })}
              </Button>
            )}
          </>
        );
      case "started":
        return (
          <Button variant="primary" onClick={close} data-testid="plan-close">
            {t("common.close")}
          </Button>
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
            <StatusDot status="working" label={t("plan.planning")} />
            {t("plan.planning")}
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
                  onChange={(n) => setTasks((cur) => cur.map((x, j) => (j === i ? n : x)))}
                  onRemove={() => setTasks((cur) => cur.filter((_, j) => j !== i))}
                />
              ))}
            </ol>
          </>
        )}
        {(plan.state === "created" || plan.state === "started") && (
          <>
            <p className="flex items-center gap-2 text-sm text-fg" data-testid="plan-created">
              <StatusDot status="done" label={t("plan.createdLabel")} />
              {plan.state === "created"
                ? t("plan.created", { n: plan.issueIds.length, ids: plan.issueIds.join(", ") })
                : t("plan.started", { n: plan.issueIds.length })}
            </p>
            {plan.state === "created" && block && (
              <Callout
                tone="info"
                actions={
                  block.upgrade ? (
                    <Button size="sm" variant="secondary" onClick={() => useUi.getState().openUpgrade()}>
                      {t("tasks.exec.upgrade")}
                    </Button>
                  ) : undefined
                }
              >
                <span data-testid="plan-execute-blocked">
                  {block.reason} {t("plan.tasksStay")}
                </span>
              </Callout>
            )}
          </>
        )}
        {error && (
          <p className="text-sm text-error" role="alert" data-testid="plan-error">
            {error}
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
