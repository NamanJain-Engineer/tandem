import { useRef, useState, type HTMLAttributes, type ReactNode, type Ref } from "react";
import { GripVertical, Maximize2, Minimize2, MoreHorizontal, SquareTerminal, X } from "lucide-react";
import { IconButton, Menu, StatusDot, Tooltip, cx, type MenuEntry, type Status } from "@tandem/ui";
import { t } from "../../i18n";
import { useShortcutKeys } from "../commands/useShortcutKeys";

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
  /** Shown after the title (e.g. "Lite" when rendering without WebGL). */
  badges?: ReactNode;
  /** The agent's first name (F-CD-12), before the title. */
  name?: string | null;
  /** Location: cwd relative to the workspace. */
  meta?: string | null;
  /** After the location: the worktree branch chip (UI_SPEC §4.3). */
  location?: ReactNode;
  /** Renames the pane; resolves to an error message, or null when done. */
  onRename?: (name: string) => Promise<string | null>;
  /** Asked from the menu: start editing the name. */
  renaming?: boolean;
  onRenamingChange?: (renaming: boolean) => void;
};

/** The name, editable in place (double-click, or Rename in the menu). */
function PaneName({
  name,
  onRename,
  editing,
  setEditing,
}: {
  name: string;
  onRename?: (name: string) => Promise<string | null>;
  editing: boolean;
  setEditing: (on: boolean) => void;
}) {
  const [draft, setDraft] = useState(name);
  const [error, setError] = useState<string | null>(null);
  const settled = useRef(false);
  const start = () => {
    if (!onRename) return;
    settled.current = false;
    setDraft(name);
    setError(null);
    setEditing(true);
  };
  const done = async (next: string | null) => {
    if (settled.current) return;
    settled.current = true;
    const value = next?.trim();
    if (!value || value === name || !onRename) {
      setEditing(false);
      return;
    }
    const err = await onRename(value);
    if (err) {
      settled.current = false;
      setError(err);
      return;
    }
    setEditing(false);
  };
  if (editing)
    return (
      <span className="relative inline-flex shrink-0" onPointerDown={(e) => e.stopPropagation()}>
        <input
          autoFocus
          aria-label={t("pane.renameField", { name })}
          aria-invalid={error ? true : undefined}
          value={draft}
          maxLength={32}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") void done(draft);
            if (e.key === "Escape") {
              settled.current = true;
              setEditing(false);
            }
          }}
          onBlur={() => void done(draft)}
          className={cx(
            "h-5 w-28 rounded-xs border bg-surface-1 px-1 text-sm text-fg outline-none",
            error ? "border-error" : "border-accent",
          )}
        />
        {error && (
          <span
            role="alert"
            className="absolute left-0 top-6 z-10 whitespace-nowrap rounded-xs bg-surface-3 px-1.5 py-0.5 text-xs text-error shadow-2"
          >
            {error}
          </span>
        )}
      </span>
    );
  return (
    <span
      data-testid="pane-name"
      onDoubleClick={start}
      className="shrink-0 truncate text-sm font-semibold text-fg"
    >
      {name}
    </span>
  );
}

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
  badges,
  name,
  meta,
  location,
  onRename,
  renaming = false,
  onRenamingChange,
}: PaneChromeProps) {
  const keys = useShortcutKeys();
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
      <span data-testid="pane-status" data-status={status} className="inline-flex shrink-0">
        <StatusDot status={status} size={6} />
      </span>
      <span className="inline-flex shrink-0 text-fg-muted" aria-hidden="true">
        {glyph ?? <SquareTerminal size={13} />}
      </span>
      {name && (
        <PaneName
          name={name}
          onRename={onRename}
          editing={renaming}
          setEditing={(on) => onRenamingChange?.(on)}
        />
      )}
      {/* Never narrower than 12 characters; the full title is in the tooltip. */}
      <Tooltip label={title}>
        <span
          data-testid="pane-title"
          className={cx("min-w-[12ch] truncate text-sm", name ? "text-fg-muted" : "text-fg")}
        >
          {title}
        </span>
      </Tooltip>
      {meta && (
        <span data-testid="pane-meta" className="min-w-0 truncate text-sm text-fg-subtle">
          · {meta}
        </span>
      )}
      {location}
      <span className="flex-1" />
      {badges}
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
            kbd={keys("pane.maximize")}
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
          kbd={keys("pane.close")}
          onClick={onClose}
        />
      </div>
    </div>
  );
}
