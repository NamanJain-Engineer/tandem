import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import {
  AlertTriangle,
  Cloud,
  Folder,
  GitBranch,
  Globe,
  Plus,
  Smartphone,
  SquareTerminal,
} from "lucide-react";
import type { SavedSession, WorkspaceSummary } from "@tandem/protocol";
import {
  Badge,
  Button,
  ConfirmDialog,
  Count,
  EngineGlyph,
  IconButton,
  StatusDot,
  cx,
  useToast,
  type MenuEntry,
} from "@tandem/ui";
import { t } from "../../i18n";
import { copyText } from "../../ipc/clipboard";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { useLayout } from "../../stores/layout";
import { useSetting, useSettings } from "../../stores/settings";
import { paneKey, statusView, useStatus } from "../../stores/status";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { isMultiRepo } from "../../stores/launch";
import { useRemote } from "../../stores/remote";
import { RailList } from "../rail/RailList";
import { topLevel, useWorktrees, worktreesOf } from "../../stores/worktrees";
import { useChecksPolling } from "./PullRequestChecks";
import { useIntentRadar } from "./IntentRadar";
import { WorktreeRow, usePreviewPolling } from "./Worktrees";
import { browserSources, paneTitles } from "./titles";
import { useBrowser } from "../../stores/browser";
import { pickAndAddWorkspace, switchMode } from "../shell/actions";
import { useBoot } from "../shell/useBoot";

export const RAIL_MIN = 180;
export const RAIL_MAX = 360;
/** Dragging the edge below this collapses the rail to icons (UI_SPEC §3.3). */
const COLLAPSE_BELOW = 160;

/** The rail's right edge: drag or arrow keys resize it (180–360 px); the
 * width persists (F-SH-03). */
function RailResizer({ width, onWidth }: { width: number; onWidth: (w: number, done: boolean) => void }) {
  const start = useRef<{ x: number; w: number } | null>(null);
  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    start.current = { x: e.clientX, w: width };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const next = (e: ReactPointerEvent<HTMLDivElement>) =>
    start.current ? start.current.w + e.clientX - start.current.x : width;
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={t("rail.resize")}
      aria-valuemin={RAIL_MIN}
      aria-valuemax={RAIL_MAX}
      aria-valuenow={width}
      tabIndex={0}
      data-testid="rail-resizer"
      onPointerDown={onPointerDown}
      onPointerMove={(e) => start.current && onWidth(next(e), false)}
      onPointerUp={(e) => {
        if (!start.current) return;
        onWidth(next(e), true);
        start.current = null;
      }}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
          e.preventDefault();
          onWidth(width + (e.key === "ArrowRight" ? 8 : -8), true);
        }
      }}
      className="absolute inset-y-0 -right-1 z-10 w-2 cursor-col-resize outline-none after:absolute after:inset-y-0 after:left-[3px] after:w-0.5 after:bg-transparent hover:after:bg-accent focus-visible:after:bg-accent"
    />
  );
}

/** A workspace's Saved sessions count (agents that handed back, F-CD-03). */
function useSavedCount(workspaceId: string): number {
  const [count, setCount] = useState(0);
  useEffect(() => {
    let live = true;
    const load = () =>
      void invoke<SavedSession[]>("sessions_saved_list", { workspaceId })
        .then((list) => live && setCount(list?.length ?? 0))
        .catch(() => undefined);
    load();
    let off: (() => void) | undefined;
    void listen<{ workspaceId: string }>(
      "sessions:saved",
      (e) => e.workspaceId === workspaceId && load(),
    ).then((f) => {
      if (live) off = f;
      else f();
    });
    return () => {
      live = false;
      off?.();
    };
  }, [workspaceId]);
  return count;
}

function SessionDot({ paneId }: { paneId: string }) {
  const status = useStatus((st) => st.records[paneKey(paneId)]?.status);
  const unread = useStatus((st) => st.unread[paneKey(paneId)] ?? false);
  return (
    <span
      className="relative inline-flex shrink-0"
      data-testid="rail-session-status"
      data-status={status ?? "idle"}
    >
      <StatusDot status={status ? statusView(status) : "idle"} size={6} />
      {unread && (
        <span
          data-testid="unread-dot"
          aria-label={t("rail.unread")}
          className="absolute -right-1 -top-1 size-1.5 rounded-full bg-accent"
        />
      )}
    </span>
  );
}

/** The right slot of a workspace row (F-SH-03): n working, Needs you,
 * Failed, or nothing; and the unread dot when something finished unseen. */
function useWorkspaceStatus(ws: WorkspaceSummary): {
  working: number;
  waiting: boolean;
  failed: boolean;
  unread: boolean;
} {
  const tabs = useLayout((s) => s.tabs);
  const records = useStatus((s) => s.records);
  const unread = useStatus((s) => s.unread);
  return useMemo(() => {
    const keys = ws.tabs.flatMap((tab) => Object.keys(tabs[tab.id]?.panes ?? {})).map(paneKey);
    const statuses = keys.map((k) => records[k]?.status);
    return {
      working: statuses.filter((x) => x === "working").length,
      waiting: statuses.includes("waiting"),
      failed: statuses.includes("failed"),
      unread: keys.some((k) => unread[k]),
    };
  }, [ws.tabs, tabs, records, unread]);
}

/** Quiet, never a box (D-2603): amber "Needs you", a working dot and its
 * count, or "Failed". */
function WorkspaceSlot({ ws }: { ws: WorkspaceSummary }) {
  const s = useWorkspaceStatus(ws);
  const slot = s.waiting ? (
    <Badge variant="waiting" quiet>
      {t("rail.waiting")}
    </Badge>
  ) : s.working > 0 ? (
    <span className="inline-flex items-center gap-1 text-xs tabular-nums text-fg-muted">
      <span aria-hidden="true" className="inline-flex">
        <StatusDot status="working" size={6} />
      </span>
      {t("rail.working", { n: s.working })}
    </span>
  ) : s.failed ? (
    <Badge variant="error" quiet>
      {t("rail.failed")}
    </Badge>
  ) : null;
  return (
    <>
      {slot && (
        <span data-testid="workspace-status" className="inline-flex shrink-0">
          {slot}
        </span>
      )}
      {s.unread && (
        <span
          data-testid="workspace-unread"
          aria-label={t("rail.unread")}
          className="size-1.5 shrink-0 rounded-full bg-accent"
        />
      )}
    </>
  );
}

/** The row's branch, while nothing needs saying about its agents: the
 * status slot wins the room, so the name never truncates (D-2603). */
function BranchChip({
  ws,
  selected,
  testId,
  children,
}: {
  ws: WorkspaceSummary;
  selected: boolean;
  testId?: string;
  /** Instead of the branch (a multi-repo workspace's repository count). */
  children?: ReactNode;
}) {
  const s = useWorkspaceStatus(ws);
  if (s.waiting || s.working > 0 || s.failed) return null;
  return (
    <span
      data-testid={testId}
      className={cx(
        "inline-flex min-w-0 max-w-[84px] shrink items-center gap-0.5 truncate text-xs",
        // fg-subtle fails AA on the selected row's accent-soft (D-705).
        selected ? "text-fg-muted" : "text-fg-subtle",
      )}
    >
      <GitBranch size={12} aria-hidden="true" />
      <span className="truncate">{children ?? ws.branch}</span>
    </span>
  );
}

/** Open panes of a workspace, under its expanded rail row. */
function WorkspaceSessions({ ws }: { ws: WorkspaceSummary }) {
  const savedCount = useSavedCount(ws.id);
  const engines = useEngines((st) => st.list);
  const tabs = useLayout((s) => s.tabs);
  const pages = useBrowser((s) => s.pages);
  const loaded = useLayout((s) => s.loaded[ws.id] ?? false);
  useEffect(() => {
    if (!loaded)
      void useLayout
        .getState()
        .load(ws.id)
        .catch(() => undefined);
  }, [loaded, ws.id]);
  const rows = ws.tabs.flatMap((tab) => {
    const t2 = tabs[tab.id];
    if (!t2?.tree) return [];
    const names = paneTitles(t2.panes, engines, browserSources(t2.panes, pages), ws.name);
    return Object.values(t2.panes)
      .sort((a, b) => a.createdAt - b.createdAt)
      .map((p) => ({ tab, pane: p, title: names[p.id] ?? "" }));
  });
  // Level-2 rows of the rail tree (a tree may only own treeitems and groups).
  const saved =
    savedCount > 0 ? (
      <div
        role="treeitem"
        aria-level={2}
        aria-selected={false}
        data-testid="saved-sessions"
        className="px-2 py-1 text-sm text-fg-muted"
      >
        {t("rail.savedSessions", { n: savedCount })}
      </div>
    ) : null;
  if (rows.length === 0)
    return (
      <>
        <div
          role="treeitem"
          aria-level={2}
          aria-disabled="true"
          aria-selected={false}
          className="px-2 py-1 text-sm text-fg-subtle"
        >
          {t("rail.noSessions")}
        </div>
        {saved}
      </>
    );
  const open = (tabId: string, paneId: string) => {
    const store = useWorkspaces.getState();
    store.select(ws.id);
    store.selectTab(ws.id, tabId);
    useLayout.getState().focus(tabId, paneId);
    switchMode("code");
  };
  return (
    <>
      {rows.map(({ tab, pane, title }) => (
        <div
          key={pane.id}
          role="treeitem"
          aria-level={2}
          aria-selected={false}
          tabIndex={-1}
          data-testid="rail-session"
          onClick={() => open(tab.id, pane.id)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              open(tab.id, pane.id);
            }
          }}
          className="flex h-6 w-full cursor-default items-center gap-1.5 rounded-sm px-2 text-left text-sm text-fg-muted outline-none hover:bg-surface-4 hover:text-fg focus-visible:shadow-focus"
        >
          <SessionDot paneId={pane.id} />
          {pane.kind === "agent" ? (
            <EngineGlyph
              letters={engines.find((e) => e.spec.id === pane.engineId)?.spec.logo ?? ""}
              engine={pane.engineId ?? undefined}
              size={14}
            />
          ) : pane.kind === "browser" ? (
            <Globe size={12} aria-hidden="true" className="shrink-0" />
          ) : pane.kind === "simulator" ? (
            <Smartphone size={12} aria-hidden="true" className="shrink-0" />
          ) : (
            <SquareTerminal size={12} aria-hidden="true" className="shrink-0" />
          )}
          <span className="min-w-0 flex-1 truncate">{title}</span>
        </div>
      ))}
      {saved}
    </>
  );
}

/** The host a remote workspace lives on, as the rail names it. */
function hostLabel(ws: WorkspaceSummary): string {
  if (ws.kind === "wsl") return ws.wslDistro ?? ws.hostId;
  return useRemote.getState().hostOf(ws.hostId)?.label ?? ws.hostId;
}

/** "Remote · build box" or "WSL · Ubuntu". */
function placeBadge(ws: WorkspaceSummary): string {
  return t(ws.kind === "wsl" ? "rail.wslBadge" : "rail.remoteBadge", { host: hostLabel(ws) });
}

/** UI_SPEC §3.3 Code rail: WORKSPACES + "+", the shared RailList, footer. */
export function CodeRail() {
  const { list, activeId, select, remove, move, rename, setCollapsed, confirmRemove, askRemove } =
    useWorkspaces();
  const collapsed = useUi((s) => s.railCollapsed);
  const platform = useBoot((s) => s.boot?.platform ?? "linux");
  const saved = useSetting("rail_width", 248);
  const [width, setWidth] = useState<number | null>(null);
  const { show } = useToast();
  const shown = Math.min(RAIL_MAX, Math.max(RAIL_MIN, width ?? saved));
  const localWorktrees = list
    .filter((w) => w.parentId !== null && w.laneBranch !== null && w.kind === "local" && !w.missing)
    .map((w) => w.id);
  // F-WT-09: whether each local worktree's preview answers, while the rail shows.
  usePreviewPolling(localWorktrees, !collapsed);
  // F-WT-08: their pull requests' checks (no gh for a repository not allowed).
  useChecksPolling(localWorktrees, !collapsed);
  // F-WT-10: radar marks, and the notice when two worktrees newly share symbols.
  useIntentRadar(localWorktrees, !collapsed);

  const onWidth = (w: number, done: boolean) => {
    if (done && w < COLLAPSE_BELOW) {
      setWidth(null);
      if (!useUi.getState().railCollapsed) useUi.getState().toggleRail();
      return;
    }
    const clamped = Math.round(Math.min(RAIL_MAX, Math.max(RAIL_MIN, w)));
    setWidth(done ? null : clamped);
    if (done) void useSettings.getState().set("rail_width", clamped);
  };

  const menu = (
    ws: WorkspaceSummary,
    a: { rename: () => void; moveUp: () => void; moveDown: () => void; togglePin: () => void },
  ): MenuEntry[] => [
    { id: "pin", label: ws.pinned ? t("rail.unpin") : t("rail.pin"), onSelect: a.togglePin },
    { id: "rename", label: t("rail.rename"), kbd: ["F2"], onSelect: a.rename },
    {
      // Only a repository can have worktrees; the label says why it's off.
      id: "new-worktree",
      label:
        ws.kind === "wsl"
          ? t("worktree.remote")
          : ws.branch || isMultiRepo(ws)
            ? t("worktree.new")
            : t("worktree.needsRepo"),
      disabled: ws.kind === "wsl" || (!ws.branch && !isMultiRepo(ws)) || ws.missing,
      onSelect: () => useWorktrees.getState().open({ kind: "new", workspaceId: ws.id }),
    },
    {
      // F-WT-01: what every new worktree of this repository gets.
      id: "worktree-setup",
      label: t("worktree.setup.menu"),
      disabled: ws.kind !== "local" || !ws.branch || ws.missing,
      onSelect: () => useWorktrees.getState().open({ kind: "setup", workspaceId: ws.id }),
    },
    {
      // F-WT-07: land this repository's worktrees one after another.
      id: "land-worktrees",
      label: t("landing.menu"),
      disabled: ws.kind !== "local" || !ws.branch || ws.missing,
      onSelect: () => useWorktrees.getState().open({ kind: "land", workspaceId: ws.id }),
    },
    {
      // F-EN-07: which instructions file each engine reads here.
      id: "instructions",
      label: t("instructions.menu"),
      disabled: ws.kind !== "local" || ws.missing,
      onSelect: () => useWorktrees.getState().open({ kind: "instructions", workspaceId: ws.id }),
    },
    { id: "up", label: t("rail.moveUp"), onSelect: a.moveUp },
    { id: "down", label: t("rail.moveDown"), onSelect: a.moveDown },
    { id: "sep", type: "separator" },
    // An SSH host's folder isn't on this computer to show (F-RM-01).
    ...(ws.kind === "ssh"
      ? []
      : [
          {
            id: "reveal",
            label: platform === "macos" ? t("rail.revealMac") : t("rail.reveal"),
            disabled: ws.missing,
            onSelect: () =>
              void invoke("workspace_reveal", { id: ws.id }).catch((e: { message?: string }) =>
                show({ tone: "error", title: e.message ?? "" }),
              ),
          },
        ]),
    {
      id: "copy",
      label: t("rail.copyPath"),
      onSelect: () =>
        void copyText(ws.root).then((ok) => ok && show({ tone: "success", title: t("rail.pathCopied") })),
    },
    { id: "remove", label: t("rail.remove"), danger: true, onSelect: () => askRemove(ws) },
  ];

  return (
    <nav
      aria-label={t("rail.workspaces")}
      className="relative flex h-full shrink-0 flex-col border-r border-border bg-surface-1"
      style={{ width: collapsed ? "var(--rail-collapsed)" : shown }}
    >
      <div className={cx("flex h-9 items-center gap-1 px-2", collapsed && "justify-center")}>
        {!collapsed && (
          <h2 className="flex-1 px-1 text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
            {t("rail.workspacesHeader")}
          </h2>
        )}
        <IconButton
          size="sm"
          icon={<Plus size={14} />}
          label={t("rail.addFolder")}
          onClick={() => void pickAndAddWorkspace(show)}
        />
      </div>
      <RailList
        label={t("rail.workspaces")}
        items={topLevel(list)}
        selectedId={activeId}
        collapsed={collapsed}
        expanded={Object.fromEntries(list.map((w) => [w.id, !w.collapsed]))}
        onExpand={(ws, open) => setCollapsed(ws.id, !open)}
        row={(ws) => ({
          testId: "workspace-row",
          data: { root: ws.root },
          name: ws.name,
          tooltip: ws.missing
            ? t("rail.missing")
            : ws.kind !== "local"
              ? `${placeBadge(ws)} · ${ws.root}`
              : ws.reposNote
                ? `${ws.root} · ${ws.reposNote}`
                : ws.root,
          warning: ws.missing,
          icon: ws.missing ? (
            <AlertTriangle size={14} aria-label={t("rail.missing")} className="text-warning" />
          ) : ws.kind !== "local" ? (
            <Cloud size={14} aria-label={placeBadge(ws)} className="text-fg-muted" />
          ) : (
            <Folder size={14} aria-hidden="true" className="text-fg-muted" />
          ),
          trailing: (
            <>
              {isMultiRepo(ws) ? (
                <BranchChip ws={ws} selected={ws.id === activeId} testId="workspace-repos">
                  {t("worktree.multi.count", { n: ws.repos?.length ?? 0 })}
                </BranchChip>
              ) : (
                ws.branch && <BranchChip ws={ws} selected={ws.id === activeId} />
              )}
              <WorkspaceSlot ws={ws} />
              {ws.liveCount > 0 && (
                <Count value={ws.liveCount} label={t("rail.live", { n: ws.liveCount })} />
              )}
            </>
          ),
          children: (
            <>
              <WorkspaceSessions ws={ws} />
              {worktreesOf(list, ws.id).map((wt) => (
                <WorktreeRow
                  key={wt.id}
                  wt={wt}
                  base={ws.branch ?? "HEAD"}
                  slot={<WorkspaceSlot ws={wt} />}
                />
              ))}
            </>
          ),
        })}
        menu={menu}
        onOpen={(ws) => select(ws.id)}
        onMove={(id, afterId, pinned) => void move(id, afterId, pinned)}
        onRename={(ws, name) =>
          void rename(ws.id, name).then((err) => err && show({ tone: "error", title: err.message }))
        }
        onRemove={(ws) => askRemove(ws)}
      />
      {!collapsed && (
        <div className="border-t border-divider px-1.5 pt-1.5">
          <Button
            variant="ghost"
            size="sm"
            fullWidth
            data-testid="rail-remote-hosts"
            iconLeft={<Cloud size={14} />}
            onClick={() => useUi.getState().openSettings("remote")}
          >
            {t("rail.remoteHosts")}
          </Button>
        </div>
      )}
      {!collapsed && <RailResizer width={shown} onWidth={onWidth} />}
      <ConfirmDialog
        open={confirmRemove !== null}
        onOpenChange={(o) => !o && askRemove(null)}
        title={t("code.remove.title", { name: confirmRemove?.name ?? "" })}
        body={t("code.remove.body")}
        confirmLabel={t("code.remove.confirm")}
        onConfirm={() => {
          if (!confirmRemove) return;
          const name = confirmRemove.name;
          void remove(confirmRemove.id).then(() => show({ title: t("code.removed", { name }) }));
        }}
      />
    </nav>
  );
}
