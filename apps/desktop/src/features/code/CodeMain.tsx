import { useCloseConfirm } from "../../stores/closeConfirm";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import { SortableContext, arrayMove, horizontalListSortingStrategy, useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { FolderPlus, LayoutGrid, Plus, X } from "lucide-react";
import type { Tab } from "@tandem/protocol";
import {
  Button,
  ConfirmDialog,
  EmptyState,
  IconButton,
  cx,
  terminalThemeFor,
  useTheme,
  useToast,
  type TerminalThemeSetting,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useSetting } from "../../stores/settings";
import { useWorkspaces } from "../../stores/workspaces";
import { pickAndAddWorkspace } from "../shell/actions";
import { PaneDndProvider, useNewTabTarget, useDrag } from "./dnd";
import { useFileDrops } from "./drops";
import { PaneGrid } from "./PaneGrid";
import { PanePicker } from "./PanePicker";
import type { TerminalOptions } from "./TerminalView";
import { webglPool } from "./webglPool";
import { useShortcutKeys } from "../commands/useShortcutKeys";

function useTerminalOptions(): TerminalOptions {
  const { resolved, highContrast } = useTheme();
  const themeSetting = useSetting<TerminalThemeSetting>("terminal_theme", "follow_app");
  const fontFamily = useSetting("term_font_family", "JetBrains Mono");
  const fontSize = useSetting("term_font_size", 13);
  const cursorStyle = useSetting<"bar" | "block" | "underline">("term_cursor_style", "bar");
  const cursorBlink = useSetting("term_cursor_blink", false);
  const scrollback = useSetting("term_scrollback", 10_000);
  // "off" keeps every pane on the DOM renderer (Settings › Terminal).
  const gpu = useSetting<"auto" | "off">("term_gpu_rendering", "auto") !== "off";
  const firstGpu = useRef(gpu);
  useEffect(() => {
    if (firstGpu.current === gpu) return;
    firstGpu.current = gpu;
    webglPool.reset();
  }, [gpu]);
  const optionAsMeta = useSetting("term_option_as_meta", false);
  const screenReader = useSetting("term_screen_reader", false);
  const confirmMultilinePaste = useSetting("term_confirm_multiline_paste", true);
  const copyOnSelect = useSetting("term_copy_on_select", false);
  return useMemo(
    () => ({
      theme: terminalThemeFor(themeSetting, resolved, highContrast),
      fontFamily,
      fontSize,
      cursorStyle,
      cursorBlink,
      scrollback,
      gpu,
      optionAsMeta,
      screenReader,
      confirmMultilinePaste,
      copyOnSelect,
    }),
    [
      themeSetting,
      resolved,
      highContrast,
      fontFamily,
      fontSize,
      cursorStyle,
      cursorBlink,
      scrollback,
      gpu,
      optionAsMeta,
      screenReader,
      confirmMultilinePaste,
      copyOnSelect,
    ],
  );
}

/** The tab strip; while a pane is dragged it's the "New tab" target, and
 * its tabs reorder by drag (F-CD-01 AC2). */
function TabStrip({
  tabs,
  workspaceId,
  label,
  children,
}: {
  tabs: Tab[];
  workspaceId: string;
  label: string;
  children: ReactNode;
}) {
  const { setNodeRef, isOver } = useNewTabTarget(workspaceId);
  const dragging = useDrag((s) => s.active !== null);
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));
  const ids = tabs.map((x) => x.id);
  const onDragEnd = (e: DragEndEvent) => {
    const over = e.over ? String(e.over.id) : null;
    const id = String(e.active.id);
    if (!over || over === id) return;
    void useWorkspaces
      .getState()
      .reorderTabs(workspaceId, arrayMove(ids, ids.indexOf(id), ids.indexOf(over)));
  };
  return (
    <div
      ref={setNodeRef}
      className={cx(
        "flex min-w-0 flex-1 items-center gap-1 rounded-sm",
        dragging && "outline-dashed outline-1 outline-border-strong",
        isOver && "bg-accent-soft outline-accent",
      )}
    >
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragEnd={onDragEnd}
        accessibility={{ screenReaderInstructions: { draggable: "" } }}
      >
        <SortableContext items={ids} strategy={horizontalListSortingStrategy}>
          <ul aria-label={label} className="flex min-w-0 items-center gap-1 overflow-x-auto">
            {children}
          </ul>
        </SortableContext>
      </DndContext>
    </div>
  );
}

/** One tab: select, double-click or F2 to rename, close, drag to reorder. */
function TabItem({
  tab,
  selected,
  onSelect,
  onClose,
}: {
  tab: Tab;
  selected: boolean;
  onSelect: () => void;
  onClose: () => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: tab.id,
  });
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(tab.name);
  const { show } = useToast();
  // Enter or Escape settles the edit; the blur that follows must not commit again.
  const settled = useRef(false);
  const startRename = () => {
    settled.current = false;
    setDraft(tab.name);
    setRenaming(true);
  };
  const done = (name: string | null) => {
    if (settled.current) return;
    settled.current = true;
    setRenaming(false);
    const next = name?.trim();
    if (next && next !== tab.name)
      void useWorkspaces
        .getState()
        .renameTab(tab.workspaceId, tab.id, next)
        .then((err) => err && show({ tone: "error", title: err.message }));
  };
  return (
    <li
      ref={setNodeRef}
      data-testid="tab"
      data-tab-id={tab.id}
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={cx(
        "group flex h-7 items-center rounded-sm text-base",
        selected ? "bg-surface-3 text-fg" : "text-fg-muted hover:bg-surface-2",
        isDragging && "z-10 opacity-80",
      )}
    >
      {renaming ? (
        <input
          autoFocus
          aria-label={t("code.renameTab", { name: tab.name })}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") done(draft);
            if (e.key === "Escape") done(null);
          }}
          onBlur={() => done(draft)}
          className="mx-1 h-6 w-28 rounded-xs border border-accent bg-surface-2 px-1 text-base text-fg outline-none"
        />
      ) : (
        <button
          type="button"
          {...attributes}
          {...listeners}
          role={undefined}
          aria-roledescription={undefined}
          aria-describedby={undefined}
          aria-current={selected ? "page" : undefined}
          aria-controls={`panel-${tab.id}`}
          tabIndex={0}
          onClick={onSelect}
          onDoubleClick={startRename}
          onKeyDown={(e) => {
            if (e.key === "F2") {
              e.preventDefault();
              startRename();
            }
          }}
          className="h-full whitespace-nowrap rounded-sm px-2.5 focus-visible:shadow-focus focus-visible:outline-none"
        >
          {tab.name}
        </button>
      )}
      <button
        type="button"
        aria-label={t("code.closeTab", { name: tab.name })}
        onClick={onClose}
        className="mr-1 inline-flex size-5 items-center justify-center rounded-xs text-fg-subtle opacity-0 hover:bg-surface-4 hover:text-fg focus-visible:opacity-100 focus-visible:shadow-focus focus-visible:outline-none group-hover:opacity-100"
      >
        <X size={12} aria-hidden="true" />
      </button>
    </li>
  );
}

/** "Stop {name}? It's working." before closing a working agent (F-CD-12 AC2). */
function CloseConfirm() {
  const pending = useCloseConfirm((s) => s.pending);
  return (
    <ConfirmDialog
      open={pending !== null}
      onOpenChange={(o) => !o && useCloseConfirm.getState().cancel()}
      title={t("pane.stopConfirm", { name: pending?.name ?? "" })}
      body={t("pane.stopBody")}
      confirmLabel={t("pane.stopAndClose")}
      cancelLabel={t("pane.keepRunning")}
      danger
      onConfirm={() => useCloseConfirm.getState().confirm()}
    />
  );
}

export function CodeMain({ visible }: { visible: boolean }) {
  const keys = useShortcutKeys();
  const { list, activeId, activeTab, selectTab, createTab, closeTab } = useWorkspaces();
  const { show } = useToast();
  const options = useTerminalOptions();
  useFileDrops(show);
  const [visited, setVisited] = useState<Record<string, Tab>>({});
  const active = list.find((w) => w.id === activeId) ?? null;
  const activeTabId = active ? activeTab[active.id] : undefined;

  useEffect(() => {
    const tab = active?.tabs.find((x) => x.id === activeTabId);
    if (tab && !visited[tab.id]) setVisited((v) => ({ ...v, [tab.id]: tab }));
  }, [active, activeTabId, visited]);

  // Drop grids whose tab is gone.
  useEffect(() => {
    const alive = new Set(list.flatMap((w) => w.tabs.map((x) => x.id)));
    const gone = Object.keys(visited).filter((id) => !alive.has(id));
    if (gone.length) {
      for (const id of gone) useLayout.getState().forgetTab(id);
      setVisited((v) => Object.fromEntries(Object.entries(v).filter(([id]) => alive.has(id))));
    }
  }, [list, visited]);

  if (!active) {
    return (
      <div className="flex h-full items-center justify-center">
        <EmptyState
          icon={<FolderPlus size={20} />}
          title={t("code.empty.title")}
          body={t("code.empty.body")}
          primary={
            <Button variant="primary" onClick={() => void pickAndAddWorkspace(show)}>
              {t("code.empty.add")}
            </Button>
          }
        />
      </div>
    );
  }

  return (
    <PaneDndProvider>
      <div className="flex h-full min-h-0 flex-col gap-1.5">
        <div className="flex h-8 shrink-0 items-center gap-1">
          <TabStrip tabs={active.tabs} workspaceId={active.id} label={t("code.tabs", { name: active.name })}>
            {active.tabs.map((tab) => (
              <TabItem
                key={tab.id}
                tab={tab}
                selected={tab.id === activeTabId}
                onSelect={() => selectTab(active.id, tab.id)}
                onClose={() => void closeTab(active.id, tab.id)}
              />
            ))}
          </TabStrip>
          <IconButton
            size="sm"
            icon={<Plus size={14} />}
            label={t("code.newTab")}
            kbd={keys("workspace.new_tab")}
            onClick={() => void createTab(active.id)}
          />
          {activeTabId && (
            <Button
              size="sm"
              variant="ghost"
              iconLeft={<LayoutGrid size={14} />}
              onClick={() => useLayout.getState().arrange(activeTabId)}
            >
              {t("code.arrange")}
            </Button>
          )}
        </div>
        <div className="relative min-h-0 flex-1">
          {active.missing && (
            <div className="absolute inset-0 z-10 flex items-center justify-center bg-bg">
              <EmptyState
                title={t("code.missing.title")}
                body={t("code.missing.body")}
                primary={
                  <Button
                    variant="primary"
                    onClick={() =>
                      void (async () => {
                        const picked = await invoke<string | null>("workspace_pick_folder", undefined, {
                          timeoutMs: 24 * 3_600_000,
                        }).catch(() => null);
                        if (!picked) return;
                        const err = await useWorkspaces.getState().locate(active.id, picked);
                        if (err) show({ tone: "error", title: err.message });
                      })()
                    }
                  >
                    {t("rail.locate")}
                  </Button>
                }
                secondary={
                  <Button variant="ghost" onClick={() => useWorkspaces.getState().askRemove(active)}>
                    {t("code.remove.confirm")}
                  </Button>
                }
              />
            </div>
          )}
          {Object.values(visited).map((tab) => {
            const shown = visible && tab.id === activeTabId && tab.workspaceId === active.id;
            return (
              <div
                key={tab.id}
                id={`panel-${tab.id}`}
                className="absolute inset-0"
                style={{ display: shown ? "block" : "none" }}
              >
                <PaneGrid
                  workspace={list.find((w) => w.id === tab.workspaceId) ?? active}
                  tabId={tab.id}
                  visible={shown}
                  options={options}
                />
              </div>
            );
          })}
        </div>
        <PanePicker workspaceName={active.name} />
        <CloseConfirm />
      </div>
    </PaneDndProvider>
  );
}
