// Whose files the Files tab shows (F-CD-19 "follows the active context"):
// the open thread's folder in Thread mode, else the active Code workspace.
// The teammate Home joins with M10.
import { useMemo } from "react";
import type { FileContext } from "@tandem/protocol";
import { useThreads } from "../../stores/threads";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";

export type ActiveFiles =
  | { kind: "context"; context: FileContext; name: string; workspaceId: string | null }
  | { kind: "no_folder" }
  | { kind: "none" };

type Thread = ReturnType<typeof useThreads.getState>["list"][number];
type Workspace = ReturnType<typeof useWorkspaces.getState>["list"][number];

function pick(
  mode: string,
  threadId: string | null,
  thread: Thread | undefined,
  workspaceId: string | null,
  workspace: Workspace | undefined,
  threadWs: Workspace | undefined,
): ActiveFiles {
  if (mode === "thread" && threadId && thread) {
    if (!thread.workspaceId) return { kind: "no_folder" };
    return {
      kind: "context",
      context: { type: "thread", threadId },
      name: threadWs?.name ?? thread.cwd.split(/[\\/]/).filter(Boolean).pop() ?? thread.cwd,
      workspaceId: thread.workspaceId,
    };
  }
  if (workspaceId && workspace) {
    return { kind: "context", context: { type: "workspace", workspaceId }, name: workspace.name, workspaceId };
  }
  return { kind: "none" };
}

export function useActiveFiles(): ActiveFiles {
  const mode = useUi((s) => s.mode);
  const threadId = useThreads((s) => s.selectedId);
  const thread = useThreads((s) => (s.selectedId ? s.list.find((t) => t.id === s.selectedId) : undefined));
  const workspaceId = useWorkspaces((s) => s.activeId);
  const workspace = useWorkspaces((s) => (s.activeId ? s.list.find((w) => w.id === s.activeId) : undefined));
  const threadWs = useWorkspaces((s) => (thread?.workspaceId ? s.list.find((w) => w.id === thread.workspaceId) : undefined));
  return useMemo<ActiveFiles>(
    () => pick(mode, threadId, thread, workspaceId, workspace, threadWs),
    [mode, threadId, thread, workspaceId, workspace, threadWs],
  );
}

/** What the Files tab shows now, outside React (a terminal link, F-CD-09). */
export function activeFiles(): ActiveFiles {
  const { selectedId, list } = useThreads.getState();
  const thread = selectedId ? list.find((t) => t.id === selectedId) : undefined;
  const ws = useWorkspaces.getState();
  const workspace = ws.activeId ? ws.list.find((w) => w.id === ws.activeId) : undefined;
  const threadWs = thread?.workspaceId ? ws.list.find((w) => w.id === thread.workspaceId) : undefined;
  return pick(useUi.getState().mode, selectedId, thread, ws.activeId, workspace, threadWs);
}
