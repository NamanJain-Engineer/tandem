import { useCallback, useEffect, useRef, useState } from "react";

/** How long Allow ignores clicks after what it would allow, or its place on
 * screen, changed. */
export const SETTLE_MS = 500;

/** The box an element takes on the screen (the window's place included,
 * so a window moved without resizing counts), as a string to compare. */
function boxOf(el: HTMLElement): string {
  const r = el.getBoundingClientRect();
  return `${window.screenX},${window.screenY},${r.left},${r.top},${r.width},${r.height}`;
}

/** Not displayed: the pane or panel is `display: none`, or the window is.
 * Without `checkVisibility` (older engines) it counts as shown: sampling
 * just goes on. */
function hidden(el: HTMLElement): boolean {
  if (document.hidden) return true;
  const visible = (el as HTMLElement & { checkVisibility?: () => boolean }).checkVisibility;
  return typeof visible === "function" ? !visible.call(el) : false;
}

/** The points a click on `r` could land on: its middle and four corners,
 * each pulled a little inside. */
function pointsOf(r: DOMRect): [number, number][] {
  const dx = Math.min(4, r.width / 4);
  const dy = Math.min(4, r.height / 4);
  return [
    [r.left + r.width / 2, r.top + r.height / 2],
    [r.left + dx, r.top + dy],
    [r.right - dx, r.top + dy],
    [r.left + dx, r.bottom - dy],
    [r.right - dx, r.bottom - dy],
  ];
}

/** Something else (a menu, a dialog, a toast) covers any button in the box
 * (Allow once, Allow for this agent or thread, Deny), at its middle or any
 * corner: a cover over the second button alone counts as much as one over
 * the first. A button with no size takes no clicks and is skipped. */
function covered(el: HTMLElement): boolean {
  if (typeof document.elementFromPoint !== "function") return false;
  const buttons = Array.from(el.querySelectorAll("button"));
  const targets: HTMLElement[] = buttons.length > 0 ? buttons : [el];
  return targets.some((target) => {
    const r = target.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    return pointsOf(r).some(([x, y]) => {
      const hit = document.elementFromPoint(x, y);
      return hit !== null && !target.contains(hit);
    });
  });
}

/** Whether Allow's own place on screen has held still for
 * {@link SETTLE_MS}: a ref for the element holding the buttons, `steady`
 * for their disabled state (sampled every frame), and `check()` for the
 * click or key itself, which measures again on the spot. Any move counts: a
 * card above growing or shrinking, a reorder, a scroll, the panel being
 * shown or resized. Each Allow waits only on its own box, so one agent's
 * moving card can't hold every other Allow disabled. */
export function useSteadyBox(active: boolean): {
  ref: (el: HTMLElement | null) => void;
  steady: boolean;
  check: () => boolean;
} {
  const [el, setEl] = useState<HTMLElement | null>(null);
  const last = useRef<{ box: string; since: number } | null>(null);
  const [steady, setSteady] = useState(false);
  // Still for SETTLE_MS, and uncovered all that time: a cover (an overlay
  // a double-click's first click closes) restarts the wait like a move.
  const sample = useCallback((): boolean => {
    if (el === null) return false;
    const box = covered(el) ? "covered" : boxOf(el);
    const now = performance.now();
    if (last.current === null || last.current.box !== box) last.current = { box, since: now };
    return box !== "covered" && now - last.current.since >= SETTLE_MS;
  }, [el]);
  const check = useCallback((): boolean => {
    if (!active || el === null || hidden(el)) return false;
    return sample();
  }, [active, el, sample]);
  useEffect(() => {
    last.current = null;
    setSteady(false);
    if (!active || el === null) return;
    const next =
      typeof requestAnimationFrame === "function"
        ? requestAnimationFrame
        : (cb: () => void) => window.setTimeout(cb, 16);
    const cancel =
      typeof cancelAnimationFrame === "function" ? cancelAnimationFrame : (id: number) => clearTimeout(id);
    let id = 0;
    let timer = 0;
    const tick = () => {
      // Hidden (a pane in another tab, the panel closed): no per-frame work,
      // a slow look for when it's shown, and the wait starts over then.
      if (hidden(el)) {
        last.current = null;
        setSteady(false);
        timer = window.setTimeout(tick, 250);
        return;
      }
      const now = sample();
      setSteady((was) => (was === now ? was : now));
      id = next(tick);
    };
    id = next(tick);
    return () => {
      cancel(id);
      clearTimeout(timer);
    };
  }, [active, el, sample]);
  return { ref: setEl, steady, check };
}
