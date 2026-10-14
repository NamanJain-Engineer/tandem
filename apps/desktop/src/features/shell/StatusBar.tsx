import { GitBranch, Mic } from "lucide-react";
import { cx } from "@tandem/ui";
import { t } from "../../i18n";
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
 * with ahead/behind, which opens the Files tab's Changes view; the
 * connection and the version. An early-access build has no hosted service
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
      <span className="flex-1" />
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
