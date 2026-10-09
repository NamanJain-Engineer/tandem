// The scratchpad (F-SH-12): local Markdown notes, saved as you type.
//
// Saving is throttled, not debounced: the first unsaved keystroke starts a
// 300 ms clock, and later keystrokes don't push it back. So even while
// someone types without pausing, a crash can lose at most the last 300 ms
// of input (F-SH-12 AC1). Writes go one at a time, in order.
import { create } from "zustand";
import type { IpcError, ScratchNote } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";

/** The longest an edit waits before it's written. */
export const SAVE_MS = 300;

type Status = "idle" | "loading" | "ready" | "error";

type ScratchpadState = {
  notes: ScratchNote[];
  status: Status;
  /** Why the list couldn't load. */
  error: string | null;
  /** The note open in the editor, and its text as the editor has it. */
  activeId: string | null;
  text: string;
  /** The open note is still being read. */
  opening: boolean;
  /** The last save failed (the text is kept and saved again on the next
   * edit, or Retry). */
  saveError: string | null;
  /** Grows when a new note wants the cursor. */
  focusNonce: number;
  load: () => Promise<void>;
  open: (id: string) => Promise<void>;
  create: () => Promise<void>;
  edit: (text: string) => void;
  /** Writes what's waiting now (switching notes, closing, Retry). */
  flush: () => Promise<void>;
  remove: (id: string) => Promise<IpcError | null>;
  /** Back to the list (the narrow layout). */
  close: () => Promise<void>;
};

function message(e: unknown): string {
  return e && typeof e === "object" && "message" in e ? String((e as IpcError).message) : String(e);
}

/** The saved note in the list, newest first. */
function upsert(notes: ScratchNote[], note: ScratchNote): ScratchNote[] {
  return [note, ...notes.filter((n) => n.id !== note.id)];
}

let timer: ReturnType<typeof setTimeout> | null = null;
let pending: { id: string; text: string } | null = null;
let inflight: Promise<void> | null = null;

function schedule(): void {
  if (timer) return;
  timer = setTimeout(() => {
    timer = null;
    void save();
  }, SAVE_MS);
}

/** Writes the waiting text, after any write already on its way. */
async function save(): Promise<void> {
  while (inflight) await inflight;
  const job = pending;
  if (!job) return;
  pending = null;
  inflight = (async () => {
    try {
      const note = await invoke<ScratchNote>("scratchpad_write", { id: job.id, markdown: job.text });
      useScratchpad.setState((s) => ({ notes: upsert(s.notes, note), saveError: null }));
    } catch (e) {
      // Kept for the next try, unless newer text has replaced it.
      pending ??= job;
      useScratchpad.setState({ saveError: message(e) });
    } finally {
      inflight = null;
    }
  })();
  await inflight;
}

async function flushNow(): Promise<void> {
  if (timer) {
    clearTimeout(timer);
    timer = null;
  }
  await save();
}

export const useScratchpad = create<ScratchpadState>((set, get) => ({
  notes: [],
  status: "idle",
  error: null,
  activeId: null,
  text: "",
  opening: false,
  saveError: null,
  focusNonce: 0,

  load: async () => {
    set({ status: get().status === "ready" ? "ready" : "loading", error: null });
    try {
      const notes = await invoke<ScratchNote[]>("scratchpad_list");
      set({ notes, status: "ready" });
      const { activeId } = get();
      if (activeId && !notes.some((n) => n.id === activeId)) set({ activeId: null, text: "" });
    } catch (e) {
      set({ status: "error", error: message(e) });
    }
  },

  open: async (id) => {
    if (get().activeId === id) return;
    await flushNow();
    set({ activeId: id, text: "", opening: true, saveError: null });
    try {
      const text = await invoke<string>("scratchpad_read", { id });
      if (get().activeId === id) set({ text, opening: false });
    } catch (e) {
      if (get().activeId === id) set({ activeId: null, opening: false });
      set({ error: message(e) });
      void get().load();
    }
  },

  create: async () => {
    await flushNow();
    try {
      const note = await invoke<ScratchNote>("scratchpad_write", { id: null, markdown: "" });
      set((s) => ({
        notes: upsert(s.notes, note),
        activeId: note.id,
        text: "",
        opening: false,
        saveError: null,
        status: "ready",
        focusNonce: s.focusNonce + 1,
      }));
    } catch (e) {
      set({ saveError: message(e) });
    }
  },

  edit: (text) => {
    const id = get().activeId;
    if (!id || get().opening) return;
    set({ text });
    pending = { id, text };
    schedule();
  },

  flush: flushNow,

  remove: async (id) => {
    if (pending?.id === id) pending = null;
    while (inflight) await inflight;
    try {
      await invoke("scratchpad_delete", { id });
    } catch (e) {
      const err = e as IpcError;
      if (err?.code !== "not_found") return err;
    }
    set((s) => ({
      notes: s.notes.filter((n) => n.id !== id),
      ...(s.activeId === id ? { activeId: null, text: "", saveError: null } : {}),
    }));
    return null;
  },

  close: async () => {
    await flushNow();
    set({ activeId: null, text: "", saveError: null });
  },
}));

/** Tests: forget everything, dropping anything unsaved. */
export function resetScratchpad(): void {
  if (timer) clearTimeout(timer);
  timer = null;
  pending = null;
  inflight = null;
  useScratchpad.setState({ notes: [], status: "idle", error: null, activeId: null, text: "", opening: false, saveError: null, focusNonce: 0 });
}
