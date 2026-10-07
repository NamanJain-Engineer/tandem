import { useMemo, type ReactNode } from "react";
import { create } from "zustand";
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  pointerWithin,
  useDroppable,
  useSensor,
  useSensors,
  type Announcements,
  type DragEndEvent,
  type DragOverEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import { cx } from "@tandem/ui";
import { t } from "../../i18n";
import { useLayout } from "../../stores/layout";
import { useWorkspaces } from "../../stores/workspaces";
import type { Side } from "./layout/tree";

// Dragging a pane header (F-CD-02): the centre of another pane swaps, its
// edges split, and the tab strip makes a new tab. Targets are dnd-kit
// droppables, measured when the drag starts, so hit-testing happens per
// enter/leave rather than per pointer move. Esc cancels (dnd-kit's pointer
// sensor) and nothing changes until the drop.

export type Zone = "center" | Side;
export type PaneDrag = { kind: "pane"; paneId: string; tabId: string; title: string };
export type DropTarget =
  { kind: "zone"; paneId: string; tabId: string; zone: Zone } | { kind: "new_tab"; workspaceId: string };

type DragUi = { active: PaneDrag | null; over: DropTarget | null };
export const useDrag = create<DragUi>(() => ({ active: null, over: null }));

export function outcomeLabel(target: DropTarget | null): string | null {
  if (!target) return null;
  if (target.kind === "new_tab") return t("drop.newTab");
  return target.zone === "center" ? t("drop.swap") : t(`drop.${target.zone}`);
}

/** What a drop does to the layout. */
export async function applyDrop(drag: PaneDrag, target: DropTarget): Promise<void> {
  const layout = useLayout.getState();
  if (target.kind === "new_tab") {
    const tab = await layout.moveToNewTab(drag.tabId, drag.paneId);
    if (tab) {
      await useWorkspaces.getState().refresh();
      useWorkspaces.getState().selectTab(tab.workspaceId, tab.id);
    }
    return;
  }
  if (target.paneId === drag.paneId || target.tabId !== drag.tabId) return;
  if (target.zone === "center") layout.swap(drag.tabId, drag.paneId, target.paneId);
  else layout.move(drag.tabId, drag.paneId, target.paneId, target.zone);
}

const announcements: Announcements = {
  onDragStart: ({ active }) => (active.data.current as PaneDrag | undefined)?.title ?? "",
  onDragOver: ({ over }) => outcomeLabel((over?.data.current as DropTarget | undefined) ?? null) ?? "",
  onDragEnd: ({ over }) => outcomeLabel((over?.data.current as DropTarget | undefined) ?? null) ?? "",
  onDragCancel: () => t("drop.cancelled"),
};

export function PaneDndProvider({ children }: { children: ReactNode }) {
  // A drag starts only after 4 px of movement (F-CD-02).
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));
  const active = useDrag((s) => s.active);
  const over = useDrag((s) => s.over);
  const label = useMemo(() => outcomeLabel(over), [over]);

  const onDragStart = (e: DragStartEvent) =>
    useDrag.setState({ active: (e.active.data.current as PaneDrag | undefined) ?? null, over: null });
  const onDragOver = (e: DragOverEvent) =>
    useDrag.setState({ over: (e.over?.data.current as DropTarget | undefined) ?? null });
  const onDragEnd = (e: DragEndEvent) => {
    const drag = e.active.data.current as PaneDrag | undefined;
    const target = e.over?.data.current as DropTarget | undefined;
    useDrag.setState({ active: null, over: null });
    if (drag && target) void applyDrop(drag, target);
  };
  const onDragCancel = () => useDrag.setState({ active: null, over: null });

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={pointerWithin}
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDragEnd={onDragEnd}
      onDragCancel={onDragCancel}
      accessibility={{ announcements, screenReaderInstructions: { draggable: "" } }}
    >
      {children}
      <DragOverlay dropAnimation={null}>
        {active && (
          <div
            data-testid="drag-ghost"
            className="inline-flex max-w-[280px] items-center gap-2 rounded-md border border-border-strong bg-surface-3 px-3 py-1.5 text-sm text-fg shadow-2"
          >
            <span className="truncate">{active.title}</span>
            {label && <span className="shrink-0 font-semibold text-accent">{label}</span>}
          </div>
        )}
      </DragOverlay>
    </DndContext>
  );
}

const ZONES: Array<{ zone: Zone; className: string }> = [
  { zone: "left", className: "left-0 top-0 h-full w-1/4" },
  { zone: "right", className: "right-0 top-0 h-full w-1/4" },
  { zone: "top", className: "left-1/4 top-0 h-1/4 w-1/2" },
  { zone: "bottom", className: "bottom-0 left-1/4 h-1/4 w-1/2" },
  { zone: "center", className: "left-1/4 top-1/4 h-1/2 w-1/2" },
];

/** Where the drop lands, drawn over the target pane. */
const PREVIEW: Record<Zone, string> = {
  left: "left-0 top-0 h-full w-1/2",
  right: "right-0 top-0 h-full w-1/2",
  top: "left-0 top-0 h-1/2 w-full",
  bottom: "bottom-0 left-0 h-1/2 w-full",
  center: "inset-0",
};

function ZoneTarget({
  paneId,
  tabId,
  zone,
  className,
  disabled,
}: {
  paneId: string;
  tabId: string;
  zone: Zone;
  className: string;
  disabled: boolean;
}) {
  const data: DropTarget = { kind: "zone", paneId, tabId, zone };
  const { setNodeRef } = useDroppable({ id: `zone:${paneId}:${zone}`, data, disabled });
  return <div ref={setNodeRef} data-zone={zone} className={cx("pointer-events-none absolute", className)} />;
}

/** The five drop zones of a pane (UI_SPEC §2.21). */
export function DropZones({ paneId, tabId }: { paneId: string; tabId: string }) {
  const active = useDrag((s) => s.active);
  const over = useDrag((s) => s.over);
  const self = active?.paneId === paneId;
  const hovered = over?.kind === "zone" && over.paneId === paneId ? over.zone : null;
  return (
    <div aria-hidden="true" className="pointer-events-none absolute inset-0 z-10">
      {ZONES.map((z) => (
        <ZoneTarget
          key={z.zone}
          paneId={paneId}
          tabId={tabId}
          zone={z.zone}
          className={z.className}
          disabled={self || (active !== null && active.tabId !== tabId)}
        />
      ))}
      {hovered && (
        <div
          data-testid="drop-preview"
          data-zone={hovered}
          className={cx("absolute rounded-sm border border-accent bg-accent-soft", PREVIEW[hovered])}
        />
      )}
    </div>
  );
}

/** The tab strip as a drop target: "New tab". */
export function useNewTabTarget(workspaceId: string) {
  const data: DropTarget = { kind: "new_tab", workspaceId };
  const { setNodeRef, isOver } = useDroppable({ id: `newtab:${workspaceId}`, data });
  return { setNodeRef, isOver };
}
