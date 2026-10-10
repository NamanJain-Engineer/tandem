import { useCallback, useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { ChevronDown, ChevronRight, File, FileCode, FileImage, FileText, Folder, FolderOpen, Link2, RotateCcw, Search } from "lucide-react";
import { cx } from "../cx";
import { ContextMenu, type MenuEntry } from "./Menu";

export type FileTreeChange = "added" | "modified" | "deleted" | "renamed" | "type_changed" | "untracked" | "conflicted";

export type FileTreeNode = {
  /** Relative to the tree's root, `/`-separated; unique. */
  path: string;
  name: string;
  kind: "file" | "dir";
  symlink?: boolean;
  /** Matched by `.gitignore` (shown with "Show ignored"). */
  ignored?: boolean;
  gitStatus?: FileTreeChange | null;
};

export type FileTreeLabels = {
  tree: string;
  filter: string;
  loading: string;
  empty: string;
  noMatches: (query: string) => string;
  error: string;
  retry: string;
  ignored: string;
  symlink: string;
  status: Record<FileTreeChange, string>;
};

export const FILE_TREE_LABELS: FileTreeLabels = {
  tree: "Files",
  filter: "Filter files",
  loading: "Loading…",
  empty: "This folder is empty",
  noMatches: (q) => `No loaded files match "${q}"`,
  error: "Couldn't read this folder",
  retry: "Try again",
  ignored: "ignored",
  symlink: "link",
  status: {
    added: "added",
    modified: "modified",
    deleted: "deleted",
    renamed: "renamed",
    type_changed: "type changed",
    untracked: "untracked",
    conflicted: "conflicted",
  },
};

/** UI_SPEC §2.26: M warning, A success, D danger, U and R info; C (a conflict) danger. */
export const GIT_LETTER: Record<FileTreeChange, { letter: string; tone: string }> = {
  added: { letter: "A", tone: "text-success" },
  modified: { letter: "M", tone: "text-warning" },
  deleted: { letter: "D", tone: "text-error" },
  renamed: { letter: "R", tone: "text-info" },
  type_changed: { letter: "T", tone: "text-warning" },
  untracked: { letter: "U", tone: "text-info" },
  conflicted: { letter: "C", tone: "text-error" },
};

export const FILE_ROW_HEIGHT = 24;
/** Longer lists render only the rows in view. */
export const VIRTUALIZE_ABOVE = 400;
const OVERSCAN = 12;
const INDENT = 12;

export type FileTreeProps = {
  labels?: Partial<FileTreeLabels>;
  /** Each folder's entries by its path (`""` is the root); `undefined`: not loaded yet. */
  entries: Record<string, FileTreeNode[] | undefined>;
  /** Folders whose listing failed, with the message. */
  errors?: Record<string, string>;
  expanded: ReadonlySet<string>;
  selected?: string | null;
  onToggle: (dir: string, open: boolean) => void;
  onOpen: (node: FileTreeNode) => void;
  onSelect?: (path: string) => void;
  onRetry?: (dir: string) => void;
  /** The right-click / Menu-key menu for a row. */
  menu?: (node: FileTreeNode) => MenuEntry[];
  /** A pointer went down on a row: the app may start a drag out of the tree. */
  onRowPointerDown?: (node: FileTreeNode, event: PointerEvent<HTMLElement>) => void;
  /** Shown above the rows (the filter box is built in). */
  toolbar?: ReactNode;
  className?: string;
};

type Row =
  | { type: "node"; node: FileTreeNode; depth: number; setsize: number; posinset: number }
  | { type: "loading"; dir: string; depth: number }
  | { type: "empty"; dir: string; depth: number }
  | { type: "error"; dir: string; depth: number; message: string };

function parentOf(path: string): string {
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at);
}

function fileIcon(node: FileTreeNode, open: boolean): ReactNode {
  if (node.kind === "dir") return open ? <FolderOpen size={14} aria-hidden="true" /> : <Folder size={14} aria-hidden="true" />;
  const ext = node.name.includes(".") ? node.name.slice(node.name.lastIndexOf(".") + 1).toLowerCase() : "";
  if (["png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "bmp", "avif"].includes(ext)) return <FileImage size={14} aria-hidden="true" />;
  if (["md", "txt", "rst", "adoc"].includes(ext)) return <FileText size={14} aria-hidden="true" />;
  if (ext) return <FileCode size={14} aria-hidden="true" />;
  return <File size={14} aria-hidden="true" />;
}

/** The rows to show: expanded folders inline; with a filter, only loaded
 * names that match plus the folders on their way. */
export function visibleRows(
  entries: FileTreeProps["entries"],
  expanded: ReadonlySet<string>,
  errors: Record<string, string> = {},
  filter = "",
): Row[] {
  const rows: Row[] = [];
  const q = filter.trim().toLowerCase();
  // With a filter: which loaded paths match, and their ancestors.
  let keep: Set<string> | null = null;
  if (q) {
    keep = new Set();
    for (const list of Object.values(entries)) {
      for (const node of list ?? []) {
        if (node.name.toLowerCase().includes(q)) {
          let p = node.path;
          while (p && !keep.has(p)) {
            keep.add(p);
            p = parentOf(p);
          }
        }
      }
    }
  }
  const walk = (dir: string, depth: number) => {
    const list = entries[dir];
    if (errors[dir] !== undefined) {
      rows.push({ type: "error", dir, depth, message: errors[dir] ?? "" });
      return;
    }
    if (list === undefined) {
      if (!keep) rows.push({ type: "loading", dir, depth });
      return;
    }
    const shown = keep ? list.filter((n) => keep?.has(n.path)) : list;
    if (shown.length === 0 && !keep && dir !== "") rows.push({ type: "empty", dir, depth });
    shown.forEach((node, i) => {
      rows.push({ type: "node", node, depth, setsize: shown.length, posinset: i + 1 });
      const open = node.kind === "dir" && (keep ? keep.has(node.path) && entries[node.path] !== undefined : expanded.has(node.path));
      if (open) walk(node.path, depth + 1);
    });
  };
  walk("", 0);
  return rows;
}

/** UI_SPEC §2.26 FileTree: a lazily loaded, keyboard-navigable tree with git
 * letters, a filter box, one context menu and drag-out hooks. */
export function FileTree({
  labels: partial,
  entries,
  errors,
  expanded,
  selected,
  onToggle,
  onOpen,
  onSelect,
  onRetry,
  menu,
  onRowPointerDown,
  toolbar,
  className,
}: FileTreeProps) {
  const labels = { ...FILE_TREE_LABELS, ...partial };
  const baseId = useId();
  const [filter, setFilter] = useState("");
  const [active, setActive] = useState<string | null>(selected ?? null);
  const [menuNode, setMenuNode] = useState<FileTreeNode | null>(null);
  const [scroll, setScroll] = useState({ top: 0, height: 600 });
  const listRef = useRef<HTMLDivElement>(null);
  const typeahead = useRef({ text: "", at: 0 });

  const rows = useMemo(() => visibleRows(entries, expanded, errors, filter), [entries, expanded, errors, filter]);
  const nodeRows = useMemo(() => rows.filter((r): r is Extract<Row, { type: "node" }> => r.type === "node"), [rows]);
  const indexOf = useCallback((path: string | null) => (path === null ? -1 : nodeRows.findIndex((r) => r.node.path === path)), [nodeRows]);

  useEffect(() => {
    if (selected !== undefined && selected !== null) setActive(selected);
  }, [selected]);

  // The active row must stay a row (a refresh can remove it).
  const activePath = active !== null && indexOf(active) >= 0 ? active : (nodeRows[0]?.node.path ?? null);
  const rowId = (path: string) => `${baseId}-row-${encodeURIComponent(path)}`;

  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const measure = () => setScroll((s) => ({ ...s, height: el.clientHeight || s.height }));
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const virtual = rows.length > VIRTUALIZE_ABOVE;
  const first = virtual ? Math.max(0, Math.floor(scroll.top / FILE_ROW_HEIGHT) - OVERSCAN) : 0;
  const last = virtual ? Math.min(rows.length, Math.ceil((scroll.top + scroll.height) / FILE_ROW_HEIGHT) + OVERSCAN) : rows.length;

  const reveal = (path: string) => {
    const el = listRef.current;
    const i = rows.findIndex((r) => r.type === "node" && r.node.path === path);
    if (!el || i < 0) return;
    const top = i * FILE_ROW_HEIGHT;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (top + FILE_ROW_HEIGHT > el.scrollTop + el.clientHeight) el.scrollTop = top + FILE_ROW_HEIGHT - el.clientHeight;
  };

  const focusRow = (path: string) => {
    setActive(path);
    onSelect?.(path);
    reveal(path);
  };

  const activate = (node: FileTreeNode) => {
    focusRow(node.path);
    if (node.kind === "dir") onToggle(node.path, !expanded.has(node.path));
    else onOpen(node);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.nativeEvent.isComposing) return;
    const i = indexOf(activePath);
    const cur = nodeRows[i];
    const go = (j: number) => {
      const r = nodeRows[Math.max(0, Math.min(nodeRows.length - 1, j))];
      if (r) focusRow(r.node.path);
    };
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        go(i + 1);
        return;
      case "ArrowUp":
        e.preventDefault();
        go(i - 1);
        return;
      case "Home":
        e.preventDefault();
        go(0);
        return;
      case "End":
        e.preventDefault();
        go(nodeRows.length - 1);
        return;
      case "ArrowRight":
        if (!cur || cur.node.kind !== "dir") return;
        e.preventDefault();
        if (!expanded.has(cur.node.path)) onToggle(cur.node.path, true);
        else if (nodeRows[i + 1] && parentOf(nodeRows[i + 1]?.node.path ?? "") === cur.node.path) go(i + 1);
        return;
      case "ArrowLeft": {
        if (!cur) return;
        e.preventDefault();
        if (cur.node.kind === "dir" && expanded.has(cur.node.path)) onToggle(cur.node.path, false);
        else {
          const parent = parentOf(cur.node.path);
          if (parent) focusRow(parent);
        }
        return;
      }
      case "Enter":
      case " ":
        if (!cur) return;
        e.preventDefault();
        activate(cur.node);
        return;
      default:
        // Type-ahead: jump to the next row whose name starts with what was typed.
        if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
          const now = Date.now();
          const t = typeahead.current;
          t.text = now - t.at > 600 ? e.key.toLowerCase() : t.text + e.key.toLowerCase();
          t.at = now;
          const from = t.text.length === 1 ? i + 1 : i;
          for (let k = 0; k < nodeRows.length; k++) {
            const r = nodeRows[(from + k) % nodeRows.length];
            if (r?.node.name.toLowerCase().startsWith(t.text)) {
              focusRow(r.node.path);
              break;
            }
          }
        }
    }
  };

  const status = (n: FileTreeNode) => (n.gitStatus ? GIT_LETTER[n.gitStatus] : null);
  const describe = (n: FileTreeNode) =>
    [n.name, n.gitStatus ? labels.status[n.gitStatus] : null, n.ignored ? labels.ignored : null, n.symlink ? labels.symlink : null]
      .filter(Boolean)
      .join(", ");

  const menuEntries = menuNode && menu ? menu(menuNode) : [];
  const rootError = errors?.[""];

  // A tree needs items: with none, say why instead (loading, empty, error,
  // or nothing matches the filter).
  const placeholder =
    nodeRows.length === 0 ? (
      <div
        role="status"
        data-testid="file-tree"
        className={cx("flex min-h-0 flex-1 items-start gap-2 px-3 py-2 text-sm", rootError !== undefined ? "text-error" : "text-fg-subtle")}
      >
        <span className="min-w-0">
          {rootError !== undefined
            ? `${labels.error}: ${rootError}`
            : filter
              ? labels.noMatches(filter)
              : entries[""] === undefined
                ? labels.loading
                : labels.empty}
        </span>
        {rootError !== undefined && onRetry && (
          <button
            type="button"
            onClick={() => onRetry("")}
            className="inline-flex items-center gap-1 rounded-xs px-1 text-fg-muted hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
          >
            <RotateCcw size={12} aria-hidden="true" />
            {labels.retry}
          </button>
        )}
      </div>
    ) : null;

  const tree = (
    <div
      ref={listRef}
      role="tree"
      aria-label={labels.tree}
      tabIndex={0}
      aria-activedescendant={activePath !== null && nodeRows.length > 0 ? rowId(activePath) : undefined}
      data-testid="file-tree"
      onKeyDown={onKeyDown}
      onScroll={(e) => virtual && setScroll({ top: e.currentTarget.scrollTop, height: e.currentTarget.clientHeight })}
      onContextMenuCapture={(e) => {
        const row = (e.target as HTMLElement).closest<HTMLElement>("[data-path]");
        const path = row?.dataset.path ?? activePath;
        const r = nodeRows.find((x) => x.node.path === path);
        setMenuNode(r?.node ?? null);
        if (r) focusRow(r.node.path);
      }}
      className="relative min-h-0 flex-1 overflow-auto py-1 outline-none focus-visible:shadow-focus"
    >
      <div style={virtual ? { height: rows.length * FILE_ROW_HEIGHT, position: "relative" } : undefined}>
        <div style={virtual ? { transform: `translateY(${first * FILE_ROW_HEIGHT}px)` } : undefined}>
          {rows.slice(first, last).map((row, k) => {
            const pad = { paddingLeft: 8 + row.depth * INDENT };
            if (row.type !== "node") {
              return (
                <div
                  key={`${row.type}:${row.dir}:${first + k}`}
                  role="none"
                  style={pad}
                  className={cx("flex h-6 items-center gap-2 pr-2 text-sm", row.type === "error" ? "text-error" : "text-fg-subtle")}
                >
                  <span className="truncate">{row.type === "loading" ? labels.loading : row.type === "empty" ? labels.empty : `${labels.error}: ${row.message}`}</span>
                  {row.type === "error" && onRetry && (
                    <button
                      type="button"
                      tabIndex={-1}
                      onClick={() => onRetry(row.dir)}
                      className="inline-flex items-center gap-1 rounded-xs px-1 text-fg-muted hover:text-fg"
                    >
                      <RotateCcw size={12} aria-hidden="true" />
                      {labels.retry}
                    </button>
                  )}
                </div>
              );
            }
            const { node } = row;
            const open = node.kind === "dir" && (filter ? true : expanded.has(node.path));
            const s = status(node);
            const isActive = node.path === activePath;
            return (
              <div
                key={node.path}
                id={rowId(node.path)}
                role="treeitem"
                aria-level={row.depth + 1}
                aria-setsize={row.setsize}
                aria-posinset={row.posinset}
                aria-expanded={node.kind === "dir" ? open : undefined}
                aria-selected={node.path === (selected ?? null)}
                aria-label={describe(node)}
                data-path={node.path}
                data-kind={node.kind}
                data-testid="file-row"
                style={pad}
                onClick={() => activate(node)}
                onPointerDown={(e) => onRowPointerDown?.(node, e)}
                className={cx(
                  "relative flex h-6 cursor-default select-none items-center gap-1.5 pr-2 text-sm",
                  node.path === selected ? "bg-accent-soft text-fg" : "hover:bg-surface-3",
                  isActive && "outline outline-1 -outline-offset-1 outline-border-strong",
                  node.ignored ? "text-fg-subtle" : "text-fg",
                )}
              >
                {Array.from({ length: row.depth }, (_, d) => (
                  <span key={d} aria-hidden="true" className="absolute inset-y-0 w-px bg-divider" style={{ left: 8 + d * INDENT + 5 }} />
                ))}
                <span aria-hidden="true" className="inline-flex w-3 shrink-0 justify-center text-fg-subtle">
                  {node.kind === "dir" ? open ? <ChevronDown size={12} /> : <ChevronRight size={12} /> : null}
                </span>
                <span aria-hidden="true" className={cx("inline-flex shrink-0", node.kind === "dir" ? "text-fg-muted" : "text-fg-subtle")}>
                  {fileIcon(node, open)}
                </span>
                <span className={cx("min-w-0 flex-1 truncate", node.ignored && "italic")}>{node.name}</span>
                {node.symlink && <Link2 size={12} aria-hidden="true" className="shrink-0 text-fg-subtle" />}
                {s && (
                  <span aria-hidden="true" className={cx("w-3 shrink-0 text-center font-mono text-2xs font-semibold", s.tone)}>
                    {s.letter}
                  </span>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );

  return (
    <div className={cx("flex min-h-0 flex-col", className)}>
      <div className="flex shrink-0 items-center gap-1 px-2 py-1.5">
        <label className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-sm border border-border bg-surface-2 px-2 focus-within:shadow-focus">
          <Search size={12} aria-hidden="true" className="shrink-0 text-fg-subtle" />
          <input
            type="search"
            aria-label={labels.filter}
            placeholder={labels.filter}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape" && filter) {
                e.preventDefault();
                e.stopPropagation();
                setFilter("");
              } else if (e.key === "ArrowDown") {
                e.preventDefault();
                listRef.current?.focus();
              }
            }}
            className="min-w-0 flex-1 bg-transparent text-sm text-fg outline-none placeholder:text-fg-subtle"
          />
        </label>
        {toolbar}
      </div>
      {placeholder ?? (menu ? <ContextMenu entries={menuEntries}>{tree}</ContextMenu> : tree)}
    </div>
  );
}
