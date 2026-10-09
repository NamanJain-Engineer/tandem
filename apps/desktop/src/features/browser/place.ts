// Where a browser page sits (F-CD-17, D-206). A page is a webview the core
// draws above the whole app, so nothing the app draws can cover it: a view
// reports its place in window coordinates and hides its page while the app
// must show something there (any dialog, or a menu or list over the place).
// Toasts move to the bottom-left while a page covers their corner.
import { useEffect, useRef, type RefObject } from "react";
import type { Rect } from "@tandem/protocol";

/** Dialogs dim the whole window: every page hides. */
const MODAL = "[role=dialog],[role=alertdialog]";
/** Popups hide the pages they overlap. */
const POPUP = "[role=menu],[role=listbox]";

export const ZERO: Rect = { x: 0, y: 0, width: 0, height: 0 };

export function overlaps(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

const rectOf = (r: DOMRect): Rect => ({ x: r.x, y: r.y, width: r.width, height: r.height });

/** Whether the app must show something over `place` now. */
export function covered(place: Rect, root: ParentNode = document): boolean {
  if (root.querySelector(MODAL)) return true;
  for (const el of root.querySelectorAll(POPUP)) {
    const r = el.getBoundingClientRect();
    if (r.width > 0 && r.height > 0 && overlaps(place, rectOf(r))) return true;
  }
  return false;
}

export type Placed = { bounds: Rect; shown: boolean };

/** Shown pages by id, for the toasts' corner. */
const shownPages = new Map<string, Rect>();

/** UI_SPEC §2.12 puts toasts bottom-right (`bottom-8 right-4`, up to 3). */
function toastCorner(): Rect {
  const width = 380;
  const height = 220;
  return { x: window.innerWidth - 16 - width, y: window.innerHeight - 32 - height, width, height };
}

function updateToastSide(): void {
  const corner = toastCorner();
  const covers = [...shownPages.values()].some((r) => overlaps(r, corner));
  const root = document.documentElement;
  if (covers) root.dataset.toasts = "left";
  else delete root.dataset.toasts;
}

/** Records whether page `id` shows at `bounds` (null: it doesn't). */
export function notePage(id: string, bounds: Rect | null): void {
  if (bounds) shownPages.set(id, bounds);
  else shownPages.delete(id);
  updateToastSide();
}

/** Follows where `ref` sits and whether a page may show there: on resizes,
 * popups opening and closing, and a slow check for moves nothing reports
 * (a banner pushing the layout down). */
export function usePlace(ref: RefObject<HTMLElement | null>, onChange: (placed: Placed) => void): void {
  const latest = useRef(onChange);
  latest.current = onChange;
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let last = "";
    let frames = 0;
    let raf = 0;
    const measure = () => {
      const bounds = rectOf(el.getBoundingClientRect());
      const shown = bounds.width >= 1 && bounds.height >= 1 && !covered(bounds);
      const key = `${bounds.x},${bounds.y},${bounds.width},${bounds.height},${shown}`;
      if (key === last) return;
      last = key;
      latest.current({ bounds, shown });
    };
    // A popup places itself a frame or two after it mounts.
    const tick = () => {
      raf = 0;
      measure();
      if (--frames > 0) raf = requestAnimationFrame(tick);
    };
    const settle = () => {
      frames = 8;
      if (!raf) raf = requestAnimationFrame(tick);
    };
    measure();
    const sized = new ResizeObserver(measure);
    sized.observe(el);
    const popups = new MutationObserver(settle);
    popups.observe(document.body, { childList: true });
    window.addEventListener("resize", measure);
    const poll = window.setInterval(measure, 250);
    return () => {
      sized.disconnect();
      popups.disconnect();
      window.removeEventListener("resize", measure);
      window.clearInterval(poll);
      cancelAnimationFrame(raf);
    };
  }, [ref]);
}
