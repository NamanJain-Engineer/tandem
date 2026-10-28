import { useCallback, useEffect, useMemo, useRef, useState } from "react";

/** How long Allow ignores clicks after what it would allow, or its place on
 * screen, changed. */
export const SETTLE_MS = 500;

/** How often a box in view looks for a cover between clicks. The click
 * itself always looks again on the spot. */
export const COVER_EVERY_MS = 100;

/** The attribute that marks a control as one that grants something (Allow,
 * Approve, Trust): only those are checked for covers (`data-grant="false"`
 * doesn't count). */
export const GRANT_ATTR = "data-grant";

/** How long after its release a press still counts as the one a click
 * belongs to (the click follows the release). */
const PRESS_GRACE_MS = 250;
/** A press with no release seen for this long is stale (a test, or a lost
 * event), and no longer holds anything back. */
const PRESS_STALE_MS = 2_000;

/** The person's latest press: pointerdown, or a key that isn't a repeat,
 * until its release. A held key's repeats keep it live but never start a
 * new one. */
let press: { at: number; seen: number; live: boolean } | null = null;
let tracking = false;

function trackPresses() {
  if (tracking || typeof document === "undefined") return;
  tracking = true;
  const start = () => {
    const now = performance.now();
    press = { at: now, seen: now, live: true };
  };
  const end = () => {
    if (press) {
      press.live = false;
      press.seen = performance.now();
    }
  };
  document.addEventListener("pointerdown", start, true);
  document.addEventListener(
    "keydown",
    (e) => {
      if (!e.repeat) start();
      else if (press) press.seen = performance.now();
    },
    true,
  );
  document.addEventListener("pointerup", end, true);
  document.addEventListener("pointercancel", end, true);
  document.addEventListener("keyup", end, true);
}

/** When the press a click or key now belongs to began, if there is one. */
function pressBegan(now: number): number | null {
  if (press === null) return null;
  const current = press.live ? now - press.seen < PRESS_STALE_MS : now - press.seen < PRESS_GRACE_MS;
  return current ? press.at : null;
}

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

/** Something else (a menu, a dialog, a toast) covers a control in the box
 * that grants (marked {@link GRANT_ATTR}), at its middle or any corner: a
 * cover over "Allow for this agent" alone counts as much as one over Allow
 * once. A toast over Deny doesn't hold Allow off. Without marked controls
 * the box itself is checked. A control with no size takes no clicks and is
 * skipped. */
function covered(el: HTMLElement): boolean {
  if (typeof document.elementFromPoint !== "function") return false;
  const marked = Array.from(
    el.querySelectorAll<HTMLElement>(`[${GRANT_ATTR}]:not([${GRANT_ATTR}="false"])`),
  );
  const targets: HTMLElement[] = marked.length > 0 ? marked : [el];
  return targets.some((target) => {
    const r = target.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    return pointsOf(r).some(([x, y]) => {
      // Past the window's edge nothing can be seen to be clicked.
      if (x < 0 || y < 0 || x >= window.innerWidth || y >= window.innerHeight) return true;
      const hit = document.elementFromPoint(x, y);
      return hit !== null && !target.contains(hit);
    });
  });
}

/** Whether the whole element is in view, by an IntersectionObserver
 * (clipped by its scroller, scrolled partly or wholly away counts as out).
 * Without one (a test), it's in view. */
function useInView(el: HTMLElement | null): boolean {
  const [inView, setInView] = useState(typeof IntersectionObserver === "undefined");
  useEffect(() => {
    if (el === null || typeof IntersectionObserver === "undefined") return;
    // A new element starts out of view until the observer says otherwise.
    setInView(false);
    const io = new IntersectionObserver(
      (entries) => {
        const last = entries[entries.length - 1];
        if (last) setInView(last.isIntersecting && (last.intersectionRatio ?? 1) >= 0.99);
      },
      { threshold: [0, 0.99, 1] },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [el]);
  return inView;
}

/** Whether Allow's own place on screen has held still for
 * {@link SETTLE_MS}: a ref for the element holding the buttons, `steady`
 * for their disabled state, and `check()` for the click or key itself,
 * which measures again on the spot. Any move counts: a card above growing
 * or shrinking, a reorder, a scroll, the panel being shown or resized. Each
 * Allow waits only on its own box, so one agent's moving card can't hold
 * every other Allow disabled.
 *
 * `key` names what the box would grant (the request): a new one starts the
 * wait over, even on the same element, and `steady` and `check()` say no
 * for it until it has waited. A box out of view or hidden does no work
 * per frame and starts over when it's back, and `check()` says no for it;
 * one in view measures its place every frame and looks for covers every
 * {@link COVER_EVERY_MS} and at every pointer press. */
export function useSteadyBox(
  active: boolean,
  key: string | null = null,
): {
  ref: (el: HTMLElement | null) => void;
  steady: boolean;
  check: () => boolean;
} {
  const [el, setEl] = useState<HTMLElement | null>(null);
  useEffect(trackPresses, []);
  // A new token per request, even back to an earlier one (A → B → A).
  const token = useMemo(() => ({}), [key]);
  const inView = useInView(el);
  const last = useRef<{ box: string; since: number; token: object } | null>(null);
  const cover = useRef<{ at: number; covered: boolean } | null>(null);
  const [steadyFor, setSteadyFor] = useState<object | null>(null);
  // Still for SETTLE_MS, and uncovered all that time: a cover (an overlay
  // a double-click's first click closes) restarts the wait like a move.
  const sample = useCallback(
    (exact: boolean): boolean => {
      if (el === null) return false;
      const now = performance.now();
      if (exact || cover.current === null || now - cover.current.at >= COVER_EVERY_MS) {
        cover.current = { at: now, covered: covered(el) };
      }
      const box = cover.current.covered ? "covered" : boxOf(el);
      if (last.current === null || last.current.box !== box || last.current.token !== token) {
        last.current = { box, since: now, token };
      }
      return box !== "covered" && now - last.current.since >= SETTLE_MS;
    },
    [el, token],
  );
  // Out of view answers no too: a focused card scrolled away can't be
  // allowed by a key typed into what the person thinks is the composer.
  // And the press the click or key belongs to must have begun once this
  // request had held still: a press that began on what was here before
  // (pressed on request A, released after B took its place, or a key held
  // across the change) grants nothing.
  const check = useCallback((): boolean => {
    if (!active || el === null || !inView || hidden(el)) return false;
    if (!sample(true) || last.current === null) return false;
    const began = pressBegan(performance.now());
    return began === null || began >= last.current.since + SETTLE_MS;
  }, [active, el, inView, sample]);
  useEffect(() => {
    last.current = null;
    cover.current = null;
    setSteadyFor(null);
    if (!active || el === null || !inView) return;
    const next =
      typeof requestAnimationFrame === "function"
        ? requestAnimationFrame
        : (cb: () => void) => window.setTimeout(cb, 16);
    const cancel =
      typeof cancelAnimationFrame === "function" ? cancelAnimationFrame : (id: number) => clearTimeout(id);
    let id = 0;
    let timer = 0;
    // Every press anywhere looks for a cover too, so one that comes and
    // goes between samples (an overlay a double-click's first press closes)
    // still starts the wait over.
    const press = () => {
      if (!hidden(el)) sample(true);
    };
    document.addEventListener("pointerdown", press, true);
    const tick = () => {
      // Hidden (a pane in another tab, the panel closed): no per-frame work,
      // a slow look for when it's shown, and the wait starts over then.
      if (hidden(el)) {
        last.current = null;
        cover.current = null;
        setSteadyFor(null);
        timer = window.setTimeout(tick, 250);
        return;
      }
      const now = sample(false) ? token : null;
      setSteadyFor((was) => (was === now ? was : now));
      id = next(tick);
    };
    id = next(tick);
    return () => {
      cancel(id);
      clearTimeout(timer);
      document.removeEventListener("pointerdown", press, true);
    };
  }, [active, el, inView, sample, token]);
  return { ref: setEl, steady: steadyFor !== null && steadyFor === token, check };
}
