import { create } from "zustand";
import type { AgentStatus, StatusRecord } from "@tandem/protocol";
import type { Status } from "@tandem/ui";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

/** Agent status for every pane (F-EN-05), from `status:changed`; plus the
 * unread marks of panes that finished while nobody looked (F-SH-03). */
type StatusState = {
  records: Record<string, StatusRecord>;
  unread: Record<string, true>;
  /** The pane the user is looking at, which never turns unread. */
  focusedKey: string | null;
  apply: (records: StatusRecord[], removed: string[]) => void;
  setFocused: (key: string | null) => void;
};

export const paneKey = (paneId: string) => `pane:${paneId}`;

export const useStatus = create<StatusState>((set, get) => ({
  records: {},
  unread: {},
  focusedKey: null,
  apply: (incoming, removed) => {
    const records = { ...get().records };
    const unread = { ...get().unread };
    for (const r of incoming) {
      const prev = records[r.key];
      if (prev && prev.ts > r.ts) continue;
      records[r.key] = r;
      const finished = r.status === "done" && prev?.status !== "done";
      if (finished && get().focusedKey !== r.key) unread[r.key] = true;
      if (r.status === "working") delete unread[r.key];
    }
    for (const key of removed) {
      delete records[key];
      delete unread[key];
    }
    set({ records, unread });
  },
  setFocused: (key) => {
    const unread = { ...get().unread };
    if (key) delete unread[key];
    set({ focusedKey: key, unread });
  },
}));

/** The status shown for an agent status (UI_SPEC §1.3 tokens). */
export function statusView(s: AgentStatus): Status {
  return s === "failed" ? "error" : s;
}

let following = false;

/** Follows `status:changed`, after loading what's already known (a webview reload). */
export function followStatus(): void {
  if (following) return;
  following = true;
  void listen<{ records: StatusRecord[]; removed?: string[] }>("status:changed", ({ records, removed }) =>
    useStatus.getState().apply(records, removed ?? []),
  );
  void invoke<StatusRecord[]>("status_list")
    .then((list) => useStatus.getState().apply(list ?? [], []))
    .catch(() => undefined);
}
