import { useCallback, useEffect, useMemo, useState } from "react";
import { ClipboardCopy, FileText as FileIcon, RefreshCw, X } from "lucide-react";
import type { FileDiff, FileText, IpcError, ThreadChanges } from "@tandem/protocol";
import { Banner, Button, CodeBlock, DiffView, EmptyState, IconButton, Skeleton, Tabs, cx } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import type { Row, ToolEvent } from "./rows";

const LETTER: Record<FileDiff["change"], string> = {
  added: "A",
  modified: "M",
  deleted: "D",
  renamed: "R",
  type_changed: "T",
  untracked: "U",
  conflicted: "C",
};

const FILE_VERBS: ReadonlyArray<ToolEvent["verb"]> = ["read", "edit", "write", "multiedit"];

/** Files the thread's tool calls read or wrote, most recent first (F-TH-13 Files). */
export function referencedFiles(rows: Row[]): Array<{ path: string; wrote: boolean }> {
  const seen = new Map<string, boolean>();
  for (const row of rows) {
    const tools = row.kind === "tool" ? [row.tool] : row.kind === "tools" ? row.tools : [];
    for (const tool of tools) {
      if (!FILE_VERBS.includes(tool.verb) || !tool.argSummary || /\s/.test(tool.argSummary)) continue;
      // Edited once stays edited; the latest mention moves it to the top.
      const wrote = tool.verb !== "read" || (seen.get(tool.argSummary) ?? false);
      seen.delete(tool.argSummary);
      seen.set(tool.argSummary, wrote);
    }
  }
  return [...seen.entries()].reverse().map(([path, wrote]) => ({ path, wrote }));
}

/** `src/very/long/path/file.ts` keeps both ends. */
function middle(path: string, max = 44): string {
  if (path.length <= max) return path;
  const keep = Math.floor((max - 1) / 2);
  return `${path.slice(0, keep)}…${path.slice(path.length - keep)}`;
}

function ChangesTab({ threadId, refreshKey }: { threadId: string; refreshKey: string }) {
  const [state, setState] = useState<
    { at: "loading" } | { at: "error"; message: string } | { at: "ready"; changes: ThreadChanges }
  >({ at: "loading" });
  const [selected, setSelected] = useState<string | null>(null);
  const load = useCallback(() => {
    invoke<ThreadChanges>("thread_changes", { threadId })
      .then((changes) => setState({ at: "ready", changes }))
      .catch((e: IpcError) => setState({ at: "error", message: e.message }));
  }, [threadId]);
  // Every turn end (and opening) reads the folder again.
  useEffect(() => load(), [load, refreshKey]);

  if (state.at === "loading") {
    return (
      <div className="flex flex-col gap-2 p-3" aria-busy="true">
        {[0, 1, 2].map((i) => (
          <Skeleton key={i} className="h-6 w-full" />
        ))}
      </div>
    );
  }
  if (state.at === "error") {
    return (
      <div className="p-3">
        <Banner tone="error" action={{ label: t("thread.changes.retry"), onClick: load }}>
          {t("thread.changes.failed", { reason: state.message })}
        </Banner>
      </div>
    );
  }
  const { files, source } = state.changes;
  const copyList = () => void navigator.clipboard?.writeText(files.map((f) => f.path).join("\n"));
  const current = files.find((f) => f.path === selected) ?? null;
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="changes-list">
      <div className="flex h-9 shrink-0 items-center gap-1 px-3">
        <span className="text-xs font-semibold uppercase tracking-wide text-fg-muted">
          {t("thread.changes.title", { n: files.length })}
        </span>
        <span className="ml-auto flex gap-0.5">
          <IconButton
            size="sm"
            icon={<RefreshCw size={14} />}
            label={t("thread.changes.refresh")}
            onClick={load}
          />
          <IconButton
            size="sm"
            icon={<ClipboardCopy size={14} />}
            label={t("thread.changes.copyList")}
            onClick={copyList}
            disabled={files.length === 0}
          />
        </span>
      </div>
      {source === "snapshot" && (
        <p className="px-3 pb-2 text-sm text-fg-muted">{t("thread.changes.snapshot")}</p>
      )}
      {files.length === 0 ? (
        <div className="p-3">
          <EmptyState title={t("thread.changes.empty")} />
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto">
          <ul className="flex flex-col px-1.5" aria-label={t("thread.changes.files")}>
            {files.map((f) => (
              <li key={f.path}>
                <button
                  type="button"
                  data-testid="change-row"
                  data-path={f.path}
                  aria-pressed={f.path === selected}
                  onClick={() => setSelected(f.path === selected ? null : f.path)}
                  className={cx(
                    "flex w-full items-center gap-2 rounded-sm px-1.5 py-1 text-left text-sm hover:bg-surface-3 focus-visible:shadow-focus focus-visible:outline-none",
                    f.path === selected && "bg-surface-3",
                  )}
                >
                  <span
                    className="w-3 shrink-0 font-mono text-xs text-fg-muted"
                    aria-label={t(`thread.changes.change.${f.change}`)}
                  >
                    {LETTER[f.change]}
                  </span>
                  <span className="min-w-0 flex-1 truncate font-mono text-fg" title={f.path}>
                    {middle(f.path)}
                  </span>
                  {(f.additions > 0 || f.deletions > 0) && (
                    <span className="shrink-0 font-mono text-xs">
                      <span className="text-success">{t("thread.changes.added", { n: f.additions })}</span>{" "}
                      <span className="text-danger">{t("thread.changes.removed", { n: f.deletions })}</span>
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
          {current && (
            <div className="border-t border-divider p-2" data-testid="change-diff">
              {current.patch || current.binary ? (
                <DiffView files={[current]} />
              ) : (
                <p className="text-sm text-fg-muted">{t("thread.changes.noDiff")}</p>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function FilesTab({ threadId, files }: { threadId: string; files: Array<{ path: string; wrote: boolean }> }) {
  const [open, setOpen] = useState<{ path: string; text: FileText | null; error: string | null } | null>(
    null,
  );
  const view = (path: string) => {
    if (open?.path === path) return setOpen(null);
    setOpen({ path, text: null, error: null });
    invoke<FileText>("thread_file_read", { threadId, path })
      .then((text) => setOpen((o) => (o?.path === path ? { path, text, error: null } : o)))
      .catch((e: IpcError) =>
        setOpen((o) => (o?.path === path ? { path, text: null, error: e.message } : o)),
      );
  };
  if (files.length === 0) {
    return (
      <div className="p-3">
        <EmptyState title={t("thread.files.empty")} />
      </div>
    );
  }
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-1.5 py-1">
      <ul className="flex flex-col" aria-label={t("thread.files.title")}>
        {files.map((f) => (
          <li key={f.path} className="flex flex-col">
            <button
              type="button"
              data-testid="file-row"
              aria-expanded={open?.path === f.path}
              onClick={() => view(f.path)}
              className="flex w-full items-center gap-2 rounded-sm px-1.5 py-1 text-left text-sm hover:bg-surface-3 focus-visible:shadow-focus focus-visible:outline-none"
            >
              <FileIcon size={14} className="shrink-0 text-fg-muted" aria-hidden="true" />
              <span className="min-w-0 flex-1 truncate font-mono text-fg" title={f.path}>
                {middle(f.path)}
              </span>
              <span className="shrink-0 text-xs text-fg-muted">
                {f.wrote ? t("thread.files.edited") : t("thread.files.read")}
              </span>
            </button>
            {open?.path === f.path && (
              <div className="px-1.5 pb-2">
                {open.error ? (
                  <p className="text-sm text-danger">{open.error}</p>
                ) : !open.text ? (
                  <Skeleton className="h-16 w-full" />
                ) : open.text.binary ? (
                  <p className="text-sm text-fg-muted">{t("thread.files.binary")}</p>
                ) : (
                  <>
                    <CodeBlock code={open.text.content} language={f.path.split(".").pop() ?? ""} />
                    {open.text.truncated && (
                      <p className="pt-1 text-sm text-fg-muted">{t("thread.files.truncated")}</p>
                    )}
                  </>
                )}
              </div>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

/** UI-24: the thread's Changes and Files panel (F-TH-13). Floats over the
 * transcript when the window is narrow. */
export function ChangesPanel({
  threadId,
  refreshKey,
  rows,
  floating,
  onClose,
}: {
  threadId: string;
  /** Changes at every turn end. */
  refreshKey: string;
  rows: Row[];
  floating: boolean;
  onClose: () => void;
}) {
  const [tab, setTab] = useState("changes");
  const files = useMemo(() => referencedFiles(rows), [rows]);
  return (
    <aside
      data-testid="changes-panel"
      aria-label={t("thread.changes.panel")}
      className={cx(
        "flex w-[360px] shrink-0 flex-col border-l border-border bg-surface-2",
        floating ? "absolute inset-y-0 right-0 z-20 shadow-3" : "relative",
      )}
    >
      <div className="absolute right-1.5 top-1 z-10">
        <IconButton size="sm" icon={<X size={14} />} label={t("thread.changes.close")} onClick={onClose} />
      </div>
      <Tabs
        className="min-h-0 flex-1 [&>[role=tablist]]:px-3 [&>[role=tablist]]:pr-10"
        label={t("thread.changes.panel")}
        value={tab}
        onValueChange={setTab}
        items={[
          { id: "changes", label: t("thread.changes.tab") },
          { id: "files", label: t("thread.files.tab"), count: files.length || undefined },
        ]}
        renderPanel={(id) =>
          id === "changes" ? (
            <div className="flex h-full min-h-0 flex-col">
              <ChangesTab threadId={threadId} refreshKey={refreshKey} />
            </div>
          ) : (
            <div className="flex h-full min-h-0 flex-col">
              <FilesTab threadId={threadId} files={files} />
            </div>
          )
        }
      />
    </aside>
  );
}

/** The header toggle. */
export function ChangesToggle({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  return (
    <Button
      size="sm"
      variant={open ? "secondary" : "ghost"}
      onClick={onToggle}
      aria-pressed={open}
      data-testid="changes-toggle"
    >
      {t("thread.changes.tab")}
    </Button>
  );
}
