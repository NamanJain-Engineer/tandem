// The Files tab (F-CD-19): per context (a Code workspace or a thread's
// folder) the tree, the open files and their editor state. Saves check the
// hash the file had when it was read, so a file an agent changed meanwhile
// raises the conflict prompt instead of being overwritten (AC1); auto-save
// runs 800 ms after the last edit (AC2); tabs, expanded folders and the
// layout persist per context (AC4).
import { create } from "zustand";
import type {
  EditorTabs,
  FileContent,
  FileContext,
  FileDiff,
  FilesView,
  FsChange,
  FsEntry,
  FsKind,
  IpcError,
  Saved,
} from "@tandem/protocol";
import { inTauri, invoke, usingFakeBackend } from "../ipc/invoke";
import { useSettings } from "./settings";
import { useThreads } from "./threads";
import { useWorkspaces } from "./workspaces";

export const AUTOSAVE_MS = 800;
const PERSIST_MS = 400;
/** Saves and reads can take a moment on a slow disk. */
const IO_TIMEOUT_MS = 30_000;

export type Goto = { line: number; col?: number; nonce: number };

export type OpenFile = {
  path: string;
  status: "loading" | "ready" | "error";
  content: FileContent | null;
  /** The editor's text. */
  text: string;
  /** The hash the next save checks against: from the last read or save. */
  hash: string | null;
  dirty: boolean;
  saving: boolean;
  error: IpcError | null;
  /** A save found the file changed on disk (the conflict dialog). */
  conflict: boolean;
  /** The file changed on disk while it had unsaved edits. */
  changedOnDisk: boolean;
  goto: Goto | null;
};

export type FilesContext = {
  context: FileContext;
  entries: Record<string, FsEntry[] | undefined>;
  errors: Record<string, string>;
  expanded: string[];
  showIgnored: boolean;
  selected: string | null;
  tabs: OpenFile[];
  active: string | null;
  divider: number | null;
  view: FilesView;
  /** The saved tabs have been read back. */
  restored: boolean;
  /** Watch batches so far: the Changes view reads the status again on each. */
  batches: number;
};

type FilesStore = {
  key: string | null;
  byKey: Record<string, FilesContext>;
  setContext: (context: FileContext | null) => Promise<void>;
  listDir: (dir: string) => Promise<void>;
  refreshAll: () => Promise<void>;
  toggle: (dir: string, open: boolean) => void;
  setShowIgnored: (on: boolean) => void;
  select: (path: string | null) => void;
  /** Opens (or focuses) a file, optionally at a line. */
  open: (path: string, at?: { line: number; col?: number }) => Promise<void>;
  activate: (path: string) => void;
  /** Closing a tab selects its left neighbour. */
  close: (path: string) => void;
  edit: (path: string, text: string) => void;
  save: (path: string) => Promise<boolean>;
  reload: (path: string) => Promise<void>;
  overwrite: (path: string) => Promise<boolean>;
  compare: (path: string) => Promise<FileDiff | IpcError>;
  setDivider: (px: number) => void;
  setView: (view: FilesView) => void;
  /** Shows `view` for the context `key` as soon as it's the tab's (the
   * status bar's branch opens Changes this way). */
  showView: (key: string, view: FilesView) => void;
  create: (path: string, kind: FsKind) => Promise<IpcError | null>;
  rename: (from: string, to: string) => Promise<IpcError | null>;
  remove: (path: string) => Promise<IpcError | null>;
  /** A batch from the folder's watcher. */
  applyChange: (key: string, change: FsChange) => Promise<void>;
};

export function contextKey(c: FileContext): string {
  return c.type === "workspace" ? `ws:${c.workspaceId}` : `th:${c.threadId}`;
}

/** The context's folder on disk, for absolute paths (drag out, Copy path). */
export function contextRoot(c: FileContext): string | null {
  if (c.type === "workspace") return useWorkspaces.getState().list.find((w) => w.id === c.workspaceId)?.root ?? null;
  const th = useThreads.getState().list.find((x) => x.id === c.threadId);
  return th && th.workspaceId ? th.cwd : null;
}

/** An entry's absolute path (a tab may already be absolute). */
export function absolutePath(c: FileContext, path: string): string | null {
  if (path.startsWith("/") || /^[A-Za-z]:[\\/]/.test(path)) return path;
  const root = contextRoot(c);
  if (!root) return null;
  return path ? `${root.replace(/[\\/]$/, "")}/${path}` : root;
}

function isIpcError(e: unknown): e is IpcError {
  return !!e && typeof e === "object" && "code" in e && "message" in e;
}

function asError(e: unknown): IpcError {
  return isIpcError(e) ? e : { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
}

const blankFile = (path: string): OpenFile => ({
  path,
  status: "loading",
  content: null,
  text: "",
  hash: null,
  dirty: false,
  saving: false,
  error: null,
  conflict: false,
  changedOnDisk: false,
  goto: null,
});

const blankContext = (context: FileContext): FilesContext => ({
  context,
  entries: {},
  errors: {},
  expanded: [],
  showIgnored: false,
  selected: null,
  tabs: [],
  active: null,
  divider: null,
  view: "files",
  restored: false,
  batches: 0,
});

function parentOf(path: string): string {
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at);
}

/** What `fs_watch` takes as `channel`. */
type ChangeChannel = { onmessage: (c: FsChange) => void };

async function changeChannel(on: (c: FsChange) => void): Promise<ChangeChannel> {
  if (inTauri() && !usingFakeBackend()) {
    const core = await import("@tauri-apps/api/core");
    const ch = new core.Channel<FsChange>();
    ch.onmessage = on;
    return ch as unknown as ChangeChannel;
  }
  return { onmessage: on };
}

const autosave = new Map<string, ReturnType<typeof setTimeout>>();
let persistTimer: ReturnType<typeof setTimeout> | undefined;
let watching: { key: string; sub: string | null } | null = null;
let pendingView: { key: string; view: FilesView } | null = null;
let generation = 0;

function autoSaveOn(): boolean {
  return useSettings.getState().values.auto_save_files !== false;
}

export const useFiles = create<FilesStore>((set, get) => {
  const cur = (): FilesContext | null => {
    const key = get().key;
    return key ? (get().byKey[key] ?? null) : null;
  };
  const patchCtx = (key: string, fn: (c: FilesContext) => Partial<FilesContext>) =>
    set((s) => {
      const c = s.byKey[key];
      return c ? { byKey: { ...s.byKey, [key]: { ...c, ...fn(c) } } } : {};
    });
  const patchFile = (key: string, path: string, next: Partial<OpenFile> | ((f: OpenFile) => Partial<OpenFile>)) =>
    patchCtx(key, (c) => ({
      tabs: c.tabs.map((f) => (f.path === path ? { ...f, ...(typeof next === "function" ? next(f) : next) } : f)),
    }));
  const fileOf = (key: string, path: string) => get().byKey[key]?.tabs.find((f) => f.path === path) ?? null;

  /** Saves the tab state shortly after the last change. */
  const persist = () => {
    const key = get().key;
    if (!key || !cur()?.restored) return;
    clearTimeout(persistTimer);
    persistTimer = setTimeout(() => {
      const c = get().byKey[key];
      if (!c) return;
      const state: EditorTabs = {
        tabs: c.tabs.map((f) => ({ path: f.path })),
        active: c.active,
        expanded: c.expanded,
        showIgnored: c.showIgnored,
        divider: c.divider,
        view: c.view,
      };
      void invoke("editor_tabs_save", { contextKey: key, state }).catch(() => undefined);
    }, PERSIST_MS);
  };

  /** Writes a file of context `key` if it has unsaved edits. */
  const saveIn = async (key: string, path: string): Promise<boolean> => {
    const c = get().byKey[key];
    const f = fileOf(key, path);
    if (!c || !f) return false;
    if (!f.dirty || f.saving || f.conflict || f.content?.readonly) return !f.dirty;
    const text = f.text;
    patchFile(key, path, { saving: true });
    try {
      const saved = await invoke<Saved>(
        "fs_write",
        { context: c.context, path, text, expectedHash: f.hash },
        { timeoutMs: IO_TIMEOUT_MS },
      );
      patchFile(key, path, (x) => ({
        saving: false,
        hash: saved.hash,
        dirty: x.text !== text,
        error: null,
        changedOnDisk: false,
        content: x.content ? { ...x.content, hash: saved.hash, mtime: saved.mtime } : x.content,
      }));
      return true;
    } catch (e) {
      const err = asError(e);
      patchFile(key, path, err.code === "conflict" ? { saving: false, conflict: true } : { saving: false, error: err });
      return false;
    }
  };

  const read = async (key: string, path: string): Promise<FileContent | IpcError> => {
    const c = get().byKey[key];
    if (!c) return asError("closed");
    try {
      return await invoke<FileContent>("fs_read", { context: c.context, path }, { timeoutMs: IO_TIMEOUT_MS });
    } catch (e) {
      return asError(e);
    }
  };

  const load = async (key: string, path: string) => {
    const got = await read(key, path);
    if (isIpcError(got)) {
      patchFile(key, path, { status: "error", error: got });
      return;
    }
    patchFile(key, path, { status: "ready", content: got, text: got.text ?? "", hash: got.hash, dirty: false, error: null, changedOnDisk: false });
  };

  const watch = async (key: string, context: FileContext) => {
    const mine = { key, sub: null as string | null };
    watching = mine;
    const ch = await changeChannel((change) => void get().applyChange(key, change));
    try {
      const sub = await invoke<string>("fs_watch", { context, channel: ch });
      if (watching === mine) mine.sub = sub;
      else void invoke("fs_unwatch", { subId: sub }).catch(() => undefined);
    } catch {
      // The tree still works; it just won't follow changes on disk.
    }
  };

  const unwatch = () => {
    const w = watching;
    watching = null;
    if (w?.sub) void invoke("fs_unwatch", { subId: w.sub }).catch(() => undefined);
  };

  return {
    key: null,
    byKey: {},

    setContext: async (context) => {
      const key = context ? contextKey(context) : null;
      if (key === get().key) return;
      const mine = ++generation;
      // Pending auto-saves of the context we leave go out now.
      for (const [id, timer] of autosave) {
        clearTimeout(timer);
        autosave.delete(id);
        const [k, ...rest] = id.split("\n");
        if (k) void saveIn(k, rest.join("\n"));
      }
      unwatch();
      set({ key });
      if (!context || !key) return;
      if (!get().byKey[key]) set((s) => ({ byKey: { ...s.byKey, [key]: blankContext(context) } }));
      void watch(key, context);
      if (!get().byKey[key]?.restored) {
        const saved = await invoke<EditorTabs>("editor_tabs_get", { contextKey: key }).catch(() => null);
        if (mine !== generation && get().key !== key) return;
        patchCtx(key, () => ({
          restored: true,
          expanded: saved?.expanded ?? [],
          showIgnored: saved?.showIgnored ?? false,
          divider: saved?.divider ?? null,
          view: saved?.view ?? "files",
          tabs: (saved?.tabs ?? []).map((t) => blankFile(t.path)),
          active: saved?.active ?? null,
        }));
        for (const t of saved?.tabs ?? []) void load(key, t.path);
      }
      if (pendingView?.key === key && get().key === key) {
        const { view } = pendingView;
        pendingView = null;
        patchCtx(key, () => ({ view }));
        persist();
      }
      await get().refreshAll();
    },

    listDir: async (dir) => {
      const key = get().key;
      const c = cur();
      if (!key || !c) return;
      try {
        const list = await invoke<FsEntry[]>("fs_list", { context: c.context, dir, showIgnored: c.showIgnored });
        patchCtx(key, (x) => {
          const { [dir]: _, ...errors } = x.errors;
          return { entries: { ...x.entries, [dir]: list }, errors };
        });
      } catch (e) {
        const err = asError(e);
        // A folder that went away just closes.
        if (err.code === "not_found" && dir !== "") {
          patchCtx(key, (x) => {
            const { [dir]: _, ...entries } = x.entries;
            return { entries, expanded: x.expanded.filter((p) => p !== dir && !p.startsWith(`${dir}/`)) };
          });
          return;
        }
        patchCtx(key, (x) => ({ errors: { ...x.errors, [dir]: err.message } }));
      }
    },

    refreshAll: async () => {
      const c = cur();
      if (!c) return;
      const dirs = ["", ...c.expanded.filter((d) => c.expanded.includes(parentOf(d)) || parentOf(d) === "")];
      await Promise.all(dirs.map((d) => get().listDir(d)));
    },

    toggle: (dir, open) => {
      const key = get().key;
      if (!key) return;
      patchCtx(key, (c) => ({
        expanded: open ? [...new Set([...c.expanded, dir])] : c.expanded.filter((p) => p !== dir && !p.startsWith(`${dir}/`)),
      }));
      if (open && cur()?.entries[dir] === undefined) void get().listDir(dir);
      persist();
    },

    setShowIgnored: (on) => {
      const key = get().key;
      if (!key) return;
      patchCtx(key, () => ({ showIgnored: on }));
      persist();
      void get().refreshAll();
    },

    select: (path) => {
      const key = get().key;
      if (key) patchCtx(key, () => ({ selected: path }));
    },

    open: async (path, at) => {
      const key = get().key;
      const c = cur();
      if (!key || !c) return;
      const goto = at ? { line: at.line, col: at.col, nonce: Date.now() } : null;
      const existing = c.tabs.find((f) => f.path === path);
      if (existing) {
        patchCtx(key, (x) => ({ active: path, selected: path, tabs: x.tabs.map((f) => (f.path === path && goto ? { ...f, goto } : f)) }));
        persist();
        return;
      }
      patchCtx(key, (x) => {
        const at = x.active ? x.tabs.findIndex((f) => f.path === x.active) : -1;
        const tabs = [...x.tabs];
        tabs.splice(at < 0 ? tabs.length : at + 1, 0, { ...blankFile(path), goto });
        return { tabs, active: path, selected: path };
      });
      persist();
      await load(key, path);
      // A link may name an absolute path inside the folder: key the tab by
      // the path the core shows (relative), so it is the tree's tab.
      const shown = fileOf(key, path)?.content?.path;
      if (shown && shown !== path) {
        const twin = get().byKey[key]?.tabs.find((f) => f.path === shown);
        patchCtx(key, (x) => ({
          tabs: twin
            ? x.tabs.filter((f) => f.path !== path).map((f) => (f.path === shown && goto ? { ...f, goto } : f))
            : x.tabs.map((f) => (f.path === path ? { ...f, path: shown } : f)),
          active: x.active === path ? shown : x.active,
          selected: shown,
        }));
        persist();
      }
    },

    activate: (path) => {
      const key = get().key;
      if (!key) return;
      patchCtx(key, () => ({ active: path, selected: path }));
      persist();
    },

    close: (path) => {
      const key = get().key;
      const c = cur();
      if (!key || !c) return;
      const at = c.tabs.findIndex((f) => f.path === path);
      if (at < 0) return;
      const id = `${key}\n${path}`;
      clearTimeout(autosave.get(id));
      autosave.delete(id);
      const tabs = c.tabs.filter((f) => f.path !== path);
      const active = c.active === path ? (tabs[Math.max(0, at - 1)]?.path ?? null) : c.active;
      patchCtx(key, () => ({ tabs, active }));
      persist();
    },

    edit: (path, text) => {
      const key = get().key;
      if (!key) return;
      const f = fileOf(key, path);
      if (!f || f.content?.readonly || f.text === text) return;
      patchFile(key, path, { text, dirty: true });
      const id = `${key}\n${path}`;
      clearTimeout(autosave.get(id));
      if (autoSaveOn()) {
        autosave.set(
          id,
          setTimeout(() => {
            autosave.delete(id);
            void saveIn(key, path);
          }, AUTOSAVE_MS),
        );
      }
    },

    save: async (path) => {
      const key = get().key;
      return key ? saveIn(key, path) : false;
    },

    reload: async (path) => {
      const key = get().key;
      if (!key) return;
      const id = `${key}\n${path}`;
      clearTimeout(autosave.get(id));
      autosave.delete(id);
      patchFile(key, path, { conflict: false });
      await load(key, path);
    },

    overwrite: async (path) => {
      const key = get().key;
      if (!key) return false;
      const now = await read(key, path);
      // Deleted meanwhile: writing creates it again.
      const hash = isIpcError(now) ? null : now.hash;
      if (isIpcError(now) && now.code !== "not_found") {
        patchFile(key, path, { error: now });
        return false;
      }
      patchFile(key, path, { conflict: false, hash, dirty: true });
      return get().save(path);
    },

    compare: async (path) => {
      const c = cur();
      const f = c?.tabs.find((x) => x.path === path);
      if (!c || !f) return asError("closed");
      try {
        return await invoke<FileDiff>("fs_compare", { context: c.context, path, text: f.text }, { timeoutMs: IO_TIMEOUT_MS });
      } catch (e) {
        return asError(e);
      }
    },

    setDivider: (px) => {
      const key = get().key;
      if (!key) return;
      patchCtx(key, () => ({ divider: Math.round(px) }));
      persist();
    },

    setView: (view) => {
      const key = get().key;
      if (!key) return;
      patchCtx(key, () => ({ view }));
      persist();
    },

    showView: (key, view) => {
      if (get().key === key && get().byKey[key]?.restored) {
        patchCtx(key, () => ({ view }));
        persist();
      } else {
        pendingView = { key, view };
      }
    },

    create: async (path, kind) => {
      const c = cur();
      if (!c) return null;
      try {
        await invoke("fs_create", { context: c.context, path, kind });
        await get().listDir(parentOf(path));
        if (kind === "file") await get().open(path);
        return null;
      } catch (e) {
        return asError(e);
      }
    },

    rename: async (from, to) => {
      const key = get().key;
      const c = cur();
      if (!key || !c) return null;
      try {
        await invoke("fs_rename", { context: c.context, from, to });
        patchCtx(key, (x) => ({
          tabs: x.tabs.map((f) =>
            f.path === from || f.path.startsWith(`${from}/`) ? { ...f, path: to + f.path.slice(from.length) } : f,
          ),
          active: x.active && (x.active === from || x.active.startsWith(`${from}/`)) ? to + x.active.slice(from.length) : x.active,
        }));
        persist();
        await Promise.all([get().listDir(parentOf(from)), get().listDir(parentOf(to))]);
        return null;
      } catch (e) {
        return asError(e);
      }
    },

    remove: async (path) => {
      const key = get().key;
      const c = cur();
      if (!key || !c) return null;
      try {
        await invoke("fs_delete", { context: c.context, path });
        for (const f of c.tabs) if (f.path === path || f.path.startsWith(`${path}/`)) get().close(f.path);
        await get().listDir(parentOf(path));
        return null;
      } catch (e) {
        return asError(e);
      }
    },

    applyChange: async (key, change) => {
      if (get().key !== key) return;
      const c = get().byKey[key];
      if (!c) return;
      patchCtx(key, (x) => ({ batches: x.batches + 1 }));
      const touched = new Set(change.paths);
      // Open files: a clean one follows the disk; a dirty one keeps its text
      // and says the file changed (saving it raises the conflict prompt).
      for (const f of c.tabs) {
        if (!change.overflow && !touched.has(f.path)) continue;
        if (f.saving) continue;
        if (f.dirty) {
          const now = await read(key, f.path);
          if (!isIpcError(now) && now.hash !== f.hash) patchFile(key, f.path, { changedOnDisk: true });
        } else if (f.status !== "loading") {
          const now = await read(key, f.path);
          if (isIpcError(now)) {
            if (now.code === "not_found") patchFile(key, f.path, { status: "error", error: now });
          } else if (now.hash !== f.hash && !fileOf(key, f.path)?.dirty) {
            patchFile(key, f.path, { status: "ready", content: now, text: now.text ?? "", hash: now.hash, error: null });
          }
        }
      }
      // Badges and listings: every folder that's shown.
      await get().refreshAll();
    },
  };
});

/** Tests: forget everything without saving. */
export function resetFiles(): void {
  for (const t of autosave.values()) clearTimeout(t);
  autosave.clear();
  clearTimeout(persistTimer);
  watching = null;
  pendingView = null;
  generation++;
  useFiles.setState({ key: null, byKey: {} });
}
