import { create } from "zustand";

/** What Agent mode's main area shows: the selected teammate, or one of the
 * rail footer's pages (UI-70: Tasks, Routines). */
export type AgentPage = "teammate" | "tasks" | "routines";

export const useAgentPage = create<{ page: AgentPage; show: (page: AgentPage) => void }>((set) => ({
  page: "teammate",
  show: (page) => set({ page }),
}));
