import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useDraggable } from "@dnd-kit/core";
import {
  Panel,
  PanelGroup,
  PanelResizeHandle,
  type ImperativePanelGroupHandle,
} from "react-resizable-panels";
import type { PaneRecord, WorkspaceSummary } from "@tandem/protocol";
import { Spinner, cx, useToast, type MenuEntry, type Status } from "@tandem/ui";
import { t } from "../../i18n";
import { useLayout, type Box, type TabPanes } from "../../stores/layout";
import { DropZones, applyDrop, type PaneDrag } from "./dnd";
import { leaves, minExtent, type LayoutNode } from "./layout/tree";
import { PaneChrome } from "./PaneChrome";
import { attachPaneHost, paneHost } from "./paneHosts";
import { LaunchTiles, useTiles } from "./PanePicker";
import { TerminalView, terminalHandles, type Phase, type TerminalOptions } from "./TerminalView";

/** Gutter between panes (UI_SPEC §2.21). */
const GUTTER = 6;
const BOX_THROTTLE_MS = 50;

/** Pane titles default to "Shell · n" in creation order (UI-11). */
function titles(panes: Record<string, PaneRecord>): Record<string, string> {
  const sorted = Object.values(panes).sort((a, b) => a.createdAt - b.createdAt || a.id.localeCompare(b.id));
  return Object.fromEntries(
    sorted.map((p, i) => [p.id, p.humanName ?? p.title ?? t("pane.shell", { n: i + 1 })]),
  );
}

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

function phaseStatus(phase: Phase): Status {
  if (phase.kind === "failed") return "error";
  if (phase.kind === "exited") return phase.code === 0 || phase.code === null ? "done" : "error";
  return "idle";
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
}: PaneViewProps) {
  const [status, setStatus] = useState<Status>("idle");
  const { show } = useToast();
  const drag: PaneDrag = { kind: "pane", paneId: pane.id, tabId, title };
  const { attributes, listeners, setNodeRef } = useDraggable({ id: `pane:${pane.id}`, data: drag });
  const layout = useLayout.getState;

  const split = (side: "right" | "bottom") => {
    if (layout().requestSplit(tabId, pane.id, side) === "no_room")
      show({ tone: "warning", title: t("split.noRoom") });
  };
  const menu: MenuEntry[] = [
    { id: "split-right", label: t("pane.splitRight"), kbd: ["Mod", "D"], onSelect: () => split("right") },
    {
      id: "split-down",
      label: t("pane.splitDown"),
      kbd: ["Mod", "Shift", "D"],
      onSelect: () => split("bottom"),
    },
    { id: "new-tab", label: t("pane.moveToNewTab"), onSelect: () => void moveToNewTab(tabId, pane.id) },
    { id: "restart", label: t("pane.restart"), onSelect: () => terminalHandles.get(pane.id)?.restart() },
    {
      id: "close",
      label: t("pane.closePane"),
      danger: true,
      kbd: ["Mod", "W"],
      onSelect: () => void layout().close(tabId, pane.id),
    },
  ];

  return (
    <section
      aria-label={t("pane.label", { name: title })}
      data-testid="pane"
      data-pane-id={pane.id}
      data-focused={focused ? "true" : undefined}
      className={cx(
        "relative flex h-full min-h-0 flex-col overflow-hidden rounded-md border",
        focused && !solo ? "border-border-strong" : "border-border",
      )}
      onFocusCapture={() => layout().focus(tabId, pane.id)}
      onPointerDownCapture={() => layout().focus(tabId, pane.id)}
    >
      <PaneChrome
        title={title}
        status={status}
        focused={focused && !solo}
        solo={solo}
        maximized={maximized}
        menuItems={menu}
        onMaximize={() => layout().toggleMaximize(tabId, pane.id)}
        onClose={() => void layout().close(tabId, pane.id)}
        dragRef={setNodeRef}
        dragProps={{ ...attributes, ...listeners, tabIndex: -1, role: "toolbar" }}
      />
      <div className="relative min-h-0 flex-1">
        <TerminalView
          paneId={pane.id}
          label={t("terminal.label", { name: title })}
          visible={visible}
          focused={focused}
          bare
          options={options}
          onClose={() => void layout().close(tabId, pane.id)}
          onPhase={(p) => setStatus(phaseStatus(p))}
        />
      </div>
      <DropZones paneId={pane.id} tabId={tabId} />
    </section>
  );
});

async function moveToNewTab(tabId: string, paneId: string): Promise<void> {
  const pane = useLayout.getState().tabs[tabId]?.panes[paneId];
  if (pane)
    await applyDrop(
      { kind: "pane", paneId, tabId, title: "" },
      { kind: "new_tab", workspaceId: pane.workspaceId },
    );
}

/** The empty tab (UI-10): launch tiles instead of a shell. */
function EmptyTab({ tabId, workspace }: { tabId: string; workspace: WorkspaceSummary }) {
  const tiles = useTiles(workspace.name);
  const [busy, setBusy] = useState<string | null>(null);
  const { show } = useToast();
  const start = async () => {
    setBusy("shell");
    const res = await useLayout.getState().split(tabId, null, "right");
    setBusy(null);
    if (!res.ok) {
      if (res.error.code === "limit_reached")
        useLayout.setState({ picker: { tabId, target: null, side: "right" } });
      else show({ tone: "error", title: res.error.message });
    }
  };
  return (
    <div data-testid="empty-tab" className="flex h-full items-center justify-center p-6">
      <div className="flex w-full max-w-[600px] flex-col gap-3">
        <h2 className="text-md font-semibold text-fg">{t("code.start")}</h2>
        <LaunchTiles tiles={tiles} busy={busy} onPick={() => void start()} />
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

  const names = useMemo(() => titles(tab?.panes ?? {}), [tab?.panes]);

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
        return createPortal(
          <PaneView
            tabId={tabId}
            pane={pane}
            title={names[id] ?? ""}
            visible={visible && (!tab.maximized || tab.maximized === id)}
            focused={tab.active === id}
            solo={solo}
            maximized={tab.maximized === id}
            options={options}
          />,
          paneHost(id),
          id,
        );
      })}
    </div>
  );
}
