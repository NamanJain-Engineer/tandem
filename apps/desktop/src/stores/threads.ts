import { create } from "zustand";
import type {
  IpcError,
  SendResult,
  Thread,
  ThreadDetail,
  ThreadEventEnvelope,
  ThreadEventPage,
  ThreadsChanged,
} from "@tandem/protocol";
import { invoke, inTauri, usingFakeBackend } from "../ipc/invoke";
import { listen } from "../ipc/events";
import { GRID_MAX, addCell, neighbor, removeCell, type GridAdd } from "../features/thread/grid";
import { emptyThread, fromHistory, reduce, type ThreadView } from "../features/thread/reducer";
import { useSession } from "./session";
import { useSettings } from "./settings";

/** Events loaded when a thread opens, and per "load older" page. */
export const PAGE = 200;

export type ThreadPatch = Partial<Pick<Thread, "title" | "access" | "mode" | "model" | "effort" | "pinned">>;

/** A thread's transcript in the webview: stored pages, then live events. */
export type ThreadLive = {
  view: ThreadView;
  /** Stored envelopes loaded so far (oldest first). */
  history: ThreadEventEnvelope[];
  /** Live envelopes since subscribing, replayed when older pages load. */
  live: ThreadEventEnvelope[];
  nextBeforeId: number | null;
  detail: ThreadDetail | null;
  status: "loading" | "ready" | "error";
  error: IpcError | null;
};

type ThreadsState = {
  list: Thread[];
  loaded: boolean;
  listError: IpcError | null;
  selectedId: string | null;
  showArchived: boolean;
  byId: Record<string, ThreadLive>;
  refresh: () => Promise<void>;
  select: (id: string | null) => void;
  create: (workspaceId: string | null) => Promise<Thread | IpcError>;
  update: (id: string, patch: ThreadPatch) => Promise<IpcError | null>;
  archive: (id: string, archived: boolean) => Promise<IpcError | null>;
  remove: (id: string) => Promise<IpcError | null>;
  open: (id: string) => Promise<void>;
  loadOlder: (id: string) => Promise<void>;
  reloadDetail: (id: string) => Promise<void>;
  /** F-TH-01 AC2: the thread is on screen (opened, selected or in the grid),
   * so it's read. Reading a thread never marks it read. */
  markRead: (id: string) => Promise<void>;
  send: (
    id: string,
    text: string,
    attachmentIds: string[],
    queueIfBusy: boolean,
  ) => Promise<SendResult | IpcError>;
  stop: (id: string) => Promise<void>;
  retry: (id: string) => Promise<IpcError | null>;
  /** UI-24: which threads show their Changes panel (Mod+Alt+C). */
  changesOpen: Record<string, boolean>;
  toggleChanges: (id?: string) => void;
  /** F-TH-14: threads shown side by side (2–6); empty is the single view. */
  grid: string[];
  gridColumns: number;
  addToGrid: (id: string) => GridAdd;
  removeFromGrid: (id: string) => void;
  focusCell: (dir: "left" | "right" | "up" | "down") => void;
  setGridColumns: (n: number) => void;
  /** The saved grid, once threads and settings are loaded. */
  restoreGrid: () => Promise<void>;
  /** F-TH-10: Act, then send plan `version`; the draft stays. */
  implementPlan: (id: string, version: number) => Promise<IpcError | null>;
  setShowArchived: (v: boolean) => void;
};

const subscribed = new Set<string>();

function isIpcError(e: unknown): e is IpcError {
  return !!e && typeof e === "object" && "code" in e && "message" in e;
}

/** What `thread_subscribe` takes as `channel`. */
type EnvChannel = { onmessage: (m: ThreadEventEnvelope) => void };

async function channel(onEnv: (e: ThreadEventEnvelope) => void): Promise<EnvChannel> {
  if (inTauri() && !usingFakeBackend()) {
    const core = await import("@tauri-apps/api/core");
    const ch = new core.Channel<ThreadEventEnvelope>();
    ch.onmessage = onEnv;
    return ch as unknown as EnvChannel;
  }
  return { onmessage: onEnv };
}

const blank = (): ThreadLive => ({
  view: emptyThread,
  history: [],
  live: [],
  nextBeforeId: null,
  detail: null,
  status: "loading",
  error: null,
});

export const useThreads = create<ThreadsState>((set, get) => {
  const patch = (id: string, next: Partial<ThreadLive>) =>
    set((s) => ({ byId: { ...s.byId, [id]: { ...(s.byId[id] ?? blank()), ...next } } }));

  /** Shown now: the selected thread, or a cell of the grid, while the
   * window is visible. Every thread opened this session stays subscribed, so
   * a turn ending elsewhere must leave it unread. (Which mode or page the
   * window shows isn't known here: a selected thread counts while the person
   * is in Code mode or Settings too.) */
  const onScreen = (id: string) =>
    !document.hidden && (get().selectedId === id || get().grid.includes(id));
  /** Bumped by each mark: a reload that started before a mark can't bring
   * the dot back. */
  const marks: Record<string, number> = {};
  // The window shown again: what it shows is read.
  if (typeof document !== "undefined")
    document.addEventListener("visibilitychange", () => {
      if (document.hidden) return;
      const { selectedId, grid } = get();
      for (const id of new Set([...(selectedId ? [selectedId] : []), ...grid])) {
        if (get().list.find((t) => t.id === id)?.unread) void get().markRead(id);
      }
    });

  const onEnvelope = (env: ThreadEventEnvelope) => {
    const cur = get().byId[env.threadId];
    if (!cur) return;
    // A new turn starting or ending changes the detail (running, queued).
    // A turn that ends on a thread the person is looking at is read already:
    // marked first, so the reload shows it read.
    if (env.event.type === "turn_end" && onScreen(env.threadId)) {
      const id = env.threadId;
      void get()
        .markRead(id)
        .then(() => get().reloadDetail(id));
    } else if (env.event.type === "turn_end" || (env.event.type === "user" && env.seq === 0)) {
      void get().reloadDetail(env.threadId);
    }
    patch(env.threadId, { view: reduce(cur.view, env), live: [...cur.live, env] });
  };

  return {
    list: [],
    loaded: false,
    listError: null,
    selectedId: null,
    showArchived: false,
    byId: {},
    refresh: async () => {
      try {
        const list = await invoke<Thread[]>("thread_list", { kind: "thread", includeArchived: true });
        const selected = get().selectedId;
        set({
          list,
          loaded: true,
          listError: null,
          selectedId: selected && list.some((t) => t.id === selected) ? selected : null,
          grid: livingCells(get().grid, list),
        });
      } catch (e) {
        set({ loaded: true, listError: isIpcError(e) ? e : null });
      }
    },
    select: (id) => {
      set({ selectedId: id });
      if (id) void get().open(id);
    },
    create: async (workspaceId) => {
      try {
        const thread = await invoke<Thread>("thread_create", { args: { kind: "thread", workspaceId } });
        set((s) => ({ list: [thread, ...s.list.filter((t) => t.id !== thread.id)] }));
        get().select(thread.id);
        return thread;
      } catch (e) {
        return isIpcError(e)
          ? e
          : { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
      }
    },
    update: async (id, p) => {
      const before = get().list;
      set({ list: before.map((t) => (t.id === id ? { ...t, ...p } : t)) });
      try {
        const thread = await invoke<Thread>("thread_update", { threadId: id, ...p });
        set((s) => ({ list: s.list.map((t) => (t.id === id ? thread : t)) }));
        const live = get().byId[id];
        if (live?.detail) patch(id, { detail: { ...live.detail, thread } });
        return null;
      } catch (e) {
        set({ list: before });
        return isIpcError(e) ? e : null;
      }
    },
    archive: async (id, archived) => {
      try {
        await invoke("thread_archive", { threadId: id, archived });
        set((s) => ({
          list: s.list.map((t) => (t.id === id ? { ...t, archived } : t)),
          selectedId: archived && s.selectedId === id ? null : s.selectedId,
        }));
        return null;
      } catch (e) {
        return isIpcError(e) ? e : null;
      }
    },
    remove: async (id) => {
      try {
        await invoke("thread_delete", { threadId: id });
        set((s) => ({
          list: s.list.filter((t) => t.id !== id),
          selectedId: s.selectedId === id ? null : s.selectedId,
        }));
        return null;
      } catch (e) {
        return isIpcError(e) ? e : null;
      }
    },
    open: async (id) => {
      const cur = get().byId[id];
      if (cur && cur.status !== "error") {
        // Opened is on screen: read (F-TH-01 AC2), before the detail reloads.
        await get().markRead(id);
        void get().reloadDetail(id);
        return;
      }
      patch(id, blank());
      try {
        // Subscribe first, then read history: a turn that ends in between is
        // in the history, and live events replay on top (the reducer ignores
        // an ended turn's stragglers and replaces rows by seq).
        if (!subscribed.has(id)) {
          subscribed.add(id);
          // One subscription per thread for the app's life: the core streams
          // until the webview goes away.
          const ch = await channel(onEnvelope);
          await invoke("thread_subscribe", { threadId: id, channel: ch }).catch((e: unknown) => {
            subscribed.delete(id);
            throw e;
          });
        }
        // Read once subscribed: a turn that ends from here on reaches
        // onEnvelope, which marks it read again while it's on screen.
        await get().markRead(id);
        const [detail, page] = await Promise.all([
          invoke<ThreadDetail>("thread_get", { threadId: id }),
          invoke<ThreadEventPage>("thread_events", { threadId: id, limit: PAGE }),
        ]);
        const live = get().byId[id]?.live ?? [];
        patch(id, {
          detail,
          history: page.items,
          view: live.reduce(reduce, fromHistory(page.items)),
          nextBeforeId: page.nextBeforeId,
          status: "ready",
          error: null,
        });
      } catch (e) {
        patch(id, { status: "error", error: isIpcError(e) ? e : null });
      }
    },
    loadOlder: async (id) => {
      const cur = get().byId[id];
      if (!cur || cur.nextBeforeId === null) return;
      const page = await invoke<ThreadEventPage>("thread_events", {
        threadId: id,
        beforeId: cur.nextBeforeId,
        limit: PAGE,
      });
      const history = [...page.items, ...cur.history];
      // Older turns go first; live events since subscribing replay on top.
      const view = cur.live.reduce(reduce, fromHistory(history));
      patch(id, { history, view, nextBeforeId: page.nextBeforeId });
    },
    markRead: async (id) => {
      try {
        await invoke("thread_mark_read", { threadId: id });
        marks[id] = (marks[id] ?? 0) + 1;
        set((s) => ({ list: s.list.map((t) => (t.id === id ? { ...t, unread: false } : t)) }));
      } catch {
        // The next open retries.
      }
    },
    reloadDetail: async (id) => {
      const mark = marks[id] ?? 0;
      try {
        const loaded = await invoke<ThreadDetail>("thread_get", { threadId: id });
        // Marked read while this was in flight: it's read.
        const detail =
          (marks[id] ?? 0) !== mark ? { ...loaded, thread: { ...loaded.thread, unread: false } } : loaded;
        patch(id, { detail });
        set((s) => ({ list: s.list.map((t) => (t.id === id ? detail.thread : t)) }));
      } catch {
        // The next event or open retries.
      }
    },
    send: async (id, text, attachmentIds, queueIfBusy) => {
      try {
        const res = await invoke<SendResult>("thread_send", {
          threadId: id,
          text,
          attachmentIds,
          queueIfBusy,
        });
        void get().reloadDetail(id);
        return res;
      } catch (e) {
        return isIpcError(e)
          ? e
          : { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
      }
    },
    stop: async (id) => {
      await invoke("thread_stop", { threadId: id }, { timeoutMs: 6000 }).catch(() => undefined);
      void get().reloadDetail(id);
    },
    retry: async (id) => {
      try {
        await invoke("thread_retry", { threadId: id });
        return null;
      } catch (e) {
        return isIpcError(e) ? e : null;
      }
    },
    implementPlan: async (id, version) => {
      try {
        await invoke<SendResult>("thread_implement_plan", { threadId: id, version, access: null });
        void get().refresh();
        void get().reloadDetail(id);
        return null;
      } catch (e) {
        return isIpcError(e)
          ? e
          : { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
      }
    },
    setShowArchived: (v) => set({ showArchived: v }),
    changesOpen: {},
    toggleChanges: (id) => {
      const key = id ?? get().selectedId;
      if (key) set((s) => ({ changesOpen: { ...s.changesOpen, [key]: !s.changesOpen[key] } }));
    },
    grid: [],
    gridColumns: 1,
    addToGrid: (id) => {
      const limit = useSession.getState().entitlement?.limits.threadGrid ?? GRID_MAX;
      const { grid, result } = addCell(get().grid, get().selectedId, id, limit);
      if (result === "added" || result === "present") {
        if (grid !== get().grid) {
          set({ grid: grid.length > 1 ? grid : [] });
          saveGrid(grid.length > 1 ? grid : []);
        }
        get().select(id);
      }
      return result;
    },
    removeFromGrid: (id) => {
      const { grid, open } = removeCell(get().grid, get().selectedId, id);
      set({ grid });
      saveGrid(grid);
      if (open !== get().selectedId) get().select(open);
    },
    focusCell: (dir) => {
      const { grid, selectedId, gridColumns } = get();
      if (!selectedId || grid.length < 2) return;
      const to = neighbor(grid, selectedId, dir, gridColumns);
      if (to !== selectedId) get().select(to);
    },
    setGridColumns: (n) => {
      if (n !== get().gridColumns) set({ gridColumns: n });
    },
    restoreGrid: async () => {
      if (!useSettings.getState().loaded)
        await useSettings
          .getState()
          .load()
          .catch(() => undefined);
      const saved = useSettings.getState().values.thread_grid as { ids?: unknown } | undefined;
      const ids = Array.isArray(saved?.ids)
        ? saved.ids.filter((x): x is string => typeof x === "string")
        : [];
      const grid = livingCells(ids, get().list);
      if (grid.length < 2) return;
      set({ grid });
      const open = get().selectedId;
      get().select(open && grid.includes(open) ? open : (grid[0] ?? null));
    },
  };
});

let following = false;

/** Grid cells whose threads still exist and aren't archived; one left is no grid. */
function livingCells(ids: string[], list: Thread[]): string[] {
  const alive = ids.filter((id) => list.some((t) => t.id === id && !t.archived));
  return alive.length > 1 ? alive : [];
}

function saveGrid(ids: string[]): void {
  void useSettings.getState().set("thread_grid", { ids });
}

/** Tests: an empty store and no subscriptions. */
export function resetThreads(): void {
  subscribed.clear();
  useThreads.setState({
    list: [],
    loaded: false,
    listError: null,
    selectedId: null,
    showArchived: false,
    byId: {},
    grid: [],
    gridColumns: 1,
    changesOpen: {},
  });
}

/** Loads the thread list and keeps it fresh from `threads:changed`. */
export function followThreads(): void {
  if (following) return;
  following = true;
  void useThreads
    .getState()
    .refresh()
    .then(() => useThreads.getState().restoreGrid());
  void listen<ThreadsChanged>("threads:changed", (p) => {
    useThreads.setState((s) => {
      const grid = livingCells(s.grid, p.threads);
      if (grid.length !== s.grid.length) saveGrid(grid);
      return {
        list: p.threads,
        loaded: true,
        selectedId: s.selectedId && p.threads.some((t) => t.id === s.selectedId) ? s.selectedId : null,
        grid,
      };
    });
  });
}
