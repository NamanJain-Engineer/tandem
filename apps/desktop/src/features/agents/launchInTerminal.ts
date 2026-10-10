import type { IpcError, PaneRecord } from "@tandem/protocol";
import { invoke } from "../../ipc/invoke";
import { useFlash } from "../../stores/focus";
import { place, type Outcome } from "../../stores/launch";
import { useLayout } from "../../stores/layout";
import { switchMode } from "../shell/actions";

/** "Launch in terminal" (F-AG-13): a Code-mode agent pane with the
 * teammate's engine and account in `placePath`, named after it, whose first
 * prompt is its brief. It opens in Code mode, focused. */
export async function launchInTerminal(teammateId: string, placePath: string): Promise<Outcome<PaneRecord>> {
  let pane: PaneRecord;
  try {
    pane = await invoke<PaneRecord>(
      "teammate_launch_in_terminal",
      { id: teammateId, placePath, tabId: null },
      { timeoutMs: 30_000 },
    );
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
  await place(
    [{ seatIndex: 0, pane, workspaceId: pane.workspaceId, tabId: pane.tabId, laneBranch: null }],
    pane.tabId,
  );
  switchMode("code");
  useLayout.getState().focus(pane.tabId, pane.id);
  useFlash.getState().flash(pane.id);
  return { ok: true, value: pane };
}
