import { Fragment, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight, MessageSquarePlus } from "lucide-react";
import { cx } from "../cx";

/** One file of a diff (the shape of `FileDiff` in @tandem/protocol). */
export type DiffFile = {
  path: string;
  oldPath?: string;
  change: "added" | "modified" | "deleted" | "renamed" | "type_changed" | "untracked" | "conflicted";
  additions: number;
  deletions: number;
  binary: boolean;
  /** Unified diff text with its `diff --git` header; empty when binary. */
  patch: string;
  truncated: boolean;
};

export type DiffLine =
  | { kind: "hunk"; text: string }
  | { kind: "context" | "add" | "del"; text: string; oldNo: number | null; newNo: number | null }
  | { kind: "note"; text: string };

/** Files longer than this start collapsed (UI_SPEC §2.25). */
export const LARGE_DIFF_LINES = 2000;

const LETTER: Record<DiffFile["change"], string> = {
  added: "A",
  modified: "M",
  deleted: "D",
  renamed: "R",
  type_changed: "T",
  untracked: "U",
  conflicted: "C",
};

const LETTER_TONE: Record<DiffFile["change"], string> = {
  added: "text-success",
  modified: "text-warning",
  deleted: "text-error",
  renamed: "text-info",
  type_changed: "text-info",
  untracked: "text-info",
  conflicted: "text-error",
};

/** Hunks and lines of a unified patch, with old and new line numbers. The
 * `diff --git` header lines are dropped. */
export function parsePatch(patch: string): DiffLine[] {
  const out: DiffLine[] = [];
  let oldNo = 0;
  let newNo = 0;
  let inHunk = false;
  for (const line of patch.split("\n")) {
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/.exec(line);
    if (hunk) {
      oldNo = Number(hunk[1]);
      newNo = Number(hunk[2]);
      inHunk = true;
      out.push({ kind: "hunk", text: line });
      continue;
    }
    if (!inHunk) continue;
    if (line.startsWith("+")) out.push({ kind: "add", text: line.slice(1), oldNo: null, newNo: newNo++ });
    else if (line.startsWith("-"))
      out.push({ kind: "del", text: line.slice(1), oldNo: oldNo++, newNo: null });
    else if (line.startsWith(" "))
      out.push({ kind: "context", text: line.slice(1), oldNo: oldNo++, newNo: newNo++ });
    else if (line.startsWith("\\")) out.push({ kind: "note", text: line.slice(1).trim() });
    else if (line.startsWith("diff --git")) inHunk = false;
  }
  return out;
}

export type DiffViewLabels = {
  binary: string;
  truncated: string;
  empty: string;
  /** "Show diff ({n} lines)". */
  showLarge: (n: number) => string;
  collapse: (path: string) => string;
  expand: (path: string) => string;
};

const DEFAULT_LABELS: DiffViewLabels = {
  binary: "Binary file changed",
  truncated: "This diff is too long to show in full.",
  empty: "No changes",
  showLarge: (n) => `Show diff (${n.toLocaleString("en")} lines)`,
  collapse: (path) => `Collapse ${path}`,
  expand: (path) => `Expand ${path}`,
};

/** A line a comment can attach to: the new side for added and context
 * lines, the old side for removed ones. */
export type DiffLineTarget = { path: string; side: "new" | "old"; line: number; text: string };

/** The key `annotations` uses for a line. */
export const diffLineKey = (t: Pick<DiffLineTarget, "path" | "side" | "line">): string =>
  `${t.path}:${t.side}:${t.line}`;

/** Optional line comments: a gutter button per line and content shown under it. */
export type DiffComments = {
  /** The button's accessible name, such as "Comment on line 42". */
  label: (target: DiffLineTarget) => string;
  onComment: (target: DiffLineTarget) => void;
  /** Shown under the line whose `diffLineKey` matches. */
  annotations?: Record<string, ReactNode>;
};

function lineTarget(
  path: string,
  l: Extract<DiffLine, { kind: "context" | "add" | "del" }>,
): DiffLineTarget | null {
  if (l.kind === "del") return l.oldNo === null ? null : { path, side: "old", line: l.oldNo, text: l.text };
  return l.newNo === null ? null : { path, side: "new", line: l.newNo, text: l.text };
}

function FileBlock({
  file,
  labels,
  comments,
}: {
  file: DiffFile;
  labels: DiffViewLabels;
  comments?: DiffComments;
}) {
  const lines = parsePatch(file.patch);
  const large = lines.length > LARGE_DIFF_LINES;
  const [open, setOpen] = useState(!large);
  const name = file.oldPath && file.oldPath !== file.path ? `${file.oldPath} → ${file.path}` : file.path;
  return (
    <section
      data-testid="diff-file"
      data-path={file.path}
      className="overflow-hidden rounded-md border border-border"
    >
      <header className="flex h-8 items-center gap-2 border-b border-border bg-surface-2 px-2 text-sm">
        <button
          type="button"
          aria-expanded={open}
          aria-label={open ? labels.collapse(file.path) : labels.expand(file.path)}
          onClick={() => setOpen(!open)}
          className="inline-flex size-5 items-center justify-center rounded-xs text-fg-muted hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
        >
          {open ? (
            <ChevronDown size={14} aria-hidden="true" />
          ) : (
            <ChevronRight size={14} aria-hidden="true" />
          )}
        </button>
        <span
          aria-hidden="true"
          className={cx("w-3 font-mono text-xs font-semibold", LETTER_TONE[file.change])}
        >
          {LETTER[file.change]}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-fg" title={name}>
          {name}
        </span>
        <span className="shrink-0 font-mono text-xs tabular-nums">
          <span className="text-diff-add-fg">+{file.additions}</span>{" "}
          <span className="text-diff-del-fg">−{file.deletions}</span>
        </span>
      </header>
      {open &&
        (file.binary ? (
          <p className="px-3 py-2 text-sm text-fg-muted">{labels.binary}</p>
        ) : (
          <div role="table" aria-label={name} className="overflow-x-auto font-mono text-xs leading-5">
            {lines.map((l, i) =>
              l.kind === "hunk" ? (
                <div key={i} role="row" className="bg-diff-hunk-bg px-3 text-fg-muted">
                  <span role="cell" className="whitespace-pre">
                    {l.text}
                  </span>
                </div>
              ) : l.kind === "note" ? (
                <div key={i} role="row" className="px-3 italic text-fg-subtle">
                  <span role="cell">{l.text}</span>
                </div>
              ) : (
                <Fragment key={i}>
                  <div
                    role="row"
                    data-kind={l.kind}
                    className={cx(
                      "flex",
                      comments && "group/line",
                      l.kind === "add" && "bg-diff-add-bg",
                      l.kind === "del" && "bg-diff-del-bg",
                    )}
                  >
                    {comments && (
                      <span role="cell" className="w-5 shrink-0 select-none">
                        {(() => {
                          const target = lineTarget(file.path, l);
                          return target ? (
                            <button
                              type="button"
                              data-testid="diff-comment-line"
                              aria-label={comments.label(target)}
                              onClick={() => comments.onComment(target)}
                              className="inline-flex size-5 items-center justify-center rounded-xs text-fg-subtle opacity-0 hover:text-accent focus-visible:opacity-100 focus-visible:shadow-focus focus-visible:outline-none group-hover/line:opacity-100"
                            >
                              <MessageSquarePlus size={12} aria-hidden="true" />
                            </button>
                          ) : null;
                        })()}
                      </span>
                    )}
                    <span
                      role="cell"
                      className="w-10 shrink-0 select-none pr-2 text-right text-fg-subtle tabular-nums"
                    >
                      {l.oldNo ?? ""}
                    </span>
                    <span
                      role="cell"
                      className="w-10 shrink-0 select-none pr-2 text-right text-fg-subtle tabular-nums"
                    >
                      {l.newNo ?? ""}
                    </span>
                    <span
                      role="cell"
                      aria-hidden="true"
                      className={cx(
                        "w-4 shrink-0 select-none",
                        l.kind === "add" && "text-diff-add-fg",
                        l.kind === "del" && "text-diff-del-fg",
                      )}
                    >
                      {l.kind === "add" ? "+" : l.kind === "del" ? "−" : " "}
                    </span>
                    <span role="cell" className="whitespace-pre pr-3 text-fg">
                      {l.text}
                    </span>
                  </div>
                  {(() => {
                    const target = comments?.annotations ? lineTarget(file.path, l) : null;
                    const note = target ? comments?.annotations?.[diffLineKey(target)] : undefined;
                    return note ? (
                      <div role="row" className="border-y border-border bg-surface-2 px-3 py-2 font-sans">
                        <span role="cell">{note}</span>
                      </div>
                    ) : null;
                  })()}
                </Fragment>
              ),
            )}
          </div>
        ))}
      {open && !file.binary && file.truncated && (
        <p className="px-3 py-2 text-sm text-fg-muted">{labels.truncated}</p>
      )}
      {!open && large && (
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="w-full px-3 py-2 text-left text-sm text-accent hover:bg-surface-3 focus-visible:shadow-focus focus-visible:outline-none"
        >
          {labels.showLarge(lines.length)}
        </button>
      )}
    </section>
  );
}

/** UI_SPEC §2.25 DiffView: unified, with a line-number gutter, a header per
 * file (path, +/− counts, collapse caret), large files collapsed and a note
 * for binary files. Split view and syntax highlighting arrive with Files. */
export function DiffView({
  files,
  labels,
  className,
  comments,
}: {
  files: DiffFile[];
  labels?: Partial<DiffViewLabels>;
  className?: string;
  comments?: DiffComments;
}) {
  const l = { ...DEFAULT_LABELS, ...labels };
  if (files.length === 0) return <p className={cx("text-sm text-fg-muted", className)}>{l.empty}</p>;
  return (
    <div data-testid="diff-view" className={cx("flex flex-col gap-2", className)}>
      {files.map((f) => (
        <FileBlock key={`${f.oldPath ?? ""}:${f.path}`} file={f} labels={l} comments={comments} />
      ))}
    </div>
  );
}
