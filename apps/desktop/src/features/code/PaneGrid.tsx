import { AutoChip } from "../auto/AutoChip";
import { useSession } from "../../stores/session";
import { useAuto } from "../../stores/auto";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useDraggable } from "@dnd-kit/core";
import {
  Panel,
  PanelGroup,
  PanelResizeHandle,
  type ImperativePanelGroupHandle,
} from "react-resizable-panels";
import type { IpcError, PaneRecord, WorkspaceSummary } from "@tandem/protocol";
import { ArrowRightLeft, GitBranch, Layers, Play, SquareTerminal, X } from "lucide-react";
import {
  Badge,
  Button,
  ContextMeter,
  EngineGlyph,
  IconButton,
  Spinner,
  StatusDot,
  Tooltip,
  cx,
  useToast,
  type MenuEntry,
  type Status,
} from "@tandem/ui";
import { t } from "../../i18n";
import { useEngines } from "../../stores/engines";
import { paneKey, statusView, useStatus } from "../../stores/status";
import { useTerminals } from "../../stores/terminals";
import { useBoot } from "../shell/useBoot";
import { useCloseConfirm } from "../../stores/closeConfirm";
import { useFlash } from "../../stores/focus";
import { useWorkspaces } from "../../stores/workspaces";
import { useSetting } from "../../stores/settings";
import { useLaunch } from "../../stores/launch";
import { useRestore, waitingOnCard } from "../../stores/restore";
import { invoke } from "../../ipc/invoke";
import { useLayout, type Box, type TabPanes } from "../../stores/layout";
import { DropZones, moveToNewTab, type PaneDrag } from "./dnd";
import { useDropTarget } from "./drops";
import { GUTTER, leaves, minExtent, type LayoutNode } from "./layout/tree";
import { PaneChrome } from "./PaneChrome";
import { BrowserPane } from "./BrowserPane";
import { SimulatorPane } from "../simulator/SimulatorPane";
import { pageTitle, useBrowser } from "../../stores/browser";
import { attachPaneHost, paneHost } from "./paneHosts";
import { LaunchGrid } from "./LaunchGrid";
import { launchFor } from "./PanePicker";
import { paneTitles, useFirstPrompts, type TitleSources } from "./titles";
import { webglPool } from "./webglPool";
import type { Activity } from "./commandMarks";
import {
  TerminalView,
  copyLastOutput,
  terminalHandles,
  type Phase,
  type TerminalOptions,
} from "./TerminalView";
import { PausedCard } from "./PausedCard";
import { WorktreeChip, worktreeMenu } from "./Worktrees";
import { useHandoff } from "../../stores/handoff";
import { AccountChip } from "./AccountChip";
import { LimitBanner } from "./LimitCard";
import { GuardBanner } from "./GuardCard";
import { SandboxChip } from "./SandboxChip";
import { ResourceChip } from "./ResourceChip";
import { ApprovalBanner } from "./ApprovalBanner";
import { useActivity } from "../../stores/activity";
import { RunDialog } from "../secrets/RunDialog";
import { setFrozen, useResources } from "../../stores/resources";
import { useContexts } from "../../stores/contexts";
import { baseBranch, isWorktree, useWorktrees } from "../../stores/worktrees";
import { useShortcutKeys } from "../commands/useShortcutKeys";

const BOX_THROTTLE_MS = 50;

/** Where a pane's host lives in the layout. */
function PaneSlot({ paneId }: { paneId: string }) {
  const ref = useCallback(
    (el: HTMLDivElement | null) => {
      if (el) attachPaneHost(paneId, el);
    },
    [paneId],
  );
  return <div ref={ref} data-slot={paneId} className="h-full w-full min-h-0 min-w-0" />;
}

/** Panel minimums in percent, so no pane goes below 240 × 120 px. */
function minimums(node: Extract<LayoutNode, { type: "split" }>, box: Box): [number, number] {
  const horizontal = node.dir === "row";
  const total = horizontal ? box.w : box.h;
  if (total <= 0) return [5, 5];
  const ea = minExtent(node.a, GUTTER);
  const eb = minExtent(node.b, GUTTER);
  let a = ((horizontal ? ea.w : ea.h) / total) * 100;
  let b = ((horizontal ? eb.w : eb.h) / total) * 100;
  if (a + b > 98) {
    const scale = 98 / (a + b);
    a *= scale;
    b *= scale;
  }
  const clamp = (v: number) => Math.min(95, Math.max(5, v));
  return [clamp(a), clamp(b)];
}

function Gutter({
  horizontal,
  onDragging,
  onBlur,
  onDoubleClick,
}: {
  horizontal: boolean;
  onDragging: (d: boolean) => void;
  onBlur: () => void;
  onDoubleClick: () => void;
}) {
  return (
    <PanelResizeHandle
      onDragging={onDragging}
      onBlur={onBlur}
      onDoubleClick={onDoubleClick}
      hitAreaMargins={{ coarse: 12, fine: 2 }}
      aria-label={t("pane.resize")}
      className={cx(
        "group/gutter relative flex shrink-0 items-center justify-center outline-none",
        horizontal ? "w-1.5" : "h-1.5",
      )}
    >
      <span
        aria-hidden="true"
        className={cx(
          "rounded-full bg-transparent transition-colors duration-[var(--dur-instant)]",
          "group-hover/gutter:bg-accent group-focus-visible/gutter:bg-accent group-data-[resize-handle-state=drag]/gutter:bg-accent",
          horizontal ? "h-full w-0.5" : "h-0.5 w-full",
        )}
      />
    </PanelResizeHandle>
  );
}

function Split({
  node,
  path,
  tabId,
  box,
}: {
  node: Extract<LayoutNode, { type: "split" }>;
  path: string;
  tabId: string;
  box: Box;
}) {
  const group = useRef<ImperativePanelGroupHandle>(null);
  const layout = useRef<number[]>([]);
  const horizontal = node.dir === "row";
  const [minA, minB] = minimums(node, box);
  const boxA = horizontal ? { w: box.w * node.ratio, h: box.h } : { w: box.w, h: box.h * node.ratio };
  const boxB = horizontal ? { w: box.w - boxA.w, h: box.h } : { w: box.w, h: box.h - boxA.h };

  // The store's ratio wins after Arrange, equalize or a load.
  useEffect(() => {
    const g = group.current;
    if (!g) return;
    const want = node.ratio * 100;
    const now = g.getLayout();
    // Before both panels register, defaultSize applies.
    if (now.length === 2 && Math.abs((now[0] ?? 0) - want) > 0.1) g.setLayout([want, 100 - want]);
  }, [node.ratio]);

  const commit = () => {
    const first = layout.current[0];
    if (first !== undefined && Math.abs(first / 100 - node.ratio) > 0.001)
      useLayout.getState().setRatio(tabId, path, first / 100);
  };

  return (
    <PanelGroup
      ref={group}
      direction={horizontal ? "horizontal" : "vertical"}
      onLayout={(l) => {
        layout.current = l;
      }}
    >
      <Panel id={`${tabId}-p${path}a`} order={1} defaultSize={node.ratio * 100} minSize={minA}>
        <Node node={node.a} path={`${path}a`} tabId={tabId} box={boxA} />
      </Panel>
      <Gutter
        horizontal={horizontal}
        onDragging={(d) => {
          useLayout.getState().setDragging(d);
          if (!d) commit();
        }}
        onBlur={commit}
        onDoubleClick={() => useLayout.getState().equalizeAt(tabId, path)}
      />
      <Panel id={`${tabId}-p${path}b`} order={2} defaultSize={(1 - node.ratio) * 100} minSize={minB}>
        <Node node={node.b} path={`${path}b`} tabId={tabId} box={boxB} />
      </Panel>
    </PanelGroup>
  );
}

function Node({ node, path, tabId, box }: { node: LayoutNode; path: string; tabId: string; box: Box }) {
  if (node.type === "leaf") return <PaneSlot paneId={node.paneId} />;
  return <Split key={`${path}:${node.dir}`} node={node} path={path} tabId={tabId} box={box} />;
}

/** The pane's status dot: the process's phase first, then shell activity
 * (working between a command's start and end marks, F-CD-10). */
function paneStatus(phase: Phase, activity: Activity): Status {
  if (phase.kind === "failed") return "error";
  if (phase.kind === "exited") return phase.code === 0 || phase.code === null ? "done" : "error";
  return activity === "working" ? "working" : "idle";
}

type PaneViewProps = {
  tabId: string;
  pane: PaneRecord;
  title: string;
  visible: boolean;
  focused: boolean;
  solo: boolean;
  maximized: boolean;
  options: TerminalOptions;
  /** Card view's title card, or the one-line collapsed session (F-CD-11).
   * The terminal keeps running and keeps its size behind it. */
  cover: "card" | "line" | null;
};

const PaneView = memo(function PaneView({
  tabId,
  pane,
  title,
  visible,
  focused,
  solo,
  maximized,
  options,
  cover,
}: PaneViewProps) {
  const keys = useShortcutKeys();
  const [phase, setPhase] = useState<Phase>({ kind: "starting" });
  const [activity, setActivity] = useState<Activity>("idle");
  // A restored agent waits on its card until resumed (F-SH-07).
  const card = useRestore((st) => st.cards[pane.id]);
  const paused = waitingOnCard(pane, card);
  // Agents report through hooks or heuristics (F-EN-05); shells through
  // their command marks. An exited or failed pane says so either way.
  const agentStatus = useStatus((st) => st.records[paneKey(pane.id)]?.status);
  const agentSince = useStatus((st) => st.records[paneKey(pane.id)]?.since ?? null);
  const status = paused
    ? "idle"
    : pane.kind === "agent" && agentStatus && phase.kind === "live"
      ? statusView(agentStatus)
      : paneStatus(phase, activity);
  const glyph = useEngines((st) => st.list.find((e) => e.spec.id === pane.engineId)?.spec.logo);
  const dropping = useDropTarget((st) => st.paneId === pane.id);
  const mac = useBoot((st) => st.boot?.platform === "macos");
  const engineNewline = useEngines(
    (st) => st.list.find((e) => e.spec.id === pane.engineId)?.spec.newlineSequence ?? "\x1b\r",
  );
  // "Lite": this pane renders without a WebGL context (UI_SPEC §2.27).
  const [lite, setLite] = useState(() => !webglPool.holds(pane.id));
  useEffect(() => {
    setLite(!webglPool.holds(pane.id));
    return webglPool.subscribe((id, webgl) => id === pane.id && setLite(!webgl));
  }, [pane.id]);
  const { show } = useToast();
  const drag: PaneDrag = { kind: "pane", paneId: pane.id, tabId, title };
  const { attributes, listeners, setNodeRef } = useDraggable({ id: `pane:${pane.id}`, data: drag });
  const layout = useLayout.getState;

  const split = (side: "right" | "bottom") => {
    if (layout().requestSplit(tabId, pane.id, side) === "no_room")
      show({ tone: "warning", title: t("split.noRoom") });
  };
  const [renaming, setRenaming] = useState(false);
  const flashing = useFlash((st) => st.flashing[pane.id] !== undefined);
  // Location metadata: the cwd relative to the workspace root (F-CD-12).
  const cwd = useTerminals((st) => st.states[pane.id]?.cwd ?? null);
  const home = useWorkspaces((st) => st.list.find((w) => w.id === pane.workspaceId));
  const root = home?.root ?? null;
  // A pane in a worktree shows its branch and the worktree actions (UI_SPEC §4.3).
  const wt = home && isWorktree(home) ? home : null;
  const base = useWorkspaces((st) => (wt ? baseBranch(st.list, wt) : ""));
  const meta = relativeCwd(cwd ?? pane.cwd, root);
  const note = useHandoff((st) => st.notes[pane.id]);
  const context = useContexts((st) => st.byPane[pane.id]);
  // F-EX-06: Freeze/Thaw for a pane on this computer, where it's offered.
  const freezing = useResources((st) => st.status?.freezing ?? false);
  const frozen = useResources((st) => st.byPane[pane.id]?.frozen ?? false);
  const freezable = freezing && home?.kind === "local";
  // F-TL-12: Run with secrets, from a terminal pane in a folder here; not
  // where plain terminals open in WSL, since the values would cross unmasked.
  const plainWsl = useSetting<string | null>("wsl_default_distro", null);
  const runnable = pane.kind === "terminal" && home?.kind === "local" && !plainWsl;
  const [running, setRunning] = useState(false);
  const toggleFrozen = () =>
    void setFrozen(pane.id, !frozen).catch((e: IpcError) =>
      show({
        tone: "error",
        title: frozen ? t("resources.thawFailed") : t("resources.freezeFailed"),
        body: e.message,
      }),
    );
  const rename = async (name: string): Promise<string | null> => {
    try {
      const stored = await invoke<string>("pane_rename", { paneId: pane.id, humanName: name });
      layout().setHumanName(tabId, pane.id, stored);
      return null;
    } catch (e) {
      return (e as IpcError).message ?? t("pane.renameFailed");
    }
  };
  const menu: MenuEntry[] = [
    // The full title, which the header may truncate (F-CD-12).
    { id: "title", type: "label", label: title },
    { id: "rename", label: t("pane.rename"), onSelect: () => setRenaming(true) },
    { id: "sep-title", type: "separator" },
    {
      id: "split-right",
      label: t("pane.splitRight"),
      kbd: keys("pane.split_right"),
      onSelect: () => split("right"),
    },
    {
      id: "split-down",
      label: t("pane.splitDown"),
      kbd: keys("pane.split_down"),
      onSelect: () => split("bottom"),
    },
    { id: "new-tab", label: t("pane.moveToNewTab"), onSelect: () => void moveToNewTab(tabId, pane.id) },
    { id: "restart", label: t("pane.restart"), onSelect: () => terminalHandles.get(pane.id)?.restart() },
    ...(runnable ? [{ id: "run", label: t("run.menu"), onSelect: () => setRunning(true) }] : []),
    ...(pane.kind === "agent"
      ? [
          {
            id: "activity",
            label: t("activity.menu"),
            onSelect: () => useActivity.getState().show({ source: "pane", id: pane.id, title }),
          },
        ]
      : []),
    ...(freezable
      ? [
          {
            id: "freeze",
            label: frozen ? t("resources.thaw") : t("resources.freeze"),
            onSelect: toggleFrozen,
          },
        ]
      : []),
    {
      id: "handoff",
      label: t("pane.handoff"),
      onSelect: () =>
        useHandoff
          .getState()
          .open({ paneId: pane.id, workspaceId: pane.workspaceId, kind: pane.kind, engineId: pane.engineId }),
    },
    // Auto is hosted: absent until an account carries it (D-2903).
    ...(pane.kind === "agent" && pane.engineId && useSession.getState().entitlement?.limits.auto
      ? [{ id: "auto", label: t("pane.auto"), onSelect: () => useAuto.getState().openDialog(pane.id) }]
      : []),
    { id: "sep-copy", type: "separator" },
    {
      id: "copy-last-output",
      label: t("terminal.copyLastOutput"),
      onSelect: () => void copyLastOutput(pane.id, show),
    },
    ...(
      [
        ["copy-100", "terminal.copyLast100", 100],
        ["copy-500", "terminal.copyLast500", 500],
        ["copy-all", "terminal.copyAll", null],
      ] as const
    ).map(([id, key, n]) => ({
      id,
      label: t(key),
      onSelect: () =>
        void terminalHandles
          .get(pane.id)
          ?.copyLast(n)
          .then((ok) => ok && show({ tone: "success", title: t("terminal.copied") })),
    })),
    ...(wt ? [{ id: "sep-worktree", type: "separator" as const }, ...worktreeMenu(wt, base, show)] : []),
    {
      id: "close",
      label: t("pane.closePane"),
      danger: true,
      kbd: keys("pane.close"),
      onSelect: () => useCloseConfirm.getState().request(tabId, pane.id),
    },
  ];

  return (
    <section
      aria-label={t("pane.label", { name: title })}
      data-testid="pane"
      data-pane-id={pane.id}
      data-focused={focused ? "true" : undefined}
      data-drop-target={dropping ? "true" : undefined}
      data-flash={flashing ? "true" : undefined}
      className={cx(
        "relative flex h-full min-h-0 flex-col overflow-hidden rounded-md border",
        flashing && "tn-flash",
        dropping && "ring-2 ring-accent",
        focused && !solo ? "border-border-strong" : "border-border",
      )}
      onFocusCapture={() => layout().focus(tabId, pane.id)}
      onPointerDownCapture={() => layout().focus(tabId, pane.id)}
    >
      <PaneChrome
        title={title}
        status={status}
        since={pane.kind === "agent" && (status === "working" || status === "waiting") ? agentSince : null}
        visible={visible}
        focused={focused && !solo}
        solo={solo}
        maximized={maximized}
        menuItems={menu}
        onMaximize={() => layout().toggleMaximize(tabId, pane.id)}
        onClose={() => useCloseConfirm.getState().request(tabId, pane.id)}
        glyph={pane.kind === "agent" && glyph ? <EngineGlyph letters={glyph} engine={pane.engineId ?? undefined} size={14} active={focused} /> : undefined}
        name={pane.humanName}
        meta={meta}
        location={
          <>
            {wt && <WorktreeChip wt={wt} />}
            <AccountChip pane={pane} />
            {pane.kind === "agent" && <SandboxChip paneId={pane.id} />}
            <ResourceChip paneId={pane.id} />
          </>
        }
        actions={
          runnable ? (
            <IconButton
              size="sm"
              icon={<Play size={13} />}
              label={t("run.menu")}
              data-testid="pane-run"
              onClick={() => setRunning(true)}
            />
          ) : undefined
        }
        onRename={rename}
        renaming={renaming}
        onRenamingChange={setRenaming}
        badges={
          <>
            <AutoChip paneId={pane.id} />
            {context && (
              <ContextMeter
                label={t("pane.context")}
                used={Number(context.used)}
                window={Number(context.window)}
                tooltip={t("pane.contextTooltip")}
              />
            )}
            {lite && visible && options.gpu !== false && (
              <Tooltip label={t("pane.liteTooltip")}>
                <span
                  data-testid="lite-badge"
                  tabIndex={0}
                  className="rounded-xs focus-visible:shadow-focus focus-visible:outline-none"
                >
                  <Badge>{t("pane.lite")}</Badge>
                </span>
              </Tooltip>
            )}
          </>
        }
        dragRef={setNodeRef}
        dragProps={{ ...attributes, ...listeners, tabIndex: -1, role: "toolbar" }}
      />
      {runnable && <RunDialog paneId={pane.id} tabId={tabId} open={running} onOpenChange={setRunning} />}
      <PaneBanners pane={pane} />
      {note && (
        // F-CD-16: where the work went, until dismissed.
        <div
          data-testid="handoff-note"
          className="flex h-6 shrink-0 items-center gap-1.5 border-b border-divider bg-surface-2 pl-2 pr-1 text-xs text-fg-muted"
        >
          <ArrowRightLeft size={12} aria-hidden="true" className="shrink-0" />
          <span className="min-w-0 flex-1 truncate">{note}</span>
          <IconButton
            size="sm"
            icon={<X size={12} />}
            label={t("handoff.dismissNote")}
            onClick={() => useHandoff.getState().note(pane.id, null)}
          />
        </div>
      )}
      <div className="relative min-h-0 flex-1">
        {cover && (
          <PaneCover
            kind={cover}
            pane={pane}
            title={title}
            status={status}
            onFlip={() => layout().flipCard(tabId, pane.id)}
          />
        )}
        {paused ? (
          <PausedCard pane={pane} title={title} card={card} focused={focused && visible} options={options} />
        ) : (
          <TerminalView
            paneId={pane.id}
            label={t("terminal.label", { name: title })}
            visible={visible && !cover}
            focused={focused}
            bare
            options={options}
            onClose={() => useCloseConfirm.getState().request(tabId, pane.id)}
            onPhase={setPhase}
            onActivity={setActivity}
            agent={pane.kind === "agent"}
            newlineSequence={engineNewline}
            mac={mac}
          />
        )}
      </div>
      <DropZones paneId={pane.id} tabId={tabId} />
    </section>
  );
});

/** A cwd relative to its workspace root; nothing at the root itself. */
export function relativeCwd(cwd: string | null, root: string | null): string | null {
  if (!cwd || !root || cwd === root) return null;
  const base = root.endsWith("/") ? root : `${root}/`;
  return cwd.startsWith(base) ? cwd.slice(base.length) : cwd;
}

/** Elapsed time since `since` (ms), as "4m" or "1h 12m". */
function elapsed(since: number, now: number): string {
  const m = Math.max(0, Math.floor((now - since) / 60_000));
  return m < 60 ? t("time.minutes", { n: m }) : t("time.hoursMinutes", { h: Math.floor(m / 60), m: m % 60 });
}

/** F-CD-11: Card view's title card (click flips the pane back), or the
 * collapsed one-line session. */
function PaneCover({
  kind,
  pane,
  title,
  status,
  onFlip,
}: {
  kind: "card" | "line";
  pane: PaneRecord;
  title: string;
  status: Status;
  onFlip: () => void;
}) {
  const spec = useEngines((st) => st.list.find((e) => e.spec.id === pane.engineId)?.spec);
  const since = useStatus((st) => st.records[paneKey(pane.id)]?.since);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (status !== "working") return;
    const id = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(id);
  }, [status]);
  const engine = spec?.label ?? t("picker.terminal");
  if (kind === "line")
    return (
      <div
        data-testid="pane-collapsed"
        className="absolute inset-0 z-10 flex items-center justify-center gap-2 bg-surface-2 text-sm text-fg"
      >
        <StatusDot status={status} size={8} />
        {spec ? (
          <EngineGlyph letters={spec.logo} engine={spec.id} size={16} />
        ) : (
          <SquareTerminal size={14} aria-hidden="true" className="text-fg-muted" />
        )}
        <span className="truncate">{title}</span>
      </div>
    );
  return (
    <button
      type="button"
      data-testid="pane-card"
      data-status={status}
      onClick={onFlip}
      className="absolute inset-0 z-10 flex flex-col items-start justify-center gap-2 bg-surface-2 p-6 text-left hover:bg-surface-3 focus-visible:shadow-focus focus-visible:outline-none"
    >
      <span className="line-clamp-2 text-lg font-semibold text-fg" data-testid="pane-card-title">
        {title}
      </span>
      <span className="flex items-center gap-2 text-sm text-fg-muted">
        {spec ? (
          <EngineGlyph letters={spec.logo} engine={spec.id} size={16} />
        ) : (
          <SquareTerminal size={14} aria-hidden="true" />
        )}
        {engine}
      </span>
      <span className="flex items-center gap-2 text-sm text-fg-muted">
        <StatusDot status={status} size={8} />
        {t(`agentStatus.${status}`)}
        {status === "working" && since !== undefined && <span>· {elapsed(since, now)}</span>}
      </span>
    </button>
  );
}

/** The empty tab (UI-10): launch tiles instead of a shell. */
function EmptyTab({ tabId, workspace }: { tabId: string; workspace: WorkspaceSummary }) {
  const [busy, setBusy] = useState<string | null>(null);
  const { show } = useToast();
  const start = async (tileId: string) => {
    setBusy(tileId);
    const res = await useLayout.getState().split(tabId, null, "right", launchFor(tileId));
    setBusy(null);
    if (!res.ok) {
      if (res.error.code === "limit_reached")
        useLayout.setState({ picker: { tabId, target: null, side: "right" } });
      else show({ tone: "error", title: res.error.message });
    }
  };
  return (
    <div data-testid="empty-tab" className="flex h-full items-center justify-center p-6">
      <div className="flex w-full max-w-[480px] flex-col gap-3">
        <div className="flex flex-col gap-1">
          <h2 className="text-md font-semibold text-fg">{t("code.startIn", { name: workspace.name })}</h2>
          <p className="text-sm text-fg-muted">{t("code.startBody")}</p>
        </div>
        <LaunchGrid tabId={tabId} busy={busy} onPick={(id) => void start(id)} />
        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            variant="secondary"
            iconLeft={<Layers size={14} aria-hidden="true" />}
            onClick={() => useLaunch.getState().open(workspace.id)}
            data-testid="launch-several"
          >
            {t("launch.several")}
          </Button>
          {workspace.kind === "local" && workspace.branch && !workspace.missing && (
            <Button
              size="sm"
              variant="ghost"
              iconLeft={<GitBranch size={14} aria-hidden="true" />}
              onClick={() => useWorktrees.getState().open({ kind: "new", workspaceId: workspace.id })}
              data-testid="launch-worktree"
            >
              {t("launch.newWorktree")}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}

/** UI_SPEC §2.21 / UI-11: one tab's panes. Every pane renders once, into a
 * host the layout moves between slots, so swaps and maximize keep it alive. */
export function PaneGrid({
  workspace,
  tabId,
  visible,
  options,
}: {
  workspace: WorkspaceSummary;
  tabId: string;
  visible: boolean;
  options: TerminalOptions;
}) {
  const tab: TabPanes | undefined = useLayout((s) => s.tabs[tabId]);
  const loaded = useLayout((s) => s.loaded[workspace.id] ?? false);
  const [root, setRoot] = useState<HTMLDivElement | null>(null);
  const [box, setBox] = useState<Box>({ w: 0, h: 0 });

  useEffect(() => {
    if (!loaded)
      void useLayout
        .getState()
        .load(workspace.id)
        .catch(() => undefined);
  }, [loaded, workspace.id]);

  const needsShell = loaded && !workspace.missing && (!tab || (!tab.tree && !tab.keepEmpty));
  useEffect(() => {
    if (needsShell) void useLayout.getState().ensurePane(tabId);
  }, [needsShell, tabId]);

  // The grid's size, throttled (F-CD-02: reflow at most every 50 ms).
  useEffect(() => {
    const el = root;
    if (!el) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const measure = () => {
      timer = undefined;
      const next = { w: el.clientWidth, h: el.clientHeight };
      if (next.w === 0) return;
      setBox((b) => (b.w === next.w && b.h === next.h ? b : next));
      useLayout.getState().setBox(tabId, next);
    };
    const ro = new ResizeObserver(() => {
      timer ??= setTimeout(measure, BOX_THROTTLE_MS);
    });
    ro.observe(el);
    measure();
    return () => {
      ro.disconnect();
      clearTimeout(timer);
    };
  }, [root, tabId]);

  const engines = useEngines((st) => st.list);
  const cards = useLayout((st) => st.cards[tabId]);
  const records = useStatus((st) => st.records);
  const ptyStates = useTerminals((st) => st.states);
  const prompts = useFirstPrompts((st) => st.prompts);
  const pages = useBrowser((st) => st.pages);
  const folder = useWorkspaces((st) => st.list.find((w) => w.tabs.some((x) => x.id === tabId))?.name ?? null);
  const names = useMemo(() => {
    const sources: Record<string, TitleSources> = {};
    for (const id of Object.keys(tab?.panes ?? {})) {
      const page = pages[id];
      sources[id] = {
        hook: records[paneKey(id)]?.title,
        osc: ptyStates[id]?.title,
        firstPrompt: prompts[id],
        page: page ? pageTitle(page) : null,
      };
    }
    return paneTitles(tab?.panes ?? {}, engines, sources, folder);
  }, [tab?.panes, engines, records, ptyStates, prompts, pages, folder]);
  // The pane in front never turns unread (F-SH-03).
  const active = tab?.active ?? null;
  useEffect(() => {
    const st = useStatus.getState();
    if (visible && active) st.setFocused(paneKey(active));
    else if (active && st.focusedKey === paneKey(active)) st.setFocused(null);
  }, [visible, active]);

  if (!tab || (!tab.tree && !tab.keepEmpty)) {
    return (
      <div ref={setRoot} className="flex h-full items-center justify-center">
        {!workspace.missing && <Spinner label={t("terminal.starting")} />}
      </div>
    );
  }
  if (!tab.tree) {
    return (
      <div ref={setRoot} className="h-full">
        <EmptyTab tabId={tabId} workspace={workspace} />
      </div>
    );
  }

  const ids = leaves(tab.tree);
  const solo = ids.length === 1;
  return (
    <div
      ref={setRoot}
      role="group"
      aria-label={t("code.grid", { name: workspace.name })}
      data-testid="pane-grid"
      className="relative h-full min-h-0"
    >
      {tab.maximized ? (
        <PaneSlot paneId={tab.maximized} />
      ) : (
        <Node node={tab.tree} path="" tabId={tabId} box={box} />
      )}
      {ids.map((id) => {
        const pane = tab.panes[id];
        if (!pane) return null;
        const cover = cards && !cards.flipped[id] ? "card" : workspace.sessionsCollapsed ? "line" : null;
        return createPortal(
          pane.kind === "browser" ? (
            <BrowserPane
              tabId={tabId}
              pane={pane}
              title={names[id] ?? ""}
              visible={visible && (!tab.maximized || tab.maximized === id)}
              focused={tab.active === id}
              solo={solo}
              maximized={tab.maximized === id}
              cover={cover}
            />
          ) : pane.kind === "simulator" ? (
            <SimulatorPane
              tabId={tabId}
              pane={pane}
              title={names[id] ?? ""}
              visible={visible && (!tab.maximized || tab.maximized === id)}
              focused={tab.active === id}
              solo={solo}
              maximized={tab.maximized === id}
              cover={cover}
            />
          ) : (
            <PaneView
              tabId={tabId}
              pane={pane}
              title={names[id] ?? ""}
              visible={visible && (!tab.maximized || tab.maximized === id)}
              focused={tab.active === id}
              solo={solo}
              maximized={tab.maximized === id}
              options={options}
              cover={cover}
            />
          ),
          paneHost(id),
          id,
        );
      })}
    </div>
  );
}

/** The banners under a pane's chrome. F-TL-12: the approval comes first, so
 * banners the agent's own output or reports can raise (limits, the runaway
 * guard) never push its Allow down. */
export function PaneBanners({ pane }: { pane: PaneRecord }) {
  return (
    <>
      {pane.kind === "agent" && <ApprovalBanner paneId={pane.id} />}
      <LimitBanner pane={pane} />
      <GuardBanner paneId={pane.id} />
    </>
  );
}
