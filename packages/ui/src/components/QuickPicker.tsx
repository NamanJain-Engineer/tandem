import { Dialog as D } from "radix-ui";
import { Command } from "cmdk";
import { Search } from "lucide-react";
import { type KeyboardEvent, type ReactNode } from "react";
import { Kbd } from "./Kbd";

export type QuickPickerItem = {
  id: string;
  /** The row's content; the picker adds selection styling. */
  content: ReactNode;
  /** What screen readers read for the row. */
  label: string;
};

export type QuickPickerProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  placeholder: string;
  /** Controlled query: the caller filters (often in the core). */
  query: string;
  onQueryChange: (q: string) => void;
  items: QuickPickerItem[];
  /** Enter; `mod` is true for Mod+Enter. */
  onSelect: (id: string, mod: boolean) => void;
  /** Mod+Backspace on the highlighted row. */
  onDelete?: (id: string) => void;
  /** Under the input (filters). */
  header?: ReactNode;
  /** Shown instead of the list: loading, error, empty and off states. */
  state?: ReactNode;
  /** Key hints at the bottom, e.g. [["↩", "Insert"], ["⌘", "↩", "Run"]]. */
  hints?: Array<{ keys: string[]; label: string }>;
  value?: string;
  onValueChange?: (id: string) => void;
};

/** A palette-shaped picker over a list the caller provides (UI_SPEC §2.22
 * shell; UI-141, UI-145): the same dialog, input and rows as the command
 * palette, with Mod+Enter and Mod+Backspace for a second and a delete action. */
export function QuickPicker({
  open,
  onOpenChange,
  title,
  placeholder,
  query,
  onQueryChange,
  items,
  onSelect,
  onDelete,
  header,
  state,
  hints,
  value,
  onValueChange,
}: QuickPickerProps) {
  const current = () => value ?? items[0]?.id;
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key === "Enter") {
      const id = current();
      e.preventDefault();
      if (id) onSelect(id, true);
    } else if (mod && e.key === "Backspace" && onDelete) {
      const id = current();
      e.preventDefault();
      if (id) onDelete(id);
    }
  };
  return (
    <D.Root open={open} onOpenChange={onOpenChange}>
      <D.Portal>
        <D.Overlay className="tn-fade fixed inset-0 z-[var(--z-command)] bg-overlay" />
        <D.Content
          aria-describedby={undefined}
          data-testid="quick-picker"
          className="tn-dialog fixed left-1/2 top-[18vh] z-[var(--z-command)] w-[680px] max-w-[calc(100vw-32px)] -translate-x-1/2 overflow-hidden rounded-lg border border-border bg-surface-3 shadow-3 focus:outline-none"
        >
          <D.Title className="sr-only">{title}</D.Title>
          <Command label={title} shouldFilter={false} value={value} onValueChange={onValueChange} onKeyDown={onKeyDown}>
            <div className="flex items-center gap-2 border-b border-divider px-3">
              <Search size={16} aria-hidden="true" className="text-fg-subtle" />
              <Command.Input
                value={query}
                onValueChange={onQueryChange}
                placeholder={placeholder}
                className="h-11 flex-1 bg-transparent text-md text-fg placeholder:text-fg-subtle focus:outline-none"
              />
            </div>
            {header && <div className="border-b border-divider px-3 py-2">{header}</div>}
            <Command.List className="max-h-[50vh] overflow-y-auto p-1.5">
              {state ??
                items.map((item) => (
                  <Command.Item
                    key={item.id}
                    value={item.id}
                    aria-label={item.label}
                    onSelect={() => onSelect(item.id, false)}
                    className="flex min-h-9 cursor-default items-center gap-2.5 rounded-sm px-2.5 py-1 text-base text-fg outline-none data-[selected=true]:bg-surface-4"
                  >
                    {item.content}
                  </Command.Item>
                ))}
            </Command.List>
            {hints && hints.length > 0 && (
              <div className="flex items-center gap-4 border-t border-divider px-3 py-1.5 text-xs text-fg-muted">
                {hints.map((h) => (
                  <span key={h.label} className="inline-flex items-center gap-1.5">
                    <Kbd keys={h.keys} />
                    {h.label}
                  </span>
                ))}
              </div>
            )}
          </Command>
        </D.Content>
      </D.Portal>
    </D.Root>
  );
}
