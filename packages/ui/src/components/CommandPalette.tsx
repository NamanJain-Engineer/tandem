import { Dialog as D } from "radix-ui";
import { Command, defaultFilter } from "cmdk";
import { Search } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Kbd } from "./Kbd";

export type PaletteItem = {
  id: string;
  label: string;
  icon?: ReactNode;
  description?: string;
  kbd?: string[];
  keywords?: string[];
  onSelect: () => void;
};

/** Group headings per UI_SPEC §2.22; `prefix` narrows to one group. */
export type PaletteGroup = { heading: string; prefix?: ">" | "@" | "#" | "/" | "?"; items: PaletteItem[] };

export type CommandPaletteProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  groups: PaletteGroup[];
  placeholder?: string;
  emptyText?: (q: string) => string;
  title?: string;
  recentKey?: string;
  recentHeading?: string;
  /**
   * False when the caller searches itself (Find work, F-CN-05): rows show
   * as given, prefixes and recents are off, and `onQueryChange` gets the text.
   */
  shouldFilter?: boolean;
  onQueryChange?: (query: string) => void;
  /** A new value clears the input, like reopening (switching what it searches). */
  resetKey?: string;
};

const RECENT_MAX = 10;

function readRecent(key: string): string[] {
  try {
    const raw = localStorage.getItem(key);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

/** Mod+K palette shell (UI_SPEC §2.22): combobox + listbox in a dialog. */
export function CommandPalette({
  open,
  onOpenChange,
  groups,
  placeholder = "Type a command or search…",
  emptyText = (q) => `No matches for "${q}"`,
  title = "Command palette",
  recentKey = "tandem.palette.recent",
  recentHeading = "Suggestions",
  shouldFilter = true,
  onQueryChange,
  resetKey,
}: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [recent, setRecent] = useState<string[]>([]);
  useEffect(() => {
    if (open) {
      setQuery("");
      onQueryChange?.("");
      setRecent(shouldFilter ? readRecent(recentKey) : []);
    }
    // onQueryChange is the caller's callback; only these say "start over".
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, recentKey, resetKey, shouldFilter]);
  const changeQuery = (q: string) => {
    setQuery(q);
    onQueryChange?.(q);
  };

  const prefix = shouldFilter
    ? ([">", "@", "#", "/", "?"] as const).find((p) => query.startsWith(p))
    : undefined;
  const search = prefix ? query.slice(1).trimStart() : query;
  const visible = useMemo(
    () => (prefix ? groups.filter((g) => g.prefix === prefix) : groups),
    [groups, prefix],
  );
  const all = useMemo(() => new Map(groups.flatMap((g) => g.items).map((i) => [i.id, i])), [groups]);
  const recentItems =
    !prefix && !search ? recent.map((id) => all.get(id)).filter((i): i is PaletteItem => !!i) : [];

  const run = (item: PaletteItem) => {
    if (shouldFilter) {
      const next = [item.id, ...recent.filter((r) => r !== item.id)].slice(0, RECENT_MAX);
      try {
        localStorage.setItem(recentKey, JSON.stringify(next));
      } catch {
        /* storage can be unavailable */
      }
    }
    onOpenChange(false);
    item.onSelect();
  };

  const row = (item: PaletteItem, key: string) => (
    <Command.Item
      key={key}
      value={`${key} ${item.label} ${item.keywords?.join(" ") ?? ""}`}
      onSelect={() => run(item)}
      className="flex h-9 cursor-default items-center gap-2.5 rounded-sm px-2.5 text-base text-fg outline-none data-[selected=true]:bg-surface-4"
    >
      {item.icon && (
        <span aria-hidden="true" className="inline-flex text-fg-muted">
          {item.icon}
        </span>
      )}
      <span className="truncate">{item.label}</span>
      {item.description && <span className="truncate text-sm text-fg-subtle">{item.description}</span>}
      {item.kbd && <Kbd keys={item.kbd} className="ml-auto" />}
    </Command.Item>
  );

  return (
    <D.Root open={open} onOpenChange={onOpenChange}>
      <D.Portal>
        <D.Overlay className="tn-fade fixed inset-0 z-[var(--z-command)] bg-overlay" />
        <D.Content
          aria-describedby={undefined}
          className="tn-dialog fixed left-1/2 top-[18vh] z-[var(--z-command)] w-[640px] max-w-[calc(100vw-32px)] -translate-x-1/2 overflow-hidden rounded-lg border border-border bg-surface-3 shadow-3 focus:outline-none"
        >
          <D.Title className="sr-only">{title}</D.Title>
          <Command
            label={title}
            shouldFilter={shouldFilter}
            filter={(value, s, keywords) => defaultFilter(value, s.replace(/^[>@#/?]\s*/, ""), keywords)}
          >
            <div className="flex items-center gap-2 border-b border-divider px-3">
              <Search size={16} aria-hidden="true" className="text-fg-subtle" />
              <Command.Input
                value={query}
                onValueChange={changeQuery}
                placeholder={placeholder}
                className="h-11 flex-1 bg-transparent text-md text-fg placeholder:text-fg-subtle focus:outline-none"
              />
            </div>
            <Command.List className="max-h-[50vh] overflow-y-auto p-1.5">
              <Command.Empty className="px-3 py-6 text-center text-base text-fg-muted">
                {emptyText(search)}
              </Command.Empty>
              {recentItems.length > 0 && (
                <Command.Group heading={recentHeading} className="tn-cmd-group">
                  {recentItems.map((i) => row(i, `recent:${i.id}`))}
                </Command.Group>
              )}
              {visible.map((g) => (
                <Command.Group key={g.heading} heading={g.heading} className="tn-cmd-group">
                  {g.items.map((i) => row(i, i.id))}
                </Command.Group>
              ))}
            </Command.List>
          </Command>
        </D.Content>
      </D.Portal>
    </D.Root>
  );
}
