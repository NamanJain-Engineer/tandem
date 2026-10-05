import type { HTMLAttributes, ReactNode, Ref } from "react";
import { GripVertical, Maximize2, Minimize2, MoreHorizontal, SquareTerminal, X } from "lucide-react";
import { IconButton, Menu, StatusDot, cx, type MenuEntry, type Status } from "@tandem/ui";
import { t } from "../../i18n";

export type PaneChromeProps = {
  title: string;
  status: Status;
  focused: boolean;
  /** Alone in its tab: no maximize control (F-CD-02). */
  solo: boolean;
  maximized: boolean;
  menuItems: MenuEntry[];
  onMaximize: () => void;
  onClose: () => void;
  /** The drag source (dnd-kit) for moving the pane. */
  dragRef?: Ref<HTMLDivElement>;
  dragProps?: HTMLAttributes<HTMLDivElement>;
  glyph?: ReactNode;
};

/** UI_SPEC §2.20: the 30 px header of every pane. The whole header drags
 * the pane, except its buttons. */
export function PaneChrome({
  title,
  status,
  focused,
  solo,
  maximized,
  menuItems,
  onMaximize,
  onClose,
  dragRef,
  dragProps,
  glyph,
}: PaneChromeProps) {
  return (
    <div
      ref={dragRef}
      {...dragProps}
      role="toolbar"
      aria-label={title}
      data-testid="pane-chrome"
      className={cx(
        "group/chrome flex h-[30px] shrink-0 cursor-grab items-center gap-1.5 border-b border-divider bg-surface-2 pl-1 pr-1 active:cursor-grabbing",
        "border-t",
        focused ? "border-t-accent" : "border-t-transparent",
      )}
    >
      <GripVertical
        size={12}
        aria-hidden="true"
        className="shrink-0 text-fg-subtle opacity-0 group-hover/chrome:opacity-100"
      />
      <StatusDot status={status} size={6} />
      <span className="inline-flex shrink-0 text-fg-muted" aria-hidden="true">
        {glyph ?? <SquareTerminal size={13} />}
      </span>
      <span data-testid="pane-title" className="min-w-0 flex-1 truncate text-sm text-fg">
        {title}
      </span>
      {/* Buttons don't start a drag. */}
      <div
        className="flex shrink-0 items-center"
        onPointerDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
      >
        {!solo && (
          <IconButton
            size="sm"
            icon={maximized ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
            label={maximized ? t("pane.restore") : t("pane.maximize")}
            kbd={["Mod", "Shift", "Enter"]}
            pressed={maximized}
            onClick={onMaximize}
          />
        )}
        <Menu
          align="end"
          trigger={
            <IconButton
              size="sm"
              icon={<MoreHorizontal size={14} />}
              label={t("pane.menu", { name: title })}
            />
          }
          entries={menuItems}
        />
        <IconButton
          size="sm"
          icon={<X size={14} />}
          label={t("pane.close", { name: title })}
          kbd={["Mod", "W"]}
          onClick={onClose}
        />
      </div>
    </div>
  );
}
