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
import { emptyThread, fromHistory, reduce, type ThreadView } from "../features/thread/reducer";

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
  send: (id: string, text: string, attachmentIds: string[], queueIfBusy: boolean) => Promise<SendResult | IpcError>;
  stop: (id: string) => Promise<void>;
  retry: (id: string) => Promise<IpcError | null>;
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

  const onEnvelope = (env: ThreadEventEnvelope) => {
    const cur = get().byId[env.threadId];
    if (!cur) return;
    // A new turn starting or ending changes the detail (running, queued).
    if (env.event.type === "turn_end" || (env.event.type === "user" && env.seq === 0)) {
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
        return isIpcError(e) ? e : { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
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
        set((s) => ({ list: s.list.filter((t) => t.id !== id), selectedId: s.selectedId === id ? null : s.selectedId }));
        return null;
      } catch (e) {
        return isIpcError(e) ? e : null;
      }
    },
    open: async (id) => {
      const cur = get().byId[id];
      if (cur && cur.status !== "error") {
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
      const page = await invoke<ThreadEventPage>("thread_events", { threadId: id, beforeId: cur.nextBeforeId, limit: PAGE });
      const history = [...page.items, ...cur.history];
      // Older turns go first; live events since subscribing replay on top.
      const view = cur.live.reduce(reduce, fromHistory(history));
      patch(id, { history, view, nextBeforeId: page.nextBeforeId });
    },
    reloadDetail: async (id) => {
      try {
        const detail = await invoke<ThreadDetail>("thread_get", { threadId: id });
        patch(id, { detail });
        set((s) => ({ list: s.list.map((t) => (t.id === id ? detail.thread : t)) }));
      } catch {
        // The next event or open retries.
      }
    },
    send: async (id, text, attachmentIds, queueIfBusy) => {
      try {
        const res = await invoke<SendResult>("thread_send", { threadId: id, text, attachmentIds, queueIfBusy });
        void get().reloadDetail(id);
        return res;
      } catch (e) {
        return isIpcError(e) ? e : { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
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
        return isIpcError(e) ? e : { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
      }
    },
    setShowArchived: (v) => set({ showArchived: v }),
  };
});

let following = false;

/** Tests: an empty store and no subscriptions. */
export function resetThreads(): void {
  subscribed.clear();
  useThreads.setState({ list: [], loaded: false, listError: null, selectedId: null, showArchived: false, byId: {} });
}

/** Loads the thread list and keeps it fresh from `threads:changed`. */
export function followThreads(): void {
  if (following) return;
  following = true;
  void useThreads.getState().refresh();
  void listen<ThreadsChanged>("threads:changed", (p) => {
    useThreads.setState((s) => ({
      list: p.threads,
      loaded: true,
      selectedId: s.selectedId && p.threads.some((t) => t.id === s.selectedId) ? s.selectedId : null,
    }));
  });
}
