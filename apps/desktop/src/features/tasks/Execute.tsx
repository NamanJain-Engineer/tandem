// F-TK-04, UI-82/83/84: Execute… asks for the workspace, the runner (a
// teammate or a bare chat engine), isolation (a new worktree by default),
// access (Edits by default; Full only with an explicit choice for this task)
// and the independent review. Runs need sign-in and Pro; at most 4 run at
// once and the rest queue. The Runs list shows each attempt with Stop,
// Resume and Retry.
import { useEffect, useMemo, useState } from "react";
import type { Access, AttemptView, Issue, TaskIsolation as Isolation } from "@tandem/protocol";
import {
  Badge,
  Button,
  Callout,
  Checkbox,
  Dialog,
  Select,
  Textarea,
  useToast,
  type BadgeVariant,
} from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { useEngines } from "../../stores/engines";
import { useSession } from "../../stores/session";
import { useTasks, isIpcError } from "../../stores/tasks";
import { useTeammates } from "../../stores/teammates";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";

/** Runs at once (F-TK-04). */
export const MAX_ACTIVE = 4;

const ACTIVE = new Set(["setup", "running", "reviewing"]);

/** Why Execute can't run here, if it can't. */
export function useExecuteBlock(): { reason: string; upgrade: boolean } | null {
  const auth = useSession((s) => s.auth);
  const ent = useSession((s) => s.entitlement);
  if (!auth?.signedIn) return { reason: t("tasks.exec.signIn"), upgrade: false };
  if (!ent?.limits.taskExecution) return { reason: t("tasks.exec.pro"), upgrade: true };
  return null;
}

export function ExecuteDialog({
  issue,
  open,
  onOpenChange,
}: {
  issue: Issue;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const workspaces = useWorkspaces((s) => s.list).filter((w) => !w.parentId && !w.missing);
  const engines = useEngines((s) => s.list).filter((e) => e.chatCapable && e.install);
  const teammates = useTeammates((s) => s.list).filter((s) => s.chatCapable);
  const attempts = useTasks((s) => s.attempts);
  const block = useExecuteBlock();
  const { show: toast } = useToast();
  const [workspace, setWorkspace] = useState("");
  const [runner, setRunner] = useState("");
  const [isolation, setIsolation] = useState<Isolation>("lane");
  const [access, setAccess] = useState<Access>("edits");
  const [fullOk, setFullOk] = useState(false);
  const [review, setReview] = useState(true);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!open) return;
    void useTasks.getState().loadAttempts();
    void useTeammates.getState().refresh();
  }, [open]);
  const runners = useMemo(
    () => [
      ...teammates.map((s) => ({
        value: `tm:${s.teammate.id}`,
        label: s.teammate.name,
        group: t("tasks.exec.teammates"),
      })),
      ...engines.map((e) => ({
        value: `en:${e.spec.id}`,
        label: e.spec.label,
        group: t("tasks.exec.engines"),
      })),
    ],
    [teammates, engines],
  );
  const ws = workspace || workspaces[0]?.id || "";
  const who = runner || runners[0]?.value || "";
  const busySlots = attempts.filter((a) => ACTIVE.has(a.attempt.state)).length;
  const full = busySlots >= MAX_ACTIVE;
  const canStart = !block && !!ws && !!who && !busy && (access !== "full" || fullOk);
  const start = async () => {
    setBusy(true);
    const r = await useTasks.getState().execute({
      issueId: issue.id,
      workspaceId: ws,
      teammateId: who.startsWith("tm:") ? who.slice(3) : undefined,
      engineId: who.startsWith("en:") ? who.slice(3) : undefined,
      isolation,
      access,
      fullConfirmed: access === "full" && fullOk,
      review,
      note: note.trim() || undefined,
    });
    setBusy(false);
    if (isIpcError(r)) {
      toast({ tone: "error", title: r.message });
      return;
    }
    onOpenChange(false);
  };
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("tasks.exec.title", { id: issue.id })}
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {t("tasks.exec.cancel")}
          </Button>
          <Button
            variant="primary"
            disabled={!canStart}
            onClick={() => void start()}
            data-testid="task-execute-start"
          >
            {full ? t("tasks.exec.queue") : t("tasks.exec.start")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3" data-testid="task-execute-dialog">
        {block && (
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
            <span data-testid="task-execute-blocked">{block.reason}</span>
          </Callout>
        )}
        <Select
          label={t("tasks.exec.workspace")}
          value={ws}
          options={workspaces.map((w) => ({ value: w.id, label: w.name }))}
          placeholder={t("tasks.exec.noWorkspace")}
          onValueChange={setWorkspace}
        />
        <Select
          label={t("tasks.exec.runner")}
          value={who}
          options={runners}
          placeholder={t("tasks.exec.noRunner")}
          onValueChange={setRunner}
        />
        <Select
          label={t("tasks.exec.isolation")}
          value={isolation}
          options={[
            { value: "lane", label: t("tasks.exec.lane") },
            { value: "shared", label: t("tasks.exec.shared") },
          ]}
          onValueChange={(v) => setIsolation(v as Isolation)}
        />
        <Select
          label={t("tasks.exec.access")}
          value={access}
          options={[
            { value: "read", label: t("tasks.exec.read") },
            { value: "edits", label: t("tasks.exec.edits") },
            { value: "full", label: t("tasks.exec.full") },
          ]}
          onValueChange={(v) => {
            setAccess(v as Access);
            setFullOk(false);
          }}
        />
        {access === "full" && (
          <Callout tone="warning">
            <Checkbox label={t("tasks.exec.fullConfirm")} checked={fullOk} onCheckedChange={setFullOk} />
          </Callout>
        )}
        <Checkbox label={t("tasks.exec.review")} checked={review} onCheckedChange={setReview} />
        <Textarea
          label={t("tasks.exec.note")}
          rows={3}
          value={note}
          placeholder={t("tasks.exec.notePlaceholder")}
          onChange={(e) => setNote(e.target.value)}
        />
        {full && <p className="text-sm text-fg-muted">{t("tasks.exec.allBusy")}</p>}
      </div>
    </Dialog>
  );
}

const BADGE: Record<string, BadgeVariant> = {
  queued: "idle",
  setup: "working",
  running: "working",
  reviewing: "working",
  in_review: "waiting",
  stopped: "idle",
  failed: "error",
  done: "done",
};

function label(v: AttemptView): string {
  if (v.attempt.state === "queued" && v.queuePosition) return t("tasks.runs.queued", { n: v.queuePosition });
  return t(`tasks.runs.state.${v.attempt.state}` as MessageKey);
}

/** UI-81 RUNS: the issue's attempts with their controls. */
export function Runs({ issueId }: { issueId: string }) {
  const attempts = useTasks((s) => s.attempts).filter((a) => a.attempt.issueId === issueId);
  const { show: toast } = useToast();
  useEffect(() => {
    void useTasks.getState().loadAttempts();
  }, [issueId]);
  if (!attempts.length) return null;
  const act = async (action: "stop" | "resume" | "retry", id: string) => {
    const r = await useTasks.getState().attemptAction(action, id);
    if (isIpcError(r)) toast({ tone: "error", title: r.message });
  };
  return (
    <div className="flex flex-col gap-1.5" data-testid="task-runs">
      <h3 className="text-xs font-medium uppercase tracking-caps text-fg-subtle">{t("tasks.runs.title")}</h3>
      <ul className="flex flex-col gap-1">
        {attempts.map((v, i) => {
          const a = v.attempt;
          const going = !["in_review", "stopped", "failed", "done"].includes(a.state);
          return (
            <li
              key={a.id}
              className="flex flex-wrap items-center gap-2 text-sm"
              data-testid="task-run"
              data-state={a.state}
            >
              <span className="text-fg">{t("tasks.runs.n", { n: i + 1 })}</span>
              <Badge variant={BADGE[a.state] ?? "neutral"}>{label(v)}</Badge>
              <span className="text-fg-muted">{a.engineId}</span>
              {a.reviewVerdict && (
                <Badge variant={a.reviewVerdict === "approve" ? "done" : "waiting"}>
                  <span data-testid="task-run-verdict">
                    {a.reviewVerdict === "approve" ? t("tasks.runs.approve") : t("tasks.runs.changes")}
                  </span>
                </Badge>
              )}
              {a.error && <span className="text-fg-muted">{a.error}</span>}
              <span className="flex-1" />
              {going ? (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => void act("stop", a.id)}
                  data-testid="task-run-stop"
                >
                  {t("tasks.runs.stop")}
                </Button>
              ) : (
                <>
                  {a.threadId && (
                    <Button size="sm" variant="ghost" onClick={() => void act("resume", a.id)}>
                      {t("tasks.runs.resume")}
                    </Button>
                  )}
                  <Button size="sm" variant="ghost" onClick={() => void act("retry", a.id)}>
                    {t("tasks.runs.retry")}
                  </Button>
                </>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
