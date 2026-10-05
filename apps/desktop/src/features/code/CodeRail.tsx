import { useRef, useState, type KeyboardEvent } from "react";
import { AlertTriangle, Folder, GitBranch, Plus } from "lucide-react";
import { Badge, ConfirmDialog, ContextMenu, IconButton, Tooltip, cx, useToast } from "@tandem/ui";
import type { WorkspaceSummary } from "@tandem/protocol";
import { t } from "../../i18n";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { pickAndAddWorkspace } from "../shell/actions";
import { PlanFooter } from "../shell/PlanFooter";

/** Code rail (F-SH-03 slice): WORKSPACES + "+", the list, plan footer. */
export function CodeRail() {
  const { list, activeId, select, remove } = useWorkspaces();
  const collapsed = useUi((s) => s.railCollapsed);
  const { show } = useToast();
  const [removing, setRemoving] = useState<WorkspaceSummary | null>(null);
  const rows = useRef<Array<HTMLDivElement | null>>([]);

  const onKey = (e: KeyboardEvent, index: number) => {
    const move = (to: number) => {
      e.preventDefault();
      rows.current[Math.max(0, Math.min(list.length - 1, to))]?.focus();
    };
    if (e.key === "ArrowDown") move(index + 1);
    else if (e.key === "ArrowUp") move(index - 1);
    else if (e.key === "Home") move(0);
    else if (e.key === "End") move(list.length - 1);
    else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      const ws = list[index];
      if (ws) select(ws.id);
    } else if (e.key === "Delete") {
      const ws = list[index];
      if (ws) setRemoving(ws);
    }
  };

  return (
    <nav aria-label={t("rail.workspaces")} className={cx("flex h-full shrink-0 flex-col border-r border-border bg-surface-1", collapsed ? "w-[var(--rail-collapsed)]" : "w-[var(--rail-w)]")}>
      <div className={cx("flex h-9 items-center gap-1 px-2", collapsed && "justify-center")}>
        {!collapsed && <h2 className="flex-1 px-1 text-2xs font-semibold uppercase tracking-caps text-fg-subtle">{t("rail.workspacesHeader")}</h2>}
        <IconButton size="sm" icon={<Plus size={14} />} label={t("rail.addFolder")} onClick={() => void pickAndAddWorkspace(show)} />
      </div>
      <div role="tree" aria-label={t("rail.workspaces")} className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2">
        {list.map((ws, i) => {
          const selected = ws.id === activeId;
          const row = (
            <div
              ref={(el) => {
                rows.current[i] = el;
              }}
              role="treeitem"
              aria-level={1}
              aria-selected={selected}
              tabIndex={selected || (!activeId && i === 0) ? 0 : -1}
              data-testid="workspace-row"
              data-root={ws.root}
              onClick={() => select(ws.id)}
              onKeyDown={(e) => onKey(e, i)}
              title={ws.root}
              className={cx(
                "group flex h-[30px] cursor-default select-none items-center gap-1.5 rounded-sm px-2 text-base text-fg outline-none hover:bg-surface-4 focus-visible:shadow-focus",
                selected && "bg-accent-soft hover:bg-accent-soft",
                collapsed && "justify-center px-0",
              )}
            >
              {ws.missing ? (
                <AlertTriangle size={14} aria-label={t("rail.missing")} className="shrink-0 text-warning" />
              ) : (
                <Folder size={14} aria-hidden="true" className="shrink-0 text-fg-muted" />
              )}
              {!collapsed && (
                <>
                  <span className="min-w-0 flex-1 truncate">{ws.name}</span>
                  {ws.branch && (
                    <span className="inline-flex max-w-[84px] shrink-0 items-center gap-0.5 truncate text-sm text-fg-subtle">
                      <GitBranch size={12} aria-hidden="true" />
                      <span className="truncate">{ws.branch}</span>
                    </span>
                  )}
                  {ws.liveCount > 0 && (
                    <span aria-label={t("rail.live", { n: ws.liveCount })}>
                      <Badge>{ws.liveCount}</Badge>
                    </span>
                  )}
                </>
              )}
            </div>
          );
          return (
            <ContextMenu key={ws.id} entries={[{ id: "remove", label: t("rail.remove"), danger: true, onSelect: () => setRemoving(ws) }]}>
              {collapsed ? <Tooltip label={ws.name} side="right">{row}</Tooltip> : row}
            </ContextMenu>
          );
        })}
      </div>
      <PlanFooter collapsed={collapsed} />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(o) => !o && setRemoving(null)}
        title={t("code.remove.title", { name: removing?.name ?? "" })}
        body={t("code.remove.body")}
        confirmLabel={t("code.remove.confirm")}
        onConfirm={() => {
          if (!removing) return;
          const name = removing.name;
          void remove(removing.id).then(() => show({ title: t("code.removed", { name }) }));
        }}
      />
    </nav>
  );
}
