import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import { SortableContext, arrayMove, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { ContextMenu, Tooltip, cx, type MenuEntry } from "@tandem/ui";
import { t } from "../../i18n";

/** The divider between the Pinned band and All. Dragging a row above it pins
 * the row; below it unpins (F-SH-03). */
const BAND = "__band__";

export type RailItem = { id: string; pinned: boolean };

export type RailRowParts = {
  icon: ReactNode;
  name: string;
  /** Right slot: status, count or age. */
  trailing?: ReactNode;
  tooltip?: string;
  /** Folder not found and similar (warning style). */
  warning?: boolean;
  /** Content under an expanded row; rows without it have no caret. */
  children?: ReactNode;
  testId?: string;
  data?: Record<string, string>;
};

export type RailListProps<T extends RailItem> = {
  label: string;
  items: T[];
  selectedId: string | null;
  collapsed: boolean;
  row: (item: T) => RailRowParts;
  menu: (
    item: T,
    actions: { rename: () => void; moveUp: () => void; moveDown: () => void; togglePin: () => void },
  ) => MenuEntry[];
  onOpen: (item: T) => void;
  onMove: (id: string, afterId: string | null, pinned: boolean) => void;
  onRename?: (item: T, name: string) => void;
  onRemove?: (item: T) => void;
  expanded?: Record<string, boolean>;
  onExpand?: (item: T, open: boolean) => void;
};

/** Where a row lands after a reorder: its band and the row before it there. */
export function placement(order: string[], id: string): { afterId: string | null; pinned: boolean } {
  const band = order.indexOf(BAND);
  const i = order.indexOf(id);
  const pinned = i < band;
  const before = order[i - 1];
  return { pinned, afterId: before && before !== BAND ? before : null };
}

function Row<T extends RailItem>({
  item,
  parts,
  index,
  selected,
  focusable,
  collapsed,
  renaming,
  expanded,
  onOpen,
  onKey,
  onRenameDone,
  onToggle,
  menu,
  setRef,
}: {
  item: T;
  parts: RailRowParts;
  index: number;
  selected: boolean;
  focusable: boolean;
  collapsed: boolean;
  renaming: boolean;
  expanded: boolean;
  onOpen: () => void;
  onKey: (e: KeyboardEvent) => void;
  onRenameDone: (name: string | null) => void;
  onToggle: (() => void) | null;
  menu: MenuEntry[];
  setRef: (el: HTMLDivElement | null) => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: item.id,
  });
  const [draft, setDraft] = useState(parts.name);
  // Enter or Escape settles the edit; the blur that follows must not commit again.
  const settled = useRef(false);
  useEffect(() => {
    if (!renaming) return;
    settled.current = false;
    setDraft(parts.name);
  }, [renaming, parts.name]);
  const settle = (name: string | null) => {
    if (settled.current) return;
    settled.current = true;
    onRenameDone(name);
  };
  const style = { transform: CSS.Translate.toString(transform), transition };
  const content = (
    <div
      ref={(el) => {
        setNodeRef(el);
        setRef(el);
      }}
      {...attributes}
      {...listeners}
      role="treeitem"
      aria-level={1}
      aria-selected={selected}
      aria-expanded={parts.children !== undefined ? expanded : undefined}
      aria-posinset={index + 1}
      aria-roledescription={undefined}
      aria-describedby={undefined}
      tabIndex={focusable ? 0 : -1}
      data-testid={parts.testId}
      data-pinned={item.pinned ? "true" : undefined}
      {...Object.fromEntries(Object.entries(parts.data ?? {}).map(([k, v]) => [`data-${k}`, v]))}
      title={collapsed ? undefined : parts.tooltip}
      style={style}
      onClick={onOpen}
      onDoubleClick={(e) => {
        e.preventDefault();
        if (!collapsed)
          onKey({ ...e, key: "F2", preventDefault: () => undefined } as unknown as KeyboardEvent);
      }}
      onKeyDown={onKey}
      className={cx(
        "group relative flex h-[30px] cursor-default select-none items-center gap-1.5 rounded-sm px-2 text-base outline-none hover:bg-surface-4 focus-visible:shadow-focus",
        parts.warning ? "text-warning" : "text-fg",
        selected && "bg-accent-soft hover:bg-accent-soft",
        isDragging && "z-10 opacity-80 shadow-2",
        collapsed && "justify-center px-0",
      )}
    >
      {!collapsed && onToggle && (
        <button
          type="button"
          tabIndex={-1}
          aria-hidden="true"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation();
            onToggle();
          }}
          className="-ml-1 inline-flex size-4 shrink-0 items-center justify-center rounded-xs text-fg-subtle hover:text-fg"
        >
          {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </button>
      )}
      <span className="inline-flex shrink-0">{parts.icon}</span>
      {!collapsed &&
        (renaming ? (
          <input
            autoFocus
            aria-label={t("rail.renameField", { name: parts.name })}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") settle(draft);
              if (e.key === "Escape") settle(null);
            }}
            onBlur={() => settle(draft)}
            className="h-6 min-w-0 flex-1 rounded-xs border border-accent bg-surface-2 px-1 text-base text-fg outline-none"
          />
        ) : (
          <>
            {/* The name keeps at least 10 characters; the trailing slot
                gives way first (D-2603). */}
            <span className="min-w-[min(10ch,100%)] flex-1 truncate" data-testid="rail-row-name">
              {parts.name}
            </span>
            {parts.trailing && (
              <span className="flex min-w-0 shrink items-center justify-end gap-1.5">{parts.trailing}</span>
            )}
          </>
        ))}
    </div>
  );
  return (
    <ContextMenu entries={menu}>
      {collapsed ? (
        <Tooltip label={parts.tooltip ?? parts.name} side="right">
          {content}
        </Tooltip>
      ) : (
        content
      )}
    </ContextMenu>
  );
}

/** UI_SPEC §3.3: the list every mode's rail uses. Pinned band and All, drag
 * to reorder or pin, context menu, inline rename, keyboard navigation. */
export function RailList<T extends RailItem>({
  label,
  items,
  selectedId,
  collapsed,
  row,
  menu,
  onOpen,
  onMove,
  onRename,
  onRemove,
  expanded = {},
  onExpand,
}: RailListProps<T>) {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [pinnedOpen, setPinnedOpen] = useState(true);
  const [dragging, setDragging] = useState(false);
  const refs = useRef(new Map<string, HTMLDivElement | null>());
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));

  const pinned = items.filter((i) => i.pinned);
  const rest = items.filter((i) => !i.pinned);
  const visible = useMemo(() => [...(pinnedOpen ? pinned : []), ...rest], [pinnedOpen, pinned, rest]);
  const order = useMemo(() => [...pinned.map((i) => i.id), BAND, ...rest.map((i) => i.id)], [pinned, rest]);
  const byId = useMemo(() => new Map(items.map((i) => [i.id, i])), [items]);

  const moveBy = (item: T, delta: -1 | 1) => {
    const band = item.pinned ? pinned : rest;
    const i = band.findIndex((x) => x.id === item.id);
    const j = i + delta;
    if (j < 0 || j >= band.length) return;
    // Moving down puts it after the next row; up, after the row two above.
    const after = delta === 1 ? band[j] : band[j - 1];
    onMove(item.id, after?.id ?? null, item.pinned);
  };
  const togglePin = (item: T) =>
    onMove(item.id, item.pinned ? null : (pinned.at(-1)?.id ?? null), !item.pinned);

  const onDragEnd = (e: DragEndEvent) => {
    setDragging(false);
    const id = String(e.active.id);
    const over = e.over ? String(e.over.id) : null;
    if (!over || over === id) return;
    const next = arrayMove(order, order.indexOf(id), order.indexOf(over));
    const { afterId, pinned: nowPinned } = placement(next, id);
    const item = byId.get(id);
    if (item) onMove(id, afterId, nowPinned);
  };

  const focusRow = (index: number) => {
    const target = visible[Math.max(0, Math.min(visible.length - 1, index))];
    if (target) refs.current.get(target.id)?.focus();
  };

  const onKey = (item: T, index: number) => (e: KeyboardEvent) => {
    const has = row(item).children !== undefined;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      focusRow(index + 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      focusRow(index - 1);
    } else if (e.key === "Home") {
      e.preventDefault();
      focusRow(0);
    } else if (e.key === "End") {
      e.preventDefault();
      focusRow(visible.length - 1);
    } else if (e.key === "ArrowRight" && has) {
      e.preventDefault();
      onExpand?.(item, true);
    } else if (e.key === "ArrowLeft" && has) {
      e.preventDefault();
      onExpand?.(item, false);
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onOpen(item);
    } else if (e.key === "F2" && onRename) {
      e.preventDefault();
      setRenaming(item.id);
    } else if (e.key === "Delete" && onRemove) {
      e.preventDefault();
      onRemove(item);
    }
  };

  const selectedVisible = visible.some((i) => i.id === selectedId);
  const renderRow = (item: T, index: number) => {
    const parts = row(item);
    const open = expanded[item.id] ?? false;
    return (
      <div key={item.id} role="none">
        <Row
          item={item}
          parts={parts}
          index={index}
          selected={item.id === selectedId}
          focusable={item.id === selectedId || (!selectedVisible && index === 0)}
          collapsed={collapsed}
          renaming={renaming === item.id}
          expanded={open}
          onOpen={() => onOpen(item)}
          onKey={onKey(item, index)}
          onToggle={parts.children !== undefined && onExpand ? () => onExpand(item, !open) : null}
          onRenameDone={(name) => {
            setRenaming(null);
            const next = name?.trim();
            if (next && next !== parts.name) onRename?.(item, next);
            refs.current.get(item.id)?.focus();
          }}
          menu={menu(item, {
            rename: () => setRenaming(item.id),
            moveUp: () => moveBy(item, -1),
            moveDown: () => moveBy(item, 1),
            togglePin: () => togglePin(item),
          })}
          setRef={(el) => refs.current.set(item.id, el)}
        />
        {open && !collapsed && parts.children && (
          <div role="group" className="pb-1 pl-6">
            {parts.children}
          </div>
        )}
      </div>
    );
  };

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragStart={(_: DragStartEvent) => setDragging(true)}
      onDragCancel={() => setDragging(false)}
      onDragEnd={onDragEnd}
      accessibility={{ screenReaderInstructions: { draggable: "" } }}
    >
      <SortableContext items={order} strategy={verticalListSortingStrategy}>
        <div className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2">
          {pinned.length > 0 && !collapsed && (
            <button
              type="button"
              aria-expanded={pinnedOpen}
              onClick={() => setPinnedOpen((o) => !o)}
              className="flex h-6 w-full items-center gap-1 rounded-xs px-1 text-2xs font-semibold uppercase tracking-caps text-fg-subtle hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
            >
              {pinnedOpen ? (
                <ChevronDown size={11} aria-hidden="true" />
              ) : (
                <ChevronRight size={11} aria-hidden="true" />
              )}
              {t("rail.pinned")}
            </button>
          )}
          <div role="tree" aria-label={label}>
            {pinned.length > 0 && (
              <div role="group" aria-label={t("rail.pinned")}>
                {pinnedOpen && pinned.map((item, i) => renderRow(item, i))}
              </div>
            )}
            <Band
              visible={!collapsed && (pinned.length > 0 || dragging)}
              hint={pinned.length === 0 && dragging}
            />
            <div role="group" aria-label={t("rail.all")}>
              {rest.map((item, i) => renderRow(item, (pinnedOpen ? pinned.length : 0) + i))}
            </div>
          </div>
        </div>
      </SortableContext>
    </DndContext>
  );
}

function Band({ visible, hint }: { visible: boolean; hint: boolean }) {
  const { setNodeRef, transform, transition } = useSortable({ id: BAND, disabled: true });
  return (
    <div
      ref={setNodeRef}
      aria-hidden="true"
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={cx(!visible && "h-1")}
    >
      {visible && (
        <div
          className={cx(
            "flex h-6 items-center px-1 text-2xs font-semibold uppercase tracking-caps",
            hint ? "rounded-xs border border-dashed border-border-strong text-fg-muted" : "text-fg-subtle",
          )}
        >
          {hint ? t("rail.pinHere") : t("rail.all")}
        </div>
      )}
    </div>
  );
}
