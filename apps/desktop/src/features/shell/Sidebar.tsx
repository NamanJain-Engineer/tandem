import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
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
import {
  Globe,
  History,
  LayoutDashboard,
  ListTodo,
  Maximize2,
  Minimize2,
  NotebookPen,
  Pin,
  PinOff,
  Plug,
  Plus,
  FolderTree,
  X,
} from "lucide-react";
import type { SidebarTab, SidebarTabKind } from "@tandem/protocol";
import { Badge, EmptyState, IconButton, Menu, cx, useToast, type MenuEntry } from "@tandem/ui";
import { t } from "../../i18n";
import { useLayout } from "../../stores/layout";
import { FLOAT_BELOW, SIDEBAR_MAX, SIDEBAR_MIN, useSidebar } from "../../stores/sidebar";
import { useStatus } from "../../stores/status";
import { focusAddressOnMount } from "../../stores/browser";
import { BrowserView } from "../browser/BrowserView";
import { moveToPane } from "../code/BrowserPane";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { DashboardPanel } from "../dashboard/ReviewQueue";
import { CheckpointsPanel } from "../checkpoints/CheckpointsPanel";
import { FilesPanel } from "../files/FilesPanel";
import { PortsPanel } from "../ports/PortsPanel";
import { ScratchpadPanel } from "../scratchpad/ScratchpadPanel";
import { TasksPanel } from "../tasks/TasksPanel";
import { useShortcutKeys } from "../commands/useShortcutKeys";

const ICONS: Record<SidebarTabKind, ReactNode> = {
  dashboard: <LayoutDashboard size={13} aria-hidden="true" />,
  browser: <Globe size={13} aria-hidden="true" />,
  files: <FolderTree size={13} aria-hidden="true" />,
  tasks: <ListTodo size={13} aria-hidden="true" />,
  scratchpad: <NotebookPen size={13} aria-hidden="true" />,
  checkpoints: <History size={13} aria-hidden="true" />,
  ports: <Plug size={13} aria-hidden="true" />,
};

const LABELS: Record<Exclude<SidebarTabKind, "browser">, () => string> = {
  dashboard: () => t("sidebar.dashboard"),
  files: () => t("sidebar.files"),
  tasks: () => t("sidebar.tasks"),
  scratchpad: () => t("sidebar.scratchpad"),
  checkpoints: () => t("sidebar.checkpoints"),
  ports: () => t("sidebar.ports"),
};

function tabLabel(tab: SidebarTab): string {
  return tab.kind === "browser" ? (tab.title ?? t("sidebar.browser")) : LABELS[tab.kind]();
}

/** The window is narrow: the sidebar floats instead of docking. */
function useNarrow(): boolean {
  const [narrow, setNarrow] = useState(() => window.innerWidth < FLOAT_BELOW);
  useEffect(() => {
    const on = () => setNarrow(window.innerWidth < FLOAT_BELOW);
    window.addEventListener("resize", on);
    return () => window.removeEventListener("resize", on);
  }, []);
  return narrow;
}

/** Waiting agents, for the Dashboard pill's badge. */
function useWaitingCount(): number {
  return useStatus((s) => Object.values(s.records).filter((r) => r.status === "waiting").length);
}

function Pill({ tab, active }: { tab: SidebarTab; active: boolean }) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: tab.id,
  });
  const waiting = useWaitingCount();
  const label = tabLabel(tab);
  const store = useSidebar.getState;
  const move = (delta: -1 | 1) => {
    const ids = store().tabs.map((x) => x.id);
    const at = ids.indexOf(tab.id);
    const to = at + delta;
    if (to < 0 || to >= ids.length) return;
    store().reorder(arrayMove(ids, at, to));
  };
  return (
    <li
      ref={setNodeRef}
      role="presentation"
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={cx(
        "group flex h-6 shrink-0 items-center rounded-full border text-sm",
        active
          ? "border-border-strong bg-surface-3 text-fg"
          : "border-transparent text-fg-muted hover:bg-surface-2",
        isDragging && "z-10 opacity-80",
      )}
    >
      <button
        type="button"
        id={`sidebar-tab-${tab.id}`}
        data-testid={`sidebar-pill-${tab.kind}`}
        data-tab-id={tab.id}
        aria-selected={active}
        aria-controls={`sidebar-panel-${tab.id}`}
        {...attributes}
        {...listeners}
        role="tab"
        aria-roledescription={undefined}
        aria-describedby={undefined}
        aria-keyshortcuts="Delete"
        tabIndex={active ? 0 : -1}
        onClick={() => store().activate(tab.id)}
        onKeyDown={(e) => {
          if (e.altKey && e.shiftKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
            e.preventDefault();
            move(e.key === "ArrowLeft" ? -1 : 1);
          } else if (e.key === "Delete") {
            e.preventDefault();
            store().close(tab.id);
          }
        }}
        className="flex h-full items-center gap-1.5 rounded-full pl-2.5 pr-1 focus-visible:shadow-focus focus-visible:outline-none"
      >
        {ICONS[tab.kind]}
        <span className="max-w-[140px] truncate">{label}</span>
        {tab.kind === "dashboard" && waiting > 0 && (
          <span aria-label={t("sidebar.waiting", { n: waiting })}>
            <Badge variant="waiting">{waiting}</Badge>
          </span>
        )}
      </button>
      {/* A tab list may only hold tabs: × is for the mouse; the keyboard
          closes the focused tab with Delete (aria-keyshortcuts). */}
      <span
        aria-hidden="true"
        title={t("sidebar.closeTab", { name: label })}
        data-testid="sidebar-pill-close"
        onClick={() => store().close(tab.id)}
        className="mr-1 inline-flex size-4 cursor-default items-center justify-center rounded-full text-fg-subtle opacity-0 hover:bg-surface-4 hover:text-fg group-hover:opacity-100"
      >
        <X size={11} aria-hidden="true" />
      </span>
    </li>
  );
}

/** The left edge: drag or arrow keys resize the docked sidebar (320–900). A
 * drag defers PTY resizes to pointer-up, like a divider (F-CD-02). */
function Resizer({ width, onLive }: { width: number; onLive: (w: number | null) => void }) {
  const start = useRef<{ x: number; w: number } | null>(null);
  const at = (e: ReactPointerEvent) =>
    Math.min(
      SIDEBAR_MAX,
      Math.max(SIDEBAR_MIN, start.current ? start.current.w - (e.clientX - start.current.x) : width),
    );
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={t("sidebar.resize")}
      aria-valuemin={SIDEBAR_MIN}
      aria-valuemax={SIDEBAR_MAX}
      aria-valuenow={width}
      tabIndex={0}
      data-testid="sidebar-resizer"
      onPointerDown={(e) => {
        e.preventDefault();
        start.current = { x: e.clientX, w: width };
        e.currentTarget.setPointerCapture(e.pointerId);
        useLayout.getState().setDragging(true);
      }}
      onPointerMove={(e) => start.current && onLive(at(e))}
      onPointerUp={(e) => {
        if (!start.current) return;
        useSidebar.getState().setWidth(at(e));
        start.current = null;
        onLive(null);
        useLayout.getState().setDragging(false);
      }}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
          e.preventDefault();
          useSidebar.getState().setWidth(width + (e.key === "ArrowLeft" ? 16 : -16));
        }
      }}
      className="absolute inset-y-0 -left-1 z-10 w-2 cursor-col-resize outline-none after:absolute after:inset-y-0 after:left-[3px] after:w-0.5 after:bg-transparent hover:after:bg-accent focus-visible:after:bg-accent"
    />
  );
}

/** A browser tab: its page, and "Move to a pane" in Code mode. */
function BrowserTab({ tab }: { tab: SidebarTab }) {
  const { show } = useToast();
  const code = useUi((s) => s.mode === "code");
  const codeTab = useWorkspaces((s) => (s.activeId ? s.activeTab[s.activeId] : undefined));
  const actions: MenuEntry[] =
    code && codeTab
      ? [
          {
            id: "to-pane",
            label: t("browser.moveToPane"),
            onSelect: () =>
              void moveToPane(tab).then((message) => message && show({ tone: "error", title: message })),
          },
        ]
      : [];
  return <BrowserView id={tab.id} url={tab.url} actions={actions} />;
}

function Panel({ tab, shown }: { tab: SidebarTab; shown: boolean }) {
  if (tab.kind === "dashboard") return <DashboardPanel visible={shown} />;
  if (tab.kind === "files") return <FilesPanel />;
  if (tab.kind === "browser") return <BrowserTab tab={tab} />;
  if (tab.kind === "checkpoints") return <CheckpointsPanel />;
  if (tab.kind === "ports") return <PortsPanel visible={shown} />;
  if (tab.kind === "scratchpad") return <ScratchpadPanel />;
  if (tab.kind === "tasks") return <TasksPanel />;
  return <EmptyState title={t("sidebar.soon")} />;
}

/** UI §3.5 / F-SH-10: the right sidebar. Docked, it takes its width from
 * the main area; below 1,100 px it floats over it; full-screen overlays the
 * content without resizing it (a spacer keeps the docked width), so
 * terminals never reflow. Every tab stays mounted. */
export function Sidebar() {
  const keys = useShortcutKeys();
  const { open, tabs, active, width, fullscreen, pinned, loaded } = useSidebar();
  const narrow = useNarrow();
  const aside = useRef<HTMLElement>(null);
  const [liveWidth, setLiveWidth] = useState<number | null>(null);
  /** The "+" menu gives focus back to "+" unless an item moved it. */
  const focusMoved = useRef(false);
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));

  useEffect(() => {
    if (!loaded) void useSidebar.getState().load();
  }, [loaded]);

  // Esc leaves full-screen.
  useEffect(() => {
    if (!fullscreen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) {
        e.preventDefault();
        useSidebar.getState().setFullscreen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [fullscreen]);

  // Floating: an outside click closes it unless it's pinned (UI §3.5).
  useEffect(() => {
    if (!open || !narrow || pinned || fullscreen) return;
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element | null;
      if (!target || aside.current?.contains(target)) return;
      // Menus and dialogs portal out of the sidebar; the toggle toggles.
      if (
        target.closest("[data-radix-popper-content-wrapper],[role=dialog],[role=menu],[data-sidebar-toggle]")
      )
        return;
      useSidebar.getState().setOpen(false);
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [open, narrow, pinned, fullscreen]);

  const w = liveWidth ?? width;
  const floating = narrow && !fullscreen;
  const docked = !narrow;
  const plus: MenuEntry[] = [
    {
      id: "browser",
      label: t("sidebar.newBrowser"),
      onSelect: () => {
        // A new tab's address field takes the focus.
        focusAddressOnMount(useSidebar.getState().openTab("browser"));
        focusMoved.current = true;
      },
    },
    {
      id: "dashboard",
      label: t("sidebar.dashboard"),
      onSelect: () => useSidebar.getState().openTab("dashboard"),
    },
    { id: "files", label: t("sidebar.files"), onSelect: () => useSidebar.getState().openTab("files") },
    {
      id: "checkpoints",
      label: t("sidebar.checkpoints"),
      onSelect: () => useSidebar.getState().openTab("checkpoints"),
    },
    { id: "ports", label: t("sidebar.ports"), onSelect: () => useSidebar.getState().openTab("ports") },
    { id: "tasks", label: t("sidebar.tasks"), onSelect: () => useSidebar.getState().openTab("tasks") },
    {
      id: "scratchpad",
      label: t("sidebar.scratchpad"),
      onSelect: () => useSidebar.getState().openTab("scratchpad"),
    },
  ];
  const onDragEnd = (e: DragEndEvent) => {
    const over = e.over ? String(e.over.id) : null;
    const id = String(e.active.id);
    const ids = tabs.map((x) => x.id);
    if (!over || over === id) return;
    useSidebar.getState().reorder(arrayMove(ids, ids.indexOf(id), ids.indexOf(over)));
  };

  return (
    <>
      {/* Full-screen over a docked sidebar: keep its width in the flow so
          the main area doesn't resize (F-SH-10 AC2). */}
      {open && docked && fullscreen && <div aria-hidden="true" className="shrink-0" style={{ width: w }} />}
      <aside
        ref={aside}
        aria-label={t("sidebar.label")}
        data-testid="sidebar"
        data-placement={fullscreen ? "fullscreen" : floating ? "floating" : "docked"}
        style={{
          display: open ? "flex" : "none",
          width: fullscreen ? undefined : w,
          zIndex: fullscreen || floating ? "var(--z-sidebar)" : undefined,
        }}
        className={cx(
          "flex-col bg-surface-1",
          fullscreen
            ? "absolute inset-0"
            : floating
              ? "absolute inset-y-0 right-0 border-l border-border shadow-e3"
              : "relative shrink-0 border-l border-border",
        )}
      >
        {docked && !fullscreen && <Resizer width={width} onLive={setLiveWidth} />}
        <div className="flex h-8 shrink-0 items-center gap-1 border-b border-divider px-2">
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={onDragEnd}
            accessibility={{ screenReaderInstructions: { draggable: "" } }}
          >
            <SortableContext items={tabs.map((x) => x.id)} strategy={horizontalListSortingStrategy}>
              <ul
                role="tablist"
                aria-label={t("sidebar.label")}
                className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
              >
                {tabs.map((tab) => (
                  <Pill key={tab.id} tab={tab} active={tab.id === active} />
                ))}
              </ul>
            </SortableContext>
          </DndContext>
          <Menu
            align="end"
            trigger={
              <IconButton
                size="sm"
                icon={<Plus size={14} />}
                label={t("sidebar.openIn")}
                data-testid="sidebar-plus"
              />
            }
            entries={plus}
            onCloseAutoFocus={(e) => {
              if (focusMoved.current) e.preventDefault();
              focusMoved.current = false;
            }}
          />
          {floating && (
            <IconButton
              size="sm"
              icon={pinned ? <PinOff size={13} /> : <Pin size={13} />}
              label={pinned ? t("sidebar.unpin") : t("sidebar.pin")}
              pressed={pinned}
              onClick={() => useSidebar.getState().togglePin()}
            />
          )}
          <IconButton
            size="sm"
            data-testid="sidebar-fullscreen"
            icon={fullscreen ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
            label={fullscreen ? t("sidebar.exitFullscreen") : t("sidebar.fullscreen")}
            pressed={fullscreen}
            onClick={() => useSidebar.getState().setFullscreen(!fullscreen)}
          />
          <IconButton
            size="sm"
            icon={<X size={14} />}
            label={t("sidebar.close")}
            kbd={keys("sidebar.toggle")}
            onClick={() => useSidebar.getState().setOpen(false)}
          />
        </div>
        <div className="relative min-h-0 flex-1">
          {tabs.length === 0 && <EmptyState title={t("sidebar.empty")} />}
          {tabs.map((tab) => (
            <div
              key={tab.id}
              role="tabpanel"
              id={`sidebar-panel-${tab.id}`}
              aria-labelledby={`sidebar-tab-${tab.id}`}
              data-testid={`sidebar-panel-${tab.kind}`}
              className="absolute inset-0 overflow-auto"
              style={{ display: tab.id === active ? "block" : "none" }}
            >
              <Panel tab={tab} shown={open && tab.id === active} />
            </div>
          ))}
        </div>
      </aside>
    </>
  );
}
