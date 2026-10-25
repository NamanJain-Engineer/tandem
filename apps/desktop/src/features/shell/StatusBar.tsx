import { useEffect, useMemo, useState } from "react";
import { GitBranch, Mic, Plug } from "lucide-react";
import type { ListeningPort } from "@tandem/protocol";
import { StatusDot, cx, meterTone } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";
import { useStatus } from "../../stores/status";
import { fullest, useUsage } from "../../stores/usage";
import { useWorktrees } from "../../stores/worktrees";
import { useChipAccount } from "./UsageChip";
import { useFiles } from "../../stores/files";
import { gitKey, useGitContext } from "../../stores/git";
import { useDictate } from "../../stores/dictate";
import { useSession } from "../../stores/session";
import { useSidebar } from "../../stores/sidebar";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { useActiveFiles } from "../files/context";
import { useBoot } from "./useBoot";

/** UI §3.6: the branch of the focused workspace (the Files tab's context)
 * with ahead/behind, which opens the Files tab's Changes view, and its CI
 * state; agents working and waiting; the usage window and the listening
 * ports (D-2603); the connection and the version. An early-access build has no hosted service
 * to be connected to, so it shows no connection at all (D-816). */
export function StatusBar() {
  const version = useBoot((s) => s.boot?.version ?? "");
  const online = useSession((s) => s.entitlement?.online ?? true);
  const hosted = useSession((s) => s.auth?.hosted ?? true);
  const active = useActiveFiles();
  const context = active.kind === "context" ? active.context : null;
  const git = useGitContext(context);
  const known = useWorkspaces((s) => s.list.find((w) => w.id === s.activeId)?.branch ?? null);
  const status = git?.panel?.status ?? null;
  const branch = status ? (status.branch ?? t("git.branch.detached")) : git?.at === "ready" ? null : known;
  const counts = status
    ? status.upstream
      ? t("git.branch.counts", { ahead: status.ahead, behind: status.behind })
      : t("git.branch.unpublished")
    : null;
  const workspaceId = useWorkspaces((s) => s.activeId);
  const openChanges = () => {
    if (!context) return;
    useFiles.getState().showView(gitKey(context), "changes");
    useSidebar.getState().openTab("files");
  };
  return (
    <footer
      role="contentinfo"
      aria-label={t("status.label")}
      className="flex h-[var(--statusbar-h)] shrink-0 items-center gap-3 border-t border-divider bg-surface-1 px-3 text-2xs text-fg-muted"
    >
      {context && branch ? (
        <button
          type="button"
          data-testid="status-branch"
          data-ahead={status?.ahead}
          data-behind={status?.behind}
          aria-label={t("status.branch", { name: branch, counts: counts ?? "" })}
          onClick={openChanges}
          className="inline-flex items-center gap-1 rounded-xs hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
        >
          <GitBranch size={11} aria-hidden="true" />
          {branch}
          {status?.upstream && (
            <span aria-hidden="true" className="tabular-nums">
              {t("git.branch.short", { ahead: status.ahead, behind: status.behind })}
            </span>
          )}
        </button>
      ) : (
        <span className="inline-flex items-center gap-1" data-testid="status-branch">
          <GitBranch size={11} aria-hidden="true" />
          {t("status.noBranch")}
        </span>
      )}
      {workspaceId && <ChecksSegment workspaceId={workspaceId} />}
      <AgentsSegment />
      <span className="flex-1" />
      <PortsSegment />
      <UsageSegment />
      <DictateSegment />
      {hosted && (
        <span data-testid="connection-state" className={online ? undefined : "text-warning"}>
          {online ? t("status.online") : t("status.offline")}
        </span>
      )}
      <span className="tabular-nums">{t("status.version", { version })}</span>
    </footer>
  );
}

const SEGMENT =
  "inline-flex items-center gap-1 rounded-xs tabular-nums hover:text-fg focus-visible:shadow-focus focus-visible:outline-none";

/** "● 2 working  ◆ 1 waiting" across every agent (D-2603): each opens the
 * Dashboard filtered to it. Nothing when no agent is busy. */
export function AgentsSegment() {
  const records = useStatus((s) => s.records);
  const { working, waiting } = useMemo(() => {
    const all = Object.values(records);
    return {
      working: all.filter((r) => r.status === "working").length,
      waiting: all.filter((r) => r.status === "waiting").length,
    };
  }, [records]);
  const open = (filter: "working" | "waiting") => {
    useDashboard.getState().setFilter(filter);
    useSidebar.getState().openTab("dashboard");
  };
  return (
    <>
      {waiting > 0 && (
        <button
          type="button"
          data-testid="status-waiting"
          onClick={() => open("waiting")}
          className={cx(SEGMENT, "font-medium text-waiting-text")}
        >
          <span aria-hidden="true" className="inline-flex"><StatusDot status="waiting" size={6} /></span>
          {t("status.waiting", { n: waiting })}
        </button>
      )}
      {working > 0 && (
        <button type="button" data-testid="status-working" onClick={() => open("working")} className={SEGMENT}>
          <span aria-hidden="true" className="inline-flex"><StatusDot status="working" size={6} /></span>
          {t("status.working", { n: working })}
        </button>
      )}
    </>
  );
}

/** The CI state of the focused worktree's pull request (F-WT-08's checks). */
function ChecksSegment({ workspaceId }: { workspaceId: string }) {
  const checks = useWorktrees((s) => s.checks[workspaceId]);
  if (checks?.state !== "ready" || !checks.pr || checks.checks.length === 0) return null;
  const failed = checks.checks.filter((c) => c.bucket === "fail").length;
  const running = checks.checks.some((c) => c.bucket === "pending");
  const state = failed > 0 ? "failed" : running ? "running" : "passed";
  return (
    <button
      type="button"
      data-testid="status-checks"
      data-state={state}
      onClick={() => useWorktrees.getState().open({ kind: "checks", worktreeId: workspaceId })}
      className={cx(SEGMENT, state === "failed" && "text-danger", state === "passed" && "text-done-text")}
    >
      <span aria-hidden="true" className="inline-flex"><StatusDot status={state === "failed" ? "error" : state === "running" ? "working" : "done"} size={6} /></span>
      {state === "failed" ? t("status.ciFailed", { n: failed }) : state === "running" ? t("status.ciRunning") : t("status.ciPassed")}
    </button>
  );
}

/** The fullest usage window of the account the title bar's chip follows:
 * "5h 42%", in warning or danger past the meter's thresholds. */
function UsageSegment() {
  const account = useChipAccount();
  const view = useUsage((s) => (account ? s.byAccount[account] : undefined));
  const top = view ? fullest(view.windows) : null;
  if (!top) return null;
  const tone = meterTone(top.usedPct / 100, "plan");
  return (
    <span
      data-testid="status-usage"
      title={t("status.usageTooltip", { window: top.kind, pct: Math.round(top.usedPct) })}
      className={cx("tabular-nums", tone === "warning" && "text-warning-text", tone === "danger" && "text-danger")}
    >
      {t("status.usage", { window: top.kind, pct: Math.round(top.usedPct) })}
    </span>
  );
}

/** Ports listening in panes (F-EX-03): one shows its address, more their
 * count; either opens the Ports tab. Read every 15 s while the window has focus. */
export const PORTS_EVERY_MS = 15_000;

function PortsSegment() {
  const [ports, setPorts] = useState<ListeningPort[]>([]);
  useEffect(() => {
    let live = true;
    const read = () =>
      void invoke<ListeningPort[]>("ports_list")
        .then((list) => live && setPorts(list ?? []))
        .catch(() => undefined);
    read();
    const timer = setInterval(() => {
      if (typeof document === "undefined" || document.hasFocus()) read();
    }, PORTS_EVERY_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, []);
  if (ports.length === 0) return null;
  const only = ports.length === 1 ? ports[0] : undefined;
  return (
    <button
      type="button"
      data-testid="status-ports"
      onClick={() => useSidebar.getState().openTab("ports")}
      className={SEGMENT}
    >
      <Plug size={11} aria-hidden="true" />
      {only ? t("status.port", { port: only.port }) : t("status.ports", { n: ports.length })}
    </button>
  );
}

/** "Dictate: Ctrl+Alt" (MILESTONES M8): opens Settings › Dictate. */
function DictateSegment() {
  const status = useDictate((s) => s.status);
  const take = useDictate((s) => s.take);
  if (!status?.enabled) return null;
  const listening = take?.state === "listening";
  return (
    <button
      type="button"
      data-testid="status-dictate"
      onClick={() => useUi.getState().openSettings("dictate")}
      className={cx(
        "inline-flex items-center gap-1 rounded-xs hover:text-fg focus-visible:shadow-focus focus-visible:outline-none",
        listening && "text-accent",
      )}
    >
      <Mic size={11} aria-hidden="true" />
      {status.model || status.engine === "cloud"
        ? t("status.dictate", { key: status.hold.label })
        : t("status.dictateNoModel")}
    </button>
  );
}
