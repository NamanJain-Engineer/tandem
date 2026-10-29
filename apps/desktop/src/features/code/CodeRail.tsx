import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  type RefObject,
} from "react";
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
import { isJj, topLevel, useWorktrees, worktreesOf } from "../../stores/worktrees";
import { useChecksPolling } from "./PullRequestChecks";
import { useIntentRadar } from "./IntentRadar";
import { useMarkFolds } from "./markFolds";
import { WorktreeRow, usePreviewPolling } from "./Worktrees";
import { WorkspaceSlot } from "./WorkspaceSlot";
import { showsStatus, useWorkspaceStatus } from "./workspaceStatus";
import { browserSources, paneTitles } from "./titles";
import { useBrowser } from "../../stores/browser";
import { pickAndAddWorkspace, switchMode } from "../shell/actions";
import { useBoot } from "../shell/useBoot";

/** D-2701: the rail is never narrower than its rows. Its minimum is a fully
 * folded worktree row at the deepest indent: the rail's border (1), the
 * list's padding (6 + 6), a worktree's indent (24) and the row's padding
 * (8 + 8); in the row, the branch icon (12), the status dot (6), the checks
 * badge's icon (20), the radar mark's (24) and the five gaps between the
 * parts (6 each); then its text parts, `RailParts`. railFold.test.tsx holds
 * the row to those parts. */
const RAIL_FIXED = 1 + 12 + 24 + 16 + 12 + 6 + 20 + 24 + 5 * 6;
/** The widest count a fully folded row allows for: two digits. */
const WIDEST_COUNT = 88;
/** The gap before a part a row adds (D-2703), and the Preview's globe folded. */
const GAP = 6;
const GLOBE = 20;

/** A fully folded row's text parts as an engine draws them: the name's 10
 * characters, a two-digit count and F-CD-24's jj badge. */
export type RailParts = { name: number; count: number; jj: number };
/** Chromium's, in Inter at 12 px (UI_SPEC §1.2): what the rail uses until
 * this engine's are measured, and where nothing is laid out (tests). */
export const PARTS: RailParts = { name: 75.7, count: 24.9, jj: 19 };

/** D-2701, D-2703: the rail's minimum: a fully folded worktree row, its
 * text parts as measured, and the widest of what the rows it shows add (a
 * jj badge, a Preview's globe, each with its gap), with a scrollbar's
 * width where scrollbars take room. */
export function railMin(
  parts: RailParts,
  rows: Array<{ jj: boolean; preview: boolean }>,
  scrollbar = 0,
): number {
  const more = Math.max(0, ...rows.map((r) => (r.jj ? parts.jj + GAP : 0) + (r.preview ? GLOBE + GAP : 0)));
  return Math.ceil(RAIL_FIXED + parts.name + parts.count + more) + scrollbar;
}

/** The minimum in Chromium with no jj or Preview row: 246. */
export const RAIL_MIN = railMin(PARTS, []);
export const RAIL_MAX = 360;
/** Dragging the edge below this collapses the rail to icons (UI_SPEC §3.3). */
const COLLAPSE_BELOW = 160;

/** The room a scrollbar takes from the rail's rows: none where scrollbars
 * float over the content (macOS, most Linux desktops), a thin one's width
 * where they don't (Windows); the rail's minimum adds it (D-2701). */
function useScrollbarWidth(): number {
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const probe = document.createElement("div");
    probe.style.cssText = "position:absolute;visibility:hidden;width:100px;height:100px;overflow-y:scroll";
    document.body.appendChild(probe);
    setWidth(probe.offsetWidth - probe.clientWidth);
    probe.remove();
  }, []);
  return width;
}

/** D-2701: the rail's text parts as this engine draws them, measured from
 * hidden copies once its fonts are in and the rail is open (WebKitGTK draws
 * a name's 10 characters 80 px wide where Chromium draws 75.7). */
function useRailParts(probe: RefObject<HTMLElement | null>, open: boolean): RailParts {
  const [parts, setParts] = useState(PARTS);
  useLayoutEffect(() => {
    if (!open) return;
    let live = true;
    const width = (part: string) =>
      probe.current?.querySelector(`[data-part="${part}"]`)?.getBoundingClientRect().width ?? 0;
    void (document.fonts?.ready ?? Promise.resolve()).then(() => {
      if (!live) return;
      const next = { name: width("name"), count: width("count"), jj: width("jj") };
      // Nothing laid out: Chromium's stand.
      if (next.name > 0 && next.count > 0 && next.jj > 0) setParts(next);
    });
    return () => {
      live = false;
    };
  }, [probe, open]);
  return parts;
}

/** D-2703: what each worktree row the rail shows adds to a fully folded
 * row: F-CD-24's jj badge, a running Preview's globe. Rows under a
 * collapsed repository, or in a folded Pinned band, aren't shown. */
function useShownMarks(
  list: WorkspaceSummary[],
  pinnedOpen: boolean,
): Array<{ jj: boolean; preview: boolean }> {
  const previews = useWorktrees((s) => s.previews);
  return topLevel(list)
    .filter((ws) => !ws.collapsed && (pinnedOpen || !ws.pinned))
    .flatMap((ws) => worktreesOf(list, ws.id))
    .map((wt) => ({
      jj: isJj(wt),
      preview: previews[wt.id]?.status === "ready" && Boolean(previews[wt.id]?.url),
    }));
}

/** The rail's right edge: drag or arrow keys resize it (its minimum to 360
 * px); the width persists (F-SH-03). */
function RailResizer({
  width,
  min,
  onWidth,
}: {
  width: number;
  min: number;
  onWidth: (w: number, done: boolean) => void;
}) {
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
      aria-valuemin={min}
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
          role="img"
          aria-label={t("rail.unread")}
          className="absolute -right-1 -top-1 size-1.5 rounded-full bg-accent"
        />
      )}
    </span>
  );
}

/** A workspace row's end: its branch (or repository count), its status and
 * its live panes. The status words fold to their dot while the row is too
 * narrow for them (D-2701), so the row never runs past the rail. */
function WorkspaceTrailing({ ws, selected }: { ws: WorkspaceSummary; selected: boolean }) {
  // Read once for the row's parts: beside the status the branch gives way.
  const s = useWorkspaceStatus(ws);
  const beside = showsStatus(s);
  const { ref, folds } = useMarkFolds<HTMLSpanElement>(
    JSON.stringify([ws.name, s, ws.branch, ws.repos?.length ?? null, ws.liveCount]),
    1,
    (el) => el.closest<HTMLElement>('[role="treeitem"]'),
  );
  return (
    <span
      ref={ref}
      data-testid="workspace-trailing"
      className="flex min-w-0 items-center justify-end gap-1.5"
    >
      {beside ? null : isMultiRepo(ws) ? (
        <BranchChip ws={ws} selected={selected} testId="workspace-repos">
          {t("worktree.multi.count", { n: ws.repos?.length ?? 0 })}
        </BranchChip>
      ) : (
        ws.branch && <BranchChip ws={ws} selected={selected} />
      )}
      <WorkspaceSlot status={s} fold={folds >= 1} />
      {ws.liveCount > 0 && <Count value={ws.liveCount} label={t("rail.live", { n: ws.liveCount })} />}
    </span>
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
          data-pane={pane.id}
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
  const read = useSettings((s) => s.loaded);
  // While the edge is dragged: its width, not yet saved.
  const [width, setWidth] = useState<number | null>(null);
  const dragging = width !== null;
  const { show } = useToast();
  const probe = useRef<HTMLDivElement | null>(null);
  // The Pinned band's fold: kept here, as the minimum counts only the rows shown.
  const [pinnedOpen, setPinnedOpen] = useState(true);
  const min = railMin(useRailParts(probe, !collapsed), useShownMarks(list, pinnedOpen), useScrollbarWidth());
  const shown = Math.min(RAIL_MAX, Math.max(min, width ?? saved));
  // D-2703: a rail narrower than its minimum widens to it, and keeps that
  // width when the marks that asked for it go; never while it's dragged,
  // nor over a saved width the settings couldn't read.
  useEffect(() => {
    if (read && !dragging && !collapsed && saved < min) void useSettings.getState().set("rail_width", min);
  }, [read, dragging, collapsed, saved, min]);
  // Its width eases once it has drawn: not while it opens or collapses,
  // nor mid-drag, when the edge follows the pointer.
  const [drawn, setDrawn] = useState(false);
  useEffect(() => {
    if (collapsed) {
      setDrawn(false);
      return;
    }
    const frame = requestAnimationFrame(() => setDrawn(true));
    return () => cancelAnimationFrame(frame);
  }, [collapsed]);
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
    const clamped = Math.round(Math.min(RAIL_MAX, Math.max(min, w)));
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
      className={cx(
        "relative flex h-full shrink-0 flex-col border-r border-border bg-surface-1",
        drawn && !dragging && "transition-[width] duration-[var(--dur-base)] ease-[var(--ease-standard)]",
      )}
      style={{ width: collapsed ? "var(--rail-collapsed)" : shown }}
    >
      {/* The text parts of the rail's minimum, measured in its own CSS. */}
      {!collapsed && (
        <div
          ref={probe}
          aria-hidden="true"
          className="pointer-events-none invisible absolute left-0 top-0 flex text-sm"
        >
          <span data-part="name" className="block w-[10ch] shrink-0" />
          <span data-part="count" className="shrink-0">
            <Badge>{WIDEST_COUNT}</Badge>
          </span>
          <span data-part="jj" className="shrink-0">
            <Badge variant="outline">{t("worktree.jj.badge")}</Badge>
          </span>
        </div>
      )}
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
        pinnedOpen={pinnedOpen}
        onPinnedOpen={setPinnedOpen}
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
          trailing: <WorkspaceTrailing ws={ws} selected={ws.id === activeId} />,
          children: (
            <>
              <WorkspaceSessions ws={ws} />
              {worktreesOf(list, ws.id).map((wt) => (
                <WorktreeRow key={wt.id} wt={wt} base={ws.branch ?? "HEAD"} />
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
      {!collapsed && <RailResizer width={shown} min={min} onWidth={onWidth} />}
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
