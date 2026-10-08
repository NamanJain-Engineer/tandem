// File drops onto a thread (F-TH-07): files and folders dropped anywhere on
// the open thread become composer attachments. The OS drop arrives as one
// Tauri event with absolute paths and a position, like terminal drops.
import { useEffect } from "react";
import { create } from "zustand";
import type { Attachment, IpcError } from "@tandem/protocol";
import { inTauri, invoke, usingFakeBackend } from "../../ipc/invoke";
import { useThreads } from "../../stores/threads";

type Toast = (t: { tone?: "info" | "success" | "warning" | "error"; title: string }) => void;

/** The drag is over the thread view (the composer shows "Drop to attach"),
 * and attachments a drop made that the composer hasn't taken yet. */
export const useThreadDrop = create<{ over: boolean; dropped: Record<string, Attachment[]> }>(() => ({
  over: false,
  dropped: {},
}));

/** Takes the dropped attachments for a thread (once). */
export function takeDropped(threadId: string): Attachment[] {
  const list = useThreadDrop.getState().dropped[threadId] ?? [];
  if (list.length) {
    useThreadDrop.setState((s) => {
      const { [threadId]: _, ...rest } = s.dropped;
      return { dropped: rest };
    });
  }
  return list;
}

function overThread(x: number, y: number): boolean {
  return !!document.elementFromPoint(x, y)?.closest('[data-testid="thread-main"]');
}

/** Attaches `paths` to the open thread. */
export async function dropOnThread(paths: string[], toast: Toast): Promise<boolean> {
  const threadId = useThreads.getState().selectedId;
  if (!threadId || paths.length === 0) return false;
  try {
    const added = await invoke<Attachment[]>("thread_attach_paths", { threadId, paths });
    useThreadDrop.setState((s) => ({
      dropped: { ...s.dropped, [threadId]: [...(s.dropped[threadId] ?? []), ...added] },
    }));
    return true;
  } catch (e) {
    toast({ tone: "error", title: (e as IpcError).message });
    return false;
  }
}

/** Follows OS drag-and-drop over the thread view while Thread mode is mounted. */
export function useThreadDrops(toast: Toast): void {
  useEffect(() => {
    const drop = async (paths: string[], x: number, y: number) => {
      useThreadDrop.setState({ over: false });
      if (overThread(x, y)) await dropOnThread(paths, toast);
    };
    // Debug builds: the E2E harness drives drops through the same path.
    const test = window as unknown as {
      __tandemThreadDrop?: (paths: string[], x: number, y: number) => Promise<void>;
    };
    test.__tandemThreadDrop = drop;
    let unlisten: (() => void) | undefined;
    let live = true;
    if (inTauri() && !usingFakeBackend()) {
      void import("@tauri-apps/api/webview").then(({ getCurrentWebview }) =>
        getCurrentWebview()
          .onDragDropEvent((event) => {
            const scale = window.devicePixelRatio || 1;
            const p = event.payload;
            if (p.type === "enter" || p.type === "over") {
              const over = overThread(p.position.x / scale, p.position.y / scale);
              if (over !== useThreadDrop.getState().over) useThreadDrop.setState({ over });
            } else if (p.type === "drop") void drop(p.paths, p.position.x / scale, p.position.y / scale);
            else useThreadDrop.setState({ over: false });
          })
          .then((f) => {
            if (live) unlisten = f;
            else f();
          }),
      );
    }
    return () => {
      live = false;
      unlisten?.();
      delete test.__tandemThreadDrop;
    };
  }, [toast]);
}
