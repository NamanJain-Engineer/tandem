import { create } from "zustand";
import type { IpcError, TeammateSummary, TeammatesChanged } from "@tandem/protocol";
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
  refresh: () => Promise<void>;
  select: (id: string | null) => void;
};

function isIpcError(e: unknown): e is IpcError {
  return !!e && typeof e === "object" && "code" in e && "message" in e;
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
  select: (id) => set({ selectedId: id }),
}));

let following = false;

/** Loads the teammates and follows `teammates:changed`. */
export function followTeammates(): void {
  if (following) return;
  following = true;
  void useTeammates.getState().refresh();
  void listen<TeammatesChanged>("teammates:changed", (p) => {
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
  useTeammates.setState({ list: [], loaded: false, error: null, version: 0, selectedId: null });
}
