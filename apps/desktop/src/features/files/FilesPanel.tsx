// The sidebar's Files tab (F-CD-19, UI_SPEC §2.26, §3.5): the tree of the
// active context (a Code workspace or a thread's folder) and the editor. Wide
// panels show both with a divider; narrow ones show one column with a back
// button. The tree follows the disk through `fs_watch` while the tab exists.
import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { ChevronsDownUp, Eye, EyeOff, FilePlus, FolderPlus, FolderTree, RefreshCw } from "lucide-react";
import type { FilesView, FsEntry, FsKind, PaneRecord } from "@tandem/protocol";
import {
  Button,
  ConfirmDialog,
  Dialog,
  EmptyState,
  FileTree,
  IconButton,
  Input,
  isMac,
  SegmentedControl,
  useToast,
  type FileTreeNode,
  type MenuEntry,
} from "@tandem/ui";
import { t } from "../../i18n";
import { copyText } from "../../ipc/clipboard";
import { invoke } from "../../ipc/invoke";
import { absolutePath, useFiles } from "../../stores/files";
import { useGit, useGitContext } from "../../stores/git";
import { useLayout } from "../../stores/layout";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { dropOnThread } from "../thread/drops";
import { BranchChip, ChangeDiff, ChangesList, changeCount } from "./ChangesView";
import { useActiveFiles } from "./context";
import { FileEditor } from "./FileEditor";
import { startTreeDrag } from "./treeDrag";

/** Panels at least this wide show the tree and the editor side by side. */
export const WIDE_AT = 600;
const TREE_DEFAULT = 240;
const TREE_MIN = 160;

type NameAsk = { kind: "file" | "dir" | "rename"; dir: string; node?: FileTreeNode } | null;

function parentOf(path: string): string {
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at);
}

function NameDialog({ ask, onClose }: { ask: NameAsk; onClose: () => void }) {
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    setName(ask?.kind === "rename" ? (ask.node?.name ?? "") : "");
    setError(null);
  }, [ask]);
  if (!ask) return null;
  const title =
    ask.kind === "file" ? t("files.dialog.newFile") : ask.kind === "dir" ? t("files.dialog.newFolder") : t("files.dialog.rename", { name: ask.node?.name ?? "" });
  const submit = async () => {
    const clean = name.trim();
    if (!clean || clean.includes("/") || clean.includes("\\") || clean === "." || clean === "..") {
      setError(t("files.dialog.invalid"));
      return;
    }
    setBusy(true);
    const store = useFiles.getState();
    const target = ask.dir ? `${ask.dir}/${clean}` : clean;
    const err =
      ask.kind === "rename" && ask.node
        ? await store.rename(ask.node.path, `${parentOf(ask.node.path) ? `${parentOf(ask.node.path)}/` : ""}${clean}`)
        : await store.create(target, (ask.kind === "dir" ? "dir" : "file") as FsKind);
    setBusy(false);
    if (err) setError(err.message);
    else onClose();
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={title}
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("files.dialog.cancel")}
          </Button>
          <Button variant="primary" loading={busy} onClick={() => void submit()}>
            {ask.kind === "rename" ? t("files.dialog.renameAction") : t("files.dialog.create")}
          </Button>
        </>
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <Input label={t("files.dialog.name")} value={name} autoFocus onChange={(e) => setName(e.target.value)} error={error ?? undefined} data-testid="files-name-input" />
      </form>
    </Dialog>
  );
}

/** Follows the panel's width (narrow or wide layout). */
function useWidth(): [(el: HTMLDivElement | null) => void, number] {
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    if (!el) return;
    const measure = () => setWidth(el.clientWidth);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    window.addEventListener("resize", measure);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [el]);
  return [setEl, width];
}

export function FilesPanel() {
  const active = useActiveFiles();
  const context = active.kind === "context" ? active.context : null;
  const ctx = useFiles((s) => (s.key ? s.byKey[s.key] : undefined));
  const { show } = useToast();
  const [ask, setAsk] = useState<NameAsk>(null);
  const [deleting, setDeleting] = useState<FileTreeNode | null>(null);
  const [showEditor, setShowEditor] = useState(false);
  const [showDiff, setShowDiff] = useState(false);
  const [liveDivider, setLiveDivider] = useState<number | null>(null);
  const [ref, width] = useWidth();
  const wide = width >= WIDE_AT;
  const mode = useUi((s) => s.mode);

  // Follow the active context while the tab exists; stop watching when it closes.
  const ctxJson = context ? JSON.stringify(context) : null;
  useEffect(() => {
    void useFiles.getState().setContext(ctxJson ? (JSON.parse(ctxJson) as typeof context) : null);
  }, [ctxJson]);
  useEffect(() => () => void useFiles.getState().setContext(null), []);

  // The Changes view and the branch chip: read again on every watch batch.
  const git = useGitContext(context);
  const batches = ctx?.batches ?? 0;
  useEffect(() => {
    if (batches > 0 && ctxJson) void useGit.getState().refresh(JSON.parse(ctxJson) as NonNullable<typeof context>);
  }, [batches, ctxJson]);
  // Picking a change in the narrow layout shows its diff.
  const picked = git?.picked ?? null;
  useEffect(() => setShowDiff(picked !== null), [picked]);

  // Opening a file in the narrow layout shows the editor.
  const activePath = ctx?.active ?? null;
  const lastActive = useRef(activePath);
  useEffect(() => {
    if (activePath && activePath !== lastActive.current) setShowEditor(true);
    lastActive.current = activePath;
  }, [activePath]);

  const expanded = useMemo(() => new Set(ctx?.expanded ?? []), [ctx?.expanded]);
  const entries = useMemo(() => {
    const out: Record<string, FileTreeNode[] | undefined> = {};
    for (const [dir, list] of Object.entries(ctx?.entries ?? {})) {
      out[dir] = list?.map((e: FsEntry) => ({
        path: e.path,
        name: e.name,
        kind: e.kind,
        symlink: e.symlink,
        ignored: e.ignored,
        gitStatus: e.gitStatus ?? null,
      }));
    }
    return out;
  }, [ctx?.entries]);

  if (active.kind !== "context") {
    return (
      <div ref={ref} className="flex h-full flex-col" data-testid="files-panel" data-state={active.kind}>
        {active.kind === "none" ? (
          <EmptyState icon={<FolderTree size={18} />} title={t("files.none.title")} body={t("files.none.body")} />
        ) : (
          <EmptyState icon={<FolderTree size={18} />} title={t("files.noFolder.title")} body={t("files.noFolder.body")} />
        )}
      </div>
    );
  }

  const store = useFiles.getState;
  const root = context ? absolutePath(context, "") : null;
  const abs = (path: string) => (context ? absolutePath(context, path) : null);

  const openTerminal = async (dir: string) => {
    const path = abs(dir);
    const wsId = active.workspaceId;
    if (!path || !wsId) return;
    const ws = useWorkspaces.getState();
    useUi.getState().setMode("code");
    ws.select(wsId);
    const tabId = ws.activeTab[wsId] ?? ws.list.find((w) => w.id === wsId)?.tabs[0]?.id;
    if (!tabId) return;
    try {
      const pane = await invoke<PaneRecord>("pane_create", { tabId, kind: "terminal", cwd: path });
      useLayout.getState().insert(tabId, pane, null, "right");
    } catch (e) {
      show({ tone: "error", title: (e as { message?: string }).message ?? String(e) });
    }
  };

  const menu = (node: FileTreeNode): MenuEntry[] => {
    const dir = node.kind === "dir" ? node.path : parentOf(node.path);
    const entriesList: MenuEntry[] = [
      { id: "new-file", label: t("files.newFile"), onSelect: () => setAsk({ kind: "file", dir }) },
      { id: "new-folder", label: t("files.newFolder"), onSelect: () => setAsk({ kind: "dir", dir }) },
      { id: "rename", label: t("files.menu.rename"), onSelect: () => setAsk({ kind: "rename", dir, node }) },
      { id: "sep-1", type: "separator" },
      {
        id: "copy-path",
        label: t("files.menu.copyPath"),
        onSelect: () => {
          const p = abs(node.path);
          if (p) void copyText(p);
        },
      },
      { id: "copy-relative", label: t("files.menu.copyRelative"), onSelect: () => void copyText(node.path) },
      {
        id: "reveal",
        label: isMac() ? t("files.menu.revealMac") : t("files.menu.reveal"),
        onSelect: () => {
          const p = abs(node.path);
          if (p) void invoke("app_reveal_path", { path: p }).catch((e: { message?: string }) => show({ tone: "error", title: e.message ?? "" }));
        },
      },
      { id: "terminal", label: t("files.menu.openTerminal"), onSelect: () => void openTerminal(dir) },
    ];
    if (mode === "thread") {
      entriesList.push({
        id: "attach",
        label: t("files.menu.attach"),
        onSelect: () => {
          const p = abs(node.path);
          if (p) void dropOnThread([p], show);
        },
      });
    }
    entriesList.push({ id: "delete", label: t("files.menu.delete"), danger: true, onSelect: () => setDeleting(node) });
    return entriesList;
  };

  const onRowPointerDown = (node: FileTreeNode, e: ReactPointerEvent<HTMLElement>) => {
    const p = abs(node.path);
    if (p) void startTreeDrag(e, p, node.name, show);
  };

  const toolbar = (
    <>
      <IconButton
        size="sm"
        icon={ctx?.showIgnored ? <EyeOff size={13} /> : <Eye size={13} />}
        label={ctx?.showIgnored ? t("files.hideIgnored") : t("files.showIgnored")}
        pressed={!!ctx?.showIgnored}
        data-testid="files-show-ignored"
        onClick={() => store().setShowIgnored(!ctx?.showIgnored)}
      />
      <IconButton size="sm" icon={<FilePlus size={13} />} label={t("files.newFile")} onClick={() => setAsk({ kind: "file", dir: "" })} />
      <IconButton size="sm" icon={<FolderPlus size={13} />} label={t("files.newFolder")} onClick={() => setAsk({ kind: "dir", dir: "" })} />
      <IconButton size="sm" icon={<RefreshCw size={13} />} label={t("files.refresh")} onClick={() => void store().refreshAll()} />
      <IconButton
        size="sm"
        icon={<ChevronsDownUp size={13} />}
        label={t("files.collapseAll")}
        onClick={() => {
          for (const d of ctx?.expanded ?? []) store().toggle(d, false);
        }}
      />
    </>
  );

  const tree = (
    <FileTree
      labels={{
        tree: t("files.tree.label", { name: active.name }),
        filter: t("files.tree.filter"),
        loading: t("files.tree.loading"),
        empty: t("files.tree.empty"),
        noMatches: (q) => t("files.tree.noMatches", { q }),
        error: t("files.tree.error"),
        retry: t("files.retry"),
        ignored: t("files.tree.ignored"),
        symlink: t("files.tree.symlink"),
        status: {
          added: t("files.status.added"),
          modified: t("files.status.modified"),
          deleted: t("files.status.deleted"),
          renamed: t("files.status.renamed"),
          type_changed: t("files.status.typeChanged"),
          untracked: t("files.status.untracked"),
          conflicted: t("files.status.conflicted"),
        },
      }}
      entries={entries}
      errors={ctx?.errors}
      expanded={expanded}
      selected={ctx?.selected ?? null}
      onToggle={(dir, open) => store().toggle(dir, open)}
      onOpen={(node) => void store().open(node.path)}
      onSelect={(p) => store().select(p)}
      onRetry={(dir) => void store().listDir(dir)}
      menu={menu}
      onRowPointerDown={onRowPointerDown}
      toolbar={toolbar}
      className="min-h-0 flex-1"
    />
  );

  const divider = Math.max(TREE_MIN, Math.min(width - 280, liveDivider ?? ctx?.divider ?? TREE_DEFAULT));
  const startResize = (e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = divider;
    const move = (ev: PointerEvent) => setLiveDivider(Math.max(TREE_MIN, Math.min(width - 280, w0 + ev.clientX - x0)));
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      store().setDivider(Math.max(TREE_MIN, Math.min(width - 280, w0 + ev.clientX - x0)));
      setLiveDivider(null);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const view: FilesView = ctx?.view ?? "files";
  const changes = changeCount(git?.panel);
  const header = (
    <div className="flex h-9 shrink-0 items-center gap-2 border-b border-divider px-2" data-testid="files-header">
      <SegmentedControl
        size="sm"
        label={t("files.view.label")}
        value={view}
        items={[
          { id: "files", label: t("files.view.files") },
          { id: "changes", label: changes > 0 ? t("files.view.changesCount", { n: changes }) : t("files.view.changes") },
        ]}
        onValueChange={(v) => store().setView(v as FilesView)}
      />
      <span className="flex-1" />
      {context && <BranchChip context={context} view={git} />}
    </div>
  );
  // A change opens in the editor (paths in the Changes view are the repository's).
  const openChange = (path: string) => {
    const panel = git?.panel;
    if (!panel) return;
    store().setView("files");
    void store().open(`${panel.root.replace(/[\\/]$/, "")}/${path}`);
  };
  const left = view === "changes" && context ? <ChangesList context={context} view={git} onOpenFile={openChange} /> : tree;
  const narrowChanges =
    showDiff && git?.picked && context ? (
      <ChangeDiff
        view={git}
        onBack={() => {
          setShowDiff(false);
          void useGit.getState().pick(context, null);
        }}
      />
    ) : (
      left
    );

  return (
    <div
      ref={ref}
      className="flex h-full min-h-0 flex-col"
      data-testid="files-panel"
      data-layout={wide ? "wide" : "narrow"}
      data-view={view}
      data-root={root ?? undefined}
    >
      {header}
      {wide ? (
        <div className="flex min-h-0 flex-1">
          <div className="flex min-h-0 shrink-0 flex-col border-r border-divider" style={{ width: divider }}>
            {left}
          </div>
          <div
            role="separator"
            aria-orientation="vertical"
            aria-label={t("files.resize")}
            aria-valuenow={divider}
            aria-valuemin={TREE_MIN}
            aria-valuemax={Math.max(TREE_MIN, width - 280)}
            tabIndex={0}
            data-testid="files-divider"
            onPointerDown={startResize}
            onKeyDown={(e) => {
              if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
                e.preventDefault();
                store().setDivider(divider + (e.key === "ArrowLeft" ? -16 : 16));
              }
            }}
            className="-ml-1 w-2 shrink-0 cursor-col-resize outline-none hover:bg-accent-soft focus-visible:bg-accent-soft"
          />
          {view === "changes" ? <ChangeDiff view={git} /> : <FileEditor />}
        </div>
      ) : view === "changes" ? (
        narrowChanges
      ) : showEditor && (ctx?.tabs.length ?? 0) > 0 ? (
        <FileEditor onBack={() => setShowEditor(false)} />
      ) : (
        tree
      )}
      <NameDialog ask={ask} onClose={() => setAsk(null)} />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => !o && setDeleting(null)}
        danger
        title={t("files.delete.title", { name: deleting?.name ?? "" })}
        body={t("files.delete.body")}
        confirmLabel={t("files.delete.confirm")}
        onConfirm={() => {
          const node = deleting;
          setDeleting(null);
          if (node)
            void store()
              .remove(node.path)
              .then((err) => err && show({ tone: "error", title: err.message }));
        }}
      />
    </div>
  );
}
