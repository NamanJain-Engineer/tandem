// The Ports tab's Panes section (F-EX-06): each live local pane's CPU and
// memory, grouped by worktree like the ports, with Freeze and Thaw.
import { Cpu } from "lucide-react";
import type { IpcError, PaneUsage } from "@tandem/protocol";
import { Badge, Banner, Button, EmptyState, Skeleton, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { formatBytes, formatCpu, setFrozen, usePaneUsage, useResources } from "../../stores/resources";
import { focusPane, groupByWorktree } from "./PortsPanel";

function memory(u: PaneUsage): string {
  if (u.memoryBytes === null) return "—";
  const used = formatBytes(u.memoryBytes);
  return u.memoryCapBytes === null
    ? used
    : t("resources.memory.capped", { used, cap: formatBytes(u.memoryCapBytes) });
}

export function PaneUsageSection({ visible }: { visible: boolean }) {
  const { load, refresh } = usePaneUsage(visible);
  const freezing = useResources((s) => s.status?.freezing ?? false);
  const notes = useResources((s) => s.byPane);
  const { show } = useToast();

  const toggle = (u: PaneUsage) =>
    void setFrozen(u.paneId, !u.frozen)
      .then(refresh)
      .catch((e: IpcError) =>
        show({
          tone: "error",
          title: u.frozen ? t("resources.thawFailed") : t("resources.freezeFailed"),
          body: e.message,
        }),
      );

  let body;
  if (load.at === "loading") {
    body = (
      <div className="space-y-2 p-3" data-testid="pane-usage-loading">
        <Skeleton className="h-8" />
      </div>
    );
  } else if (load.at === "error") {
    body = (
      <div className="p-3">
        <Banner tone="error" action={{ label: t("resources.panes.retry"), onClick: refresh }}>
          {t("resources.panes.error")}
        </Banner>
      </div>
    );
  } else if (load.panes.length === 0) {
    body = <EmptyState icon={<Cpu size={18} />} title={t("resources.panes.empty")} />;
  } else {
    body = (
      <ul className="p-1" aria-label={t("resources.panes.list")}>
        {groupByWorktree(load.panes).flatMap((group) => [
          ...(group.branch
            ? [
                <li
                  key={`group:${group.branch}`}
                  className="px-2 pb-0.5 pt-2 text-xs font-medium text-fg-subtle"
                  data-testid="pane-usage-group"
                  data-branch={group.branch}
                >
                  {t("ports.worktree", { branch: group.branch })}
                </li>,
              ]
            : []),
          ...group.ports.map((u) => {
            const frozen = notes[u.paneId]?.frozen ?? u.frozen;
            return (
              <li
                key={u.paneId}
                data-testid="pane-usage-row"
                data-pane={u.paneId}
                data-frozen={frozen ? "true" : "false"}
                className="flex min-w-0 items-center gap-2 rounded-sm px-2 py-1.5 hover:bg-surface-2"
              >
                <button
                  type="button"
                  className="min-w-0 flex-1 truncate text-left text-sm text-fg underline-offset-2 hover:underline focus-visible:shadow-focus focus-visible:outline-none"
                  onClick={() => focusPane(u.paneId, u.workspaceId)}
                >
                  {[u.paneName ?? t("resources.pane"), u.workspaceName].filter(Boolean).join(" · ")}
                </button>
                <span className="shrink-0 text-sm tabular-nums text-fg-muted" data-testid="pane-usage-cpu">
                  {t("resources.cpu", { value: formatCpu(u.cpuPercent) })}
                </span>
                <span className="shrink-0 text-sm tabular-nums text-fg-muted" data-testid="pane-usage-memory">
                  {memory(u)}
                </span>
                {frozen && <Badge variant="outline">{t("resources.chip.frozen")}</Badge>}
                {freezing && (
                  <Button
                    size="sm"
                    variant="ghost"
                    data-testid="pane-usage-freeze"
                    onClick={() => toggle({ ...u, frozen })}
                  >
                    {frozen ? t("resources.thaw") : t("resources.freeze")}
                  </Button>
                )}
              </li>
            );
          }),
        ])}
      </ul>
    );
  }

  return (
    <section className="border-b border-divider" data-testid="pane-usage">
      <h3 className="px-3 pt-2 text-xs font-medium uppercase tracking-caps text-fg-subtle">
        {t("resources.panes")}
      </h3>
      {body}
    </section>
  );
}
