import { create } from "zustand";
import type { IpcError, Teammate, TeammatePatch, TeammateSummary, TeammatesChanged, Thread } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

/** Agent mode's teammates (F-AG-01, F-AG-02): the rail's summaries, kept
 * fresh from `teammates:changed`. `version` grows with every change, so
 * views that read per-teammate data (memory, skills, chats) reload. */
type TeammatesState = {
  list: TeammateSummary[];
  loaded: boolean;
  error: IpcError | null;
  version: number;
  selectedId: string | null;
  /** The open settings have unsaved changes (the guard asks first). */
  dirty: boolean;
  /** A teammate the user picked while settings were unsaved. */
  pendingId: string | null;
  setDirty: (d: boolean) => void;
  /** Leaves unsaved settings for the pending teammate (or stays). */
  resolvePending: (leave: boolean) => void;
  /** Each teammate's chats, in tab order. */
  chats: Record<string, Thread[]>;
  /** The open chat per teammate. */
  chatOf: Record<string, string>;
  refresh: () => Promise<void>;
  select: (id: string | null) => void;
  loadChats: (teammateId: string) => Promise<void>;
  selectChat: (teammateId: string, chatId: string) => void;
  newChat: (teammateId: string, place?: string | null) => Promise<Thread | IpcError>;
  update: (id: string, patch: TeammatePatch) => Promise<Teammate | IpcError>;
  remove: (id: string) => Promise<IpcError | null>;
  duplicate: (id: string) => Promise<Teammate | IpcError>;
};

function isIpcError(e: unknown): e is IpcError {
  return !!e && typeof e === "object" && "code" in e && "message" in e;
}

function asError(e: unknown): IpcError {
  return isIpcError(e) ? e : { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
}

export const useTeammates = create<TeammatesState>((set, get) => ({
  list: [],
  loaded: false,
  error: null,
  version: 0,
  selectedId: null,
  refresh: async () => {
    try {
      const list = await invoke<TeammateSummary[]>("teammate_list");
      const selected = get().selectedId;
      set((s) => ({
        list,
        loaded: true,
        error: null,
        version: s.version + 1,
        selectedId: selected && list.some((t) => t.teammate.id === selected) ? selected : null,
      }));
    } catch (e) {
      set({ loaded: true, error: isIpcError(e) ? e : null });
    }
  },
  select: (id) => {
    if (get().dirty && id !== get().selectedId) {
      set({ pendingId: id });
      return;
    }
    set({ selectedId: id, pendingId: null });
    if (id) void get().loadChats(id);
  },
  dirty: false,
  pendingId: null,
  setDirty: (dirty) => set({ dirty }),
  resolvePending: (leave) => {
    const id = get().pendingId;
    if (!leave) {
      set({ pendingId: null });
      return;
    }
    set({ dirty: false, pendingId: null });
    get().select(id);
  },
  chats: {},
  chatOf: {},
  loadChats: async (teammateId) => {
    try {
      const list = await invoke<Thread[]>("thread_list", { kind: "teammate_chat", teammateId, includeArchived: false });
      set((s) => {
        const open = s.chatOf[teammateId];
        const keep = open && list.some((c) => c.id === open) ? open : (list[list.length - 1]?.id ?? undefined);
        const chatOf = { ...s.chatOf };
        if (keep) chatOf[teammateId] = keep;
        else delete chatOf[teammateId];
        return { chats: { ...s.chats, [teammateId]: list }, chatOf };
      });
    } catch {
      // The next change reloads.
    }
  },
  selectChat: (teammateId, chatId) => set((s) => ({ chatOf: { ...s.chatOf, [teammateId]: chatId } })),
  newChat: async (teammateId, place) => {
    try {
      const chat = await invoke<Thread>("thread_create", {
        args: { kind: "teammate_chat", teammateId, place: place ?? null },
      });
      set((s) => ({
        chats: { ...s.chats, [teammateId]: [...(s.chats[teammateId] ?? []).filter((c) => c.id !== chat.id), chat] },
        chatOf: { ...s.chatOf, [teammateId]: chat.id },
        selectedId: teammateId,
      }));
      return chat;
    } catch (e) {
      return asError(e);
    }
  },
  update: async (id, patch) => {
    try {
      const t = await invoke<Teammate>("teammate_update", { id, patch });
      set((s) => ({ list: s.list.map((x) => (x.teammate.id === id ? { ...x, teammate: t } : x)) }));
      return t;
    } catch (e) {
      return asError(e);
    }
  },
  remove: async (id) => {
    try {
      await invoke("teammate_delete", { id });
      set((s) => ({
        list: s.list.filter((x) => x.teammate.id !== id),
        selectedId: s.selectedId === id ? null : s.selectedId,
      }));
      return null;
    } catch (e) {
      return asError(e);
    }
  },
  duplicate: async (id) => {
    try {
      const t = await invoke<Teammate>("teammate_duplicate", { id });
      await get().refresh();
      return t;
    } catch (e) {
      return asError(e);
    }
  },
}));

let following = false;

/** Loads the teammates and follows `teammates:changed`. */
export function followTeammates(): void {
  if (following) return;
  following = true;
  void useTeammates.getState().refresh();
  void listen<TeammatesChanged>("teammates:changed", (p) => {
    const open = useTeammates.getState().selectedId;
    if (open) void useTeammates.getState().loadChats(open);
    useTeammates.setState((s) => ({
      list: p.teammates,
      loaded: true,
      error: null,
      version: s.version + 1,
      selectedId:
        s.selectedId && p.teammates.some((t) => t.teammate.id === s.selectedId) ? s.selectedId : null,
    }));
  });
}

/** Tests: an empty store. */
export function resetTeammates(): void {
  following = false;
  useTeammates.setState({
    list: [],
    loaded: false,
    error: null,
    version: 0,
    selectedId: null,
    chats: {},
    chatOf: {},
    dirty: false,
    pendingId: null,
  });
}
