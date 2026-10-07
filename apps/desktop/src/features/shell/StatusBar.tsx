import { GitBranch } from "lucide-react";
import { t } from "../../i18n";
import { useSession } from "../../stores/session";
import { useWorkspaces } from "../../stores/workspaces";
import { useBoot } from "./useBoot";

/** UI §3.6 (M2): branch of the focused workspace, connection and version. */
export function StatusBar() {
  const version = useBoot((s) => s.boot?.version ?? "");
  const online = useSession((s) => s.entitlement?.online ?? true);
  const branch = useWorkspaces((s) => s.list.find((w) => w.id === s.activeId)?.branch ?? null);
  return (
    <footer
      role="contentinfo"
      aria-label={t("status.label")}
      className="flex h-[var(--statusbar-h)] shrink-0 items-center gap-3 border-t border-divider bg-surface-1 px-3 text-2xs text-fg-muted"
    >
      <span className="inline-flex items-center gap-1">
        <GitBranch size={11} aria-hidden="true" />
        {branch ?? t("status.noBranch")}
      </span>
      <span className="flex-1" />
      <span data-testid="connection-state" className={online ? undefined : "text-warning"}>
        {online ? t("status.online") : t("status.offline")}
      </span>
      <span className="tabular-nums">{t("status.version", { version })}</span>
    </footer>
  );
}
