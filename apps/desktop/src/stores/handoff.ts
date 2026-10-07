import { create } from "zustand";
import type { EngineView, HandoffBrief, HandoffPlace, HandoffResult, IpcError } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";
import { place, type Outcome } from "./launch";
import { useLayout } from "./layout";
import { useWorkspaces } from "./workspaces";

/** The pane being handed off (UI-13). */
export type HandoffSource = {
  paneId: string;
  workspaceId: string;
  /** `agent` or `terminal`. */
  kind: string;
  engineId: string | null;
};

type HandoffState = {
  source: HandoffSource | null;
  /** "Handed off to …" on source panes, by pane id (F-CD-16). */
  notes: Record<string, string>;
  open: (source: HandoffSource) => void;
  close: () => void;
  /** Sets or (null) dismisses a pane's note. */
  note: (paneId: string, text: string | null) => void;
};

export const useHandoff = create<HandoffState>((set) => ({
  source: null,
  notes: {},
  open: (source) => set({ source }),
  close: () => set({ source: null }),
  note: (paneId, text) =>
    set((s) => {
      const notes = { ...s.notes };
      if (text === null) delete notes[paneId];
      else notes[paneId] = text;
      return { notes };
    }),
}));

/** The engine a handoff starts with: another installed engine than the
 * source's, Claude Code first; the source's own when it's the only one. */
export function defaultTarget(engines: EngineView[], sourceEngine: string | null): string | null {
  const installed = engines.filter((e) => e.install !== null && e.unsupportedReason === null);
  const others = installed.filter((e) => e.spec.id !== sourceEngine);
  const pick = (list: EngineView[]) => list.find((e) => e.spec.id === "claude") ?? list[0];
  return (pick(others) ?? pick(installed))?.spec.id ?? null;
}

/** Briefs take a while: the source writes one, or the target runs headless
 * (up to 2 minutes each). */
const BRIEF_TIMEOUT_MS = 270_000;

export async function generateBrief(paneId: string, engineId: string): Promise<Outcome<HandoffBrief>> {
  try {
    const brief = await invoke<HandoffBrief>(
      "handoff_brief",
      { paneId, engineId, accountId: null },
      { timeoutMs: BRIEF_TIMEOUT_MS },
    );
    return { ok: true, value: brief };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}

/** Starts the target and shows it: split right of the source when it stays
 * in the same folder, else in its new worktree's tab. */
export async function startHandoff(
  source: HandoffSource,
  engineId: string,
  brief: string,
  where: HandoffPlace,
  closeSource: boolean,
): Promise<Outcome<HandoffResult>> {
  let result: HandoffResult;
  try {
    result = await invoke<HandoffResult>("handoff_start", {
      paneId: source.paneId,
      engineId,
      accountId: null,
      brief,
      place: where,
      closeSource,
    });
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
  const layout = useLayout.getState;
  if (where === "same") {
    if (!layout().loaded[result.workspaceId])
      await layout()
        .load(result.workspaceId)
        .catch(() => undefined);
    const tab = layout().tabs[result.tabId];
    if (tab && !tab.panes[result.pane.id]) {
      layout().insert(result.tabId, result.pane, tab.panes[source.paneId] ? source.paneId : null, "right");
    }
    useWorkspaces.getState().selectTab(result.workspaceId, result.tabId);
  } else {
    await place(
      [
        {
          seatIndex: 0,
          pane: result.pane,
          workspaceId: result.workspaceId,
          tabId: result.tabId,
          laneBranch: result.laneBranch,
        },
      ],
      result.tabId,
    );
  }
  if (result.sourceClosed) layout().dropPane(source.paneId);
  return { ok: true, value: result };
}
