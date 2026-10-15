// F-RT-06, UI-171: "Run at reset…" queues a task to start on an account the
// user picks, when that account's usage window resets. The dialog states the
// plan before anything is saved; Schedule run is the approval. It never uses
// another account.
import { useEffect, useMemo, useState } from "react";
import { Moon } from "lucide-react";
import type { Access, Issue, NightAccount, NightRun, TaskIsolation as Isolation } from "@tandem/protocol";
import { Button, Callout, Checkbox, Dialog, Select, Skeleton, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import { resetClock } from "../../stores/limits";
import { useNightShift } from "../../stores/nightShift";
import { useRoutines } from "../../stores/routines";
import { isIpcError } from "../../stores/tasks";
import { useWorkspaces } from "../../stores/workspaces";
import { useExecuteBlock } from "./Execute";

/** "5-hour window", "weekly window", "limit". */
function windowWords(kind: string | null): string {
  if (kind === "5h") return t("night.window.5h");
  if (kind === "week") return t("night.window.week");
  if (kind === "limit") return t("night.window.limit");
  return kind ?? "";
}

/** The plan in words (UI-171), before anything is saved. */
export function planText(opts: {
  task: string;
  account: string;
  engine: string;
  at: number;
  window: string | null;
  isolation: Isolation;
  access: Access;
}): string {
  return t("night.plan", {
    task: opts.task,
    account: opts.account,
    engine: opts.engine,
    time: resetClock(opts.at),
    window: windowWords(opts.window),
    where: opts.isolation === "lane" ? t("night.plan.lane") : t("night.plan.shared"),
    access: t(
      opts.access === "full"
        ? "night.access.full"
        : opts.access === "read"
          ? "night.access.read"
          : "night.access.edits",
    ),
  });
}

/** Accounts that can run tasks, with their next reset; unknown ones can't be picked. */
export function accountOptions(
  accounts: NightAccount[],
  label: (id: string) => string,
  chatEngines: Set<string>,
): { value: string; label: string; disabled: boolean }[] {
  return accounts
    .filter((a) => chatEngines.has(a.engineId))
    .map((a) => ({
      value: a.accountId,
      label:
        a.resetsAt !== null
          ? t("night.account", { account: label(a.accountId), time: resetClock(a.resetsAt) })
          : t("night.account.unknown", { account: label(a.accountId) }),
      disabled: a.resetsAt === null,
    }));
}

export function RunAtResetDialog({
  issue,
  open,
  onOpenChange,
}: {
  issue: Issue;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const block = useExecuteBlock();
  const workspaces = useWorkspaces((s) => s.list).filter(
    (w) => !w.parentId && !w.missing && w.kind === "local",
  );
  const accountsList = useAccounts((s) => s.list);
  const engines = useEngines((s) => s.list);
  const engineLabel = useEngines((s) => s.label);
  const nightAccounts = useNightShift((s) => s.accounts);
  const accountsState = useNightShift((s) => s.accountsState);
  const background = useRoutines((s) => s.background?.installed ?? false);
  const { show: toast } = useToast();
  const [workspace, setWorkspace] = useState("");
  const [account, setAccount] = useState("");
  const [isolation, setIsolation] = useState<Isolation>("lane");
  const [access, setAccess] = useState<Access>("edits");
  const [fullOk, setFullOk] = useState(false);
  const [review, setReview] = useState(true);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!open) return;
    void useNightShift.getState().loadAccounts();
    if (!useRoutines.getState().background) void useRoutines.getState().load();
  }, [open]);
  const chat = useMemo(
    () => new Set(engines.filter((e) => e.chatCapable && e.install).map((e) => e.spec.id)),
    [engines],
  );
  const accountLabel = (id: string) => accountsList.find((a) => a.id === id)?.label ?? id;
  const options = accountOptions(nightAccounts, accountLabel, chat);
  const picked = nightAccounts.find(
    (a) => a.accountId === (account || options.find((o) => !o.disabled)?.value),
  );
  const ws = workspace || workspaces[0]?.id || "";
  const can =
    !block && !!ws && !!picked && picked.resetsAt !== null && !busy && (access !== "full" || fullOk);
  const schedule = async () => {
    if (!picked) return;
    setBusy(true);
    const r = await useNightShift.getState().schedule({
      issueId: issue.id,
      workspaceId: ws,
      accountId: picked.accountId,
      isolation,
      access,
      fullConfirmed: access === "full" && fullOk,
      review,
    });
    setBusy(false);
    if (isIpcError(r)) {
      toast({ tone: "error", title: r.message });
      return;
    }
    toast({ tone: "success", title: t("night.scheduled") });
    onOpenChange(false);
  };
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("night.title", { id: issue.id })}
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {t("tasks.exec.cancel")}
          </Button>
          <Button
            variant="primary"
            disabled={!can}
            onClick={() => void schedule()}
            data-testid="night-schedule"
          >
            {t("night.schedule")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3" data-testid="night-dialog">
        {block && (
          <Callout tone="info">
            <span data-testid="night-blocked">{block.reason}</span>
          </Callout>
        )}
        {accountsState === "loading" || accountsState === "idle" ? (
          <Skeleton height={32} />
        ) : accountsState === "error" ? (
          <Callout tone="error">{t("night.accountsError")}</Callout>
        ) : options.length === 0 ? (
          <p className="text-sm text-fg-muted" data-testid="night-no-accounts">
            {t("night.noAccounts")}
          </p>
        ) : (
          <Select
            label={t("night.accountLabel")}
            value={picked?.accountId ?? ""}
            options={options}
            onValueChange={setAccount}
          />
        )}
        <Select
          label={t("tasks.exec.workspace")}
          value={ws}
          options={workspaces.map((w) => ({ value: w.id, label: w.name }))}
          placeholder={t("tasks.exec.noWorkspace")}
          onValueChange={setWorkspace}
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
        {picked && picked.resetsAt !== null && (
          <Callout tone="info">
            <span data-testid="night-plan">
              {planText({
                task: issue.title,
                account: accountLabel(picked.accountId),
                engine: engineLabel(picked.engineId) ?? picked.engineId,
                at: picked.resetsAt,
                window: picked.window,
                isolation,
                access,
              })}
            </span>
            {!background && (
              <span className="mt-1 block text-fg-muted" data-testid="night-open-note">
                {t("night.mustBeOpen")}
              </span>
            )}
          </Callout>
        )}
      </div>
    </Dialog>
  );
}

/** The task's scheduled and waiting runs, with Cancel. */
export function NightRuns({ issueId }: { issueId: string }) {
  const runs = useNightShift((s) => s.runs).filter(
    (r: NightRun) => r.issueId === issueId && (r.state === "scheduled" || r.state === "waiting"),
  );
  const accounts = useAccounts((s) => s.list);
  const { show: toast } = useToast();
  useEffect(() => {
    void useNightShift.getState().load();
  }, [issueId]);
  if (!runs.length) return null;
  return (
    <ul className="flex flex-col gap-1" data-testid="night-runs">
      {runs.map((r) => {
        const account = accounts.find((a) => a.id === r.accountId)?.label ?? r.accountId;
        return (
          <li
            key={r.id}
            className="flex items-center gap-2 text-sm"
            data-testid="night-run"
            data-state={r.state}
          >
            <Moon size={13} aria-hidden="true" className="text-fg-subtle" />
            <span className="flex-1 text-fg">
              {r.state === "waiting"
                ? t("night.waiting", { account, time: resetClock(r.runAt) })
                : t("night.runsAt", { time: resetClock(r.runAt), account })}
            </span>
            <Button
              size="sm"
              variant="ghost"
              data-testid="night-cancel"
              onClick={() =>
                void useNightShift
                  .getState()
                  .cancel(r.id)
                  .then((e) => e && toast({ tone: "error", title: e.message }))
              }
            >
              {t("night.cancel")}
            </Button>
          </li>
        );
      })}
    </ul>
  );
}
