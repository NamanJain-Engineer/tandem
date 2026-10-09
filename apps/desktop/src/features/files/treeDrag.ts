// Dragging out of the Files tree (F-CD-06, F-TH-07): onto a terminal or agent
// pane it types the shell-quoted path at the caret (never Enter); onto the
// open thread it attaches. Pointer events rather than HTML5 drag-and-drop,
// which WebView2 doesn't deliver while native file drops are on.
import type { PointerEvent as ReactPointerEvent } from "react";
import { dropPaths, useDropTarget } from "../code/drops";
import { dropOnThread, useThreadDrop } from "../thread/drops";

const THRESHOLD = 5;

type Toast = (t: { tone?: "info" | "success" | "warning" | "error"; title: string }) => void;

type Target = { kind: "pane"; paneId: string } | { kind: "thread" } | null;

function targetAt(x: number, y: number): Target {
  const el = document.elementFromPoint(x, y);
  const pane = el?.closest<HTMLElement>('[data-testid="pane"]');
  if (pane?.dataset.paneId) return { kind: "pane", paneId: pane.dataset.paneId };
  if (el?.closest('[data-testid="thread-main"]')) return { kind: "thread" };
  return null;
}

function clearHighlights() {
  if (useDropTarget.getState().paneId) useDropTarget.setState({ paneId: null });
  if (useThreadDrop.getState().over) useThreadDrop.setState({ over: false });
}

/** Starts following a pointer that went down on a tree row; it becomes a
 * drag after a few pixels. Resolves with where it was dropped. */
export function startTreeDrag(
  e: ReactPointerEvent<HTMLElement>,
  path: string,
  label: string,
  toast: Toast,
): Promise<Target> {
  return new Promise((resolve) => {
    if (e.button !== 0 || !path) {
      resolve(null);
      return;
    }
    const start = { x: e.clientX, y: e.clientY };
    let ghost: HTMLDivElement | null = null;
    const done = (target: Target) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", key, true);
      ghost?.remove();
      document.body.style.removeProperty("cursor");
      clearHighlights();
      resolve(target);
    };
    const move = (ev: PointerEvent) => {
      if (!ghost) {
        if (Math.hypot(ev.clientX - start.x, ev.clientY - start.y) < THRESHOLD) return;
        ghost = document.createElement("div");
        ghost.textContent = label;
        ghost.dataset.testid = "file-drag-ghost";
        ghost.className =
          "pointer-events-none fixed z-[var(--z-max)] max-w-[240px] truncate rounded-sm border border-border bg-surface-3 px-2 py-0.5 text-sm text-fg shadow-2";
        document.body.appendChild(ghost);
        document.body.style.cursor = "grabbing";
      }
      ghost.style.left = `${ev.clientX + 12}px`;
      ghost.style.top = `${ev.clientY + 8}px`;
      const t = targetAt(ev.clientX, ev.clientY);
      const paneId = t?.kind === "pane" ? t.paneId : null;
      if (useDropTarget.getState().paneId !== paneId) useDropTarget.setState({ paneId });
      const over = t?.kind === "thread";
      if (useThreadDrop.getState().over !== over) useThreadDrop.setState({ over });
    };
    const up = (ev: PointerEvent) => {
      if (!ghost) {
        done(null);
        return;
      }
      const t = targetAt(ev.clientX, ev.clientY);
      done(t);
      if (t?.kind === "pane") void dropPaths(t.paneId, [path], toast);
      else if (t?.kind === "thread") void dropOnThread([path], toast);
    };
    const key = (ev: KeyboardEvent) => {
      if (ev.key === "Escape" && ghost) {
        ev.preventDefault();
        ev.stopPropagation();
        done(null);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("keydown", key, true);
  });
}
