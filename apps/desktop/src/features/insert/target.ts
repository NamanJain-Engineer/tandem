// Where inserted text goes (F-EX-02 recall, F-EX-05 prompts): the composer of
// the thread on screen, else the focused pane, else the clipboard. Inserting
// never sends: a pane gets a paste (bracketed when the program asked) with no
// Enter after it, a composer gets text after what's typed.
import { copyText } from "../../ipc/clipboard";
import { useLayout } from "../../stores/layout";
import { useThreads } from "../../stores/threads";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { terminalHandles } from "../code/TerminalView";
import { insertIntoComposer } from "../thread/drops";

export type InsertTarget =
  | { kind: "composer"; threadId: string }
  | { kind: "pane"; paneId: string; workspaceId: string; agent: boolean; name: string | null }
  | { kind: "clipboard" };

/** The target on screen now (taken when a picker opens, before it has focus). */
export function currentTarget(): InsertTarget {
  const ui = useUi.getState();
  if (ui.mode === "thread") {
    const id = useThreads.getState().selectedId;
    return id ? { kind: "composer", threadId: id } : { kind: "clipboard" };
  }
  if (ui.mode === "code") {
    const ws = useWorkspaces.getState();
    const tabId = ws.activeId ? ws.activeTab[ws.activeId] : null;
    const tab = tabId ? useLayout.getState().tabs[tabId] : undefined;
    const pane = tab?.active ? tab.panes[tab.active] : undefined;
    if (pane && (pane.kind === "terminal" || pane.kind === "agent"))
      return { kind: "pane", paneId: pane.id, workspaceId: pane.workspaceId, agent: pane.kind === "agent", name: pane.humanName };
  }
  return { kind: "clipboard" };
}

/** The workspace the target belongs to (recall's "This workspace"). */
export function targetWorkspace(target: InsertTarget): string | null {
  if (target.kind === "pane") return target.workspaceId;
  if (target.kind === "composer") return useThreads.getState().list.find((t) => t.id === target.threadId)?.workspaceId ?? null;
  return useWorkspaces.getState().activeId;
}

/** Puts `text` into the target. Never sends. */
export async function insertText(target: InsertTarget, text: string): Promise<"inserted" | "copied" | "failed"> {
  if (target.kind === "composer") {
    insertIntoComposer(target.threadId, text);
    return "inserted";
  }
  if (target.kind === "pane") {
    const handle = terminalHandles.get(target.paneId);
    if (handle) {
      handle.paste(text);
      // After the picker closes and gives focus back.
      setTimeout(() => handle.focus(), 0);
      return "inserted";
    }
  }
  return (await copyText(text)) ? "copied" : "failed";
}
