import { useEffect, useMemo, useState } from "react";
import type {
  EngineView,
  FileDiff,
  IpcError,
  SecondOpinion,
  SecondOpinionStarted,
  WorkspaceSummary,
} from "@tandem/protocol";
import { Button, Dialog, Select, Skeleton, Textarea, type SelectOption } from "@tandem/ui";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { accountForNext, accountsFor, useAccounts } from "../../stores/accounts";
import { useDashboard } from "../../stores/dashboard";
import { useEngines } from "../../stores/engines";
import { useSession } from "../../stores/session";
import { useWorktrees, worktreeDiff } from "../../stores/worktrees";
import { openThread, useReviews, worktreeAgents } from "./WorktreeReview";

/** Who can review (F-WT-05): installed engines; only Thread engines can run
 * read-only, and Free runs only the plan's chat engines. */
export function reviewerOptions(engines: EngineView[], chatEngines: string[] | null): SelectOption[] {
  return engines
    .filter((e) => e.install)
    .map((e) => {
      const onPlan = chatEngines === null || chatEngines.includes(e.spec.id);
      return {
        value: e.spec.id,
        label: e.spec.label,
        disabled: !e.chatCapable || !onPlan,
        disabledReason: !e.chatCapable
          ? t("worktree.opinion.readOnlyNo")
          : onPlan
            ? undefined
            : t("thread.composer.pro"),
      };
    });
}

/** The default reviewer: one no agent in the worktree runs, else any. */
export function defaultReviewer(options: SelectOption[], inWorktree: string[]): string {
  const ok = options.filter((o) => !o.disabled);
  return (ok.find((o) => !inWorktree.includes(o.value)) ?? ok[0])?.value ?? "";
}

/** "Ask for a second opinion…" (F-WT-05, D-2420). */
export function SecondOpinionDialog({ wt, onClose }: { wt: WorkspaceSummary; onClose: () => void }) {
  const engines = useEngines((s) => s.list);
  const enginesLoaded = useEngines((s) => s.loaded);
  const accounts = useAccounts((s) => s.list);
  const overrides = useAccounts((s) => s.overrides);
  const chatEngines = useSession((s) => s.entitlement?.limits.chatEngines ?? null);
  const items = useDashboard((s) => s.items);
  const [files, setFiles] = useState<FileDiff[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [engineId, setEngineId] = useState("");
  const [accountId, setAccountId] = useState("");
  const [task, setTask] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!useEngines.getState().loaded) void useEngines.getState().refresh();
    if (!useAccounts.getState().loaded) void useAccounts.getState().load();
    let live = true;
    void worktreeDiff(wt.id).then((out) => {
      if (!live) return;
      if (out.ok) setFiles(out.value);
      else setError(out.error.message);
    });
    return () => {
      live = false;
    };
  }, [wt.id]);

  const options = useMemo(() => reviewerOptions(engines, chatEngines), [engines, chatEngines]);
  const inWorktree = useMemo(
    () => worktreeAgents(items, wt.id).flatMap((i) => (i.engineId ? [i.engineId] : [])),
    [items, wt.id],
  );
  const engine = engineId || defaultReviewer(options, inWorktree);
  const engineAccounts = accountsFor(accounts, engine);
  const account =
    accountId && engineAccounts.some((a) => a.id === accountId)
      ? accountId
      : (accountForNext(accounts, overrides, engine, wt.id)?.id ?? engineAccounts[0]?.id ?? "");
  const accountLabel = engineAccounts.find((a) => a.id === account)?.label ?? null;
  const engineLabel = engines.find((e) => e.spec.id === engine)?.spec.label ?? engine;
  const pickable = options.some((o) => !o.disabled);
  const noChanges = files !== null && files.length === 0;

  const ask = async () => {
    setBusy(true);
    try {
      const started = await invoke<SecondOpinionStarted>("lane_second_opinion", {
        laneId: wt.id,
        engineId: engine,
        accountId: account || null,
        task: task.trim() || null,
      });
      useReviews.getState().setReviewing(wt.id, { threadId: started.threadId, engineId: engine });
      // Back to the diff, where the review bar shows it working.
      useWorktrees.getState().open({ kind: "diff", worktreeId: wt.id });
    } catch (e) {
      setError((e as IpcError).message);
      setBusy(false);
    }
  };

  const loading = files === null && error === null;
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("worktree.opinion.title", { branch: wt.laneBranch ?? wt.name })}
      description={t("worktree.opinion.description")}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={loading || noChanges || !pickable || !engine || error !== null}
            onClick={() => void ask()}
            data-testid="opinion-ask"
          >
            {t("worktree.opinion.ask")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3" data-testid="opinion-sheet">
        {error ? (
          <p className="text-sm text-error" data-testid="opinion-error">
            {error}
          </p>
        ) : loading || !enginesLoaded ? (
          <div className="flex flex-col gap-2" aria-busy="true">
            <Skeleton height={32} />
            <Skeleton height={64} />
          </div>
        ) : noChanges ? (
          <p className="text-sm text-fg-muted" data-testid="opinion-no-changes">
            {t("worktree.opinion.noChanges")}
          </p>
        ) : !pickable ? (
          <p className="text-sm text-fg-muted" data-testid="opinion-empty">
            {t("worktree.opinion.empty")}
          </p>
        ) : (
          <>
            <Select
              label={t("worktree.opinion.engine")}
              value={engine}
              onValueChange={(v) => {
                setEngineId(v);
                setAccountId("");
              }}
              options={options}
            />
            {engineAccounts.length > 1 && (
              <Select
                label={t("worktree.opinion.account")}
                value={account}
                onValueChange={setAccountId}
                options={engineAccounts.map((a) => ({ value: a.id, label: a.label }))}
              />
            )}
            <p className="text-xs text-fg-muted" data-testid="opinion-uses">
              {accountLabel
                ? t("worktree.opinion.uses", { account: accountLabel, engine: engineLabel })
                : t("worktree.opinion.usesDefault", { engine: engineLabel })}
            </p>
            <Textarea
              label={t("worktree.opinion.task")}
              minRows={2}
              maxRows={6}
              value={task}
              onChange={(e) => setTask(e.target.value)}
              data-testid="opinion-task"
            />
          </>
        )}
      </div>
    </Dialog>
  );
}

/** `review:second_opinion`: the comments go into the worktree's review bar
 * and a toast says so (F-WT-05). */
export function followSecondOpinions(): () => void {
  const stop = listen<SecondOpinion>("review:second_opinion", (op) => {
    const reviews = useReviews.getState();
    reviews.setReviewing(op.laneId, null);
    const engine = useEngines.getState().label(op.engineId) ?? op.engineId;
    const { notify, open } = useWorktrees.getState();
    if (op.outcome === "failed") {
      notify({
        tone: "error",
        title: t("worktree.opinion.failed"),
        action: { label: t("worktree.opinion.openThread"), onClick: () => openThread(op.threadId) },
      });
      return;
    }
    reviews.addOpinion(op);
    const n = op.comments.length + (op.general ? 1 : 0);
    notify(
      n === 0
        ? { title: t("worktree.opinion.none", { engine }) }
        : {
            tone: "success",
            title: t("worktree.opinion.done", { engine, n }),
            action: {
              label: t("worktree.opinion.showDiff"),
              onClick: () => open({ kind: "diff", worktreeId: op.laneId }),
            },
          },
    );
  });
  return () => void stop.then((s) => s());
}
