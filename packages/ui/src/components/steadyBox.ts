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

/** Keys that only modify another: they never start a press. */
const MODIFIERS = new Set([
  "Shift",
  "Control",
  "Alt",
  "AltGraph",
  "Meta",
  "OS",
  "Super",
  "Hyper",
  "Fn",
  "CapsLock",
  "NumLock",
  "ScrollLock",
]);

/** An event's time on `performance.now()`'s clock (some engines and jsdom
 * stamp events with the epoch instead). One that's ahead of now, or more
 * than a second behind, can't be trusted (an event clock that drifted from
 * the page's after a clock slew, seen at +440 ms): it's held to now, or to a
 * second ago. */
function stamp(e: Event): number {
  const raw = e.timeStamp;
  const t = raw > 1e11 ? raw - performance.timeOrigin : raw;
  const n = performance.now();
  // Ahead: now. Over a second behind: a second ago, never now, so an old
  // press start can't pass for a new one.
  return t > n ? n : Math.max(t, n - 1_000);
}

type Press = { at: number; seen: number; live: boolean };
/** Every press the person is making or just made, by pointer
 * (`p:<pointerId>`) and by key (`k:<code>`): a second press (a key tapped,
 * another finger) while one is held doesn't take its place. A held key's
 * repeats keep its press live but never start a new one. */
const presses = new Map<string, Press>();
/** The time of the event being handled now (a click, a key), so a slow
 * handler is judged by when the event happened. */
let handling: number | null = null;
let tracking = false;

function trackPresses() {
  if (tracking || typeof document === "undefined") return;
  tracking = true;
  const begin = (id: string, e: Event) => {
    const t = stamp(e);
    presses.set(id, { at: t, seen: t, live: true });
  };
  const release = (id: string, e: Event) => {
    const p = presses.get(id);
    if (p) {
      p.live = false;
      p.seen = stamp(e);
    }
  };
  // The mouse is one press whatever its pointerId; touches and pens each
  // have their own, and so does a pointer that doesn't say what it is (a
  // synthetic event), so it can't restart a held mouse press.
  const pointer = (e: PointerEvent) =>
    e.pointerType === "mouse" ? "p:mouse" : `p:${e.pointerType || "untyped"}:${e.pointerId ?? 0}`;
  const key = (e: KeyboardEvent) => `k:${e.code || e.key}`;
  document.addEventListener("pointerdown", (e) => begin(pointer(e), e), true);
  // After a release the page never saw (the window went away mid-press),
  // the engine thinks the button is still down: the next press comes as a
  // pointermove with the primary button newly down (button 0, buttons 1)
  // and a mousedown, with no pointerdown. That pointermove, from a pointer
  // that says it's the mouse, starts the mouse's press (one with no
  // pointerType is synthetic). A mousedown alone never does: a tap's compatibility
  // mousedown (WebView2) or a synthetic one would let another finger restart
  // a held mouse press.
  document.addEventListener(
    "pointermove",
    (e) => e.pointerType === "mouse" && e.button === 0 && (e.buttons & 1) === 1 && begin("p:mouse", e),
    true,
  );
  document.addEventListener("pointerup", (e) => release(pointer(e), e), true);
  document.addEventListener("pointercancel", (e) => release(pointer(e), e), true);
  document.addEventListener(
    "keydown",
    (e) => {
      if (MODIFIERS.has(e.key)) return;
      const p = presses.get(key(e));
      if (e.repeat && p) p.seen = stamp(e);
      else if (!e.repeat) begin(key(e), e);
    },
    true,
  );
  document.addEventListener("keyup", (e) => release(key(e), e), true);
  // No reset when the window loses focus: a press held across it (the
  // pointer grab keeps its release and click coming) must still be judged by
  // when it began, and an unreleased one goes stale on its own.
  for (const type of ["click", "keydown", "keyup"] as const) {
    document.addEventListener(
      type,
      (e) => {
        handling = stamp(e);
        setTimeout(() => (handling = null), 0);
      },
      true,
    );
  }
}

/** When the earliest press a click or key may belong to began: one still
 * held (and not stale), or released within the grace. None: null. */
function pressBegan(): number | null {
  const now = handling ?? performance.now();
  let earliest: number | null = null;
  for (const [id, p] of presses) {
    const current = p.live ? now - p.seen < PRESS_STALE_MS : now - p.seen < PRESS_GRACE_MS;
    if (!current) {
      if (now - p.seen > PRESS_STALE_MS) presses.delete(id);
      continue;
    }
    if (earliest === null || p.at < earliest) earliest = p.at;
  }
  return earliest;
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

/** A corner's radius in px across (`h`) and down (`v`), from its computed
 * `border-*-radius` ("6px", "9999px", "50%", "8px 4px"), at most half the
 * side it rounds. A value that doesn't read as a length (one an engine
 * keeps as `calc(50% - 1px)`) is read as the most the corner can round:
 * read as none, a pill's corner point sat outside its curve and the control
 * counted as covered for good (the review of ce0d5815). */
function radius(value: string, width: number, height: number): [number, number] {
  const [h = "0", v = h] = value.trim().split(/\s+/);
  const px = (s: string, side: number) => {
    if (s === "") return 0;
    const n = Number.parseFloat(s);
    if (Number.isNaN(n)) return side / 2;
    if (!Number.isFinite(n) || n <= 0) return 0;
    return Math.min(s.endsWith("%") ? (n * side) / 100 : n, side / 2);
  };
  return [px(h, width), px(v, height)];
}

/** The points a click on `target` could land on: its middle and four
 * corners, each pulled a little inside, and past a rounded corner's curve
 * (a pill's corner is outside it, and WebView2 hit-tests the curve). */
function pointsOf(target: HTMLElement, r: DOMRect): [number, number][] {
  const style = typeof getComputedStyle === "function" ? getComputedStyle(target) : null;
  const curve = 1 - Math.SQRT1_2;
  // How far in from a corner its point sits: 4 px (a quarter of a small
  // side), or 1 px inside the curve on its 45° line, never past the middle.
  // A radius larger than the one drawn only moves the point further in.
  const inset = (corner: string): [number, number] => {
    const [rx, ry] = radius(style?.getPropertyValue(`border-${corner}-radius`) ?? "", r.width, r.height);
    return [
      Math.min(r.width / 2, Math.max(Math.min(4, r.width / 4), rx > 0 ? rx * curve + 1 : 0)),
      Math.min(r.height / 2, Math.max(Math.min(4, r.height / 4), ry > 0 ? ry * curve + 1 : 0)),
    ];
  };
  const tl = inset("top-left");
  const tr = inset("top-right");
  const bl = inset("bottom-left");
  const br = inset("bottom-right");
  return [
    [r.left + r.width / 2, r.top + r.height / 2],
    [r.left + tl[0], r.top + tl[1]],
    [r.right - tr[0], r.top + tr[1]],
    [r.left + bl[0], r.bottom - bl[1]],
    [r.right - br[0], r.bottom - br[1]],
  ];
}

/** Nothing on the page takes pointer events (a modal menu turns them off
 * for everything but itself), so a click at a point the menu doesn't cover
 * reaches nothing. */
function pageInert(): boolean {
  return typeof getComputedStyle === "function" && getComputedStyle(document.body).pointerEvents === "none";
}

/** Something else (a menu, a dialog, a toast) covers a control in the box
 * that grants (marked {@link GRANT_ATTR}), at its middle or any corner: a
 * cover over "Allow for this agent" alone counts as much as one over Allow
 * once. A toast over Deny doesn't hold Allow off. Without marked controls
 * the box itself is checked. A control with no size takes no clicks and is
 * skipped.
 *
 * Only the control itself at a point is clear. No element there counts as
 * covered, as a point past the window or clipped by a scroller does; the
 * page's root counts as clear only while a modal menu has turned pointer
 * events off page-wide, so the menu doesn't hold every Allow off. */
function covered(el: HTMLElement): boolean {
  if (typeof document.elementFromPoint !== "function") return false;
  // The page's own viewport, the one rects and hit tests use. A phone
  // zoomed in shrinks innerWidth and innerHeight to what it shows, and
  // Allow below that counted as past the window, waiting for good (the
  // review of ce0d5815, SF-3). The larger of the two: a phone's browser
  // bars collapsed show more than clientHeight says, and Allow in that
  // strip counted as past the window (the review of b3ff3696, N-2).
  const root = document.documentElement;
  const width = Math.max(root.clientWidth, window.innerWidth);
  const height = Math.max(root.clientHeight, window.innerHeight);
  const marked = Array.from(el.querySelectorAll<HTMLElement>(`[${GRANT_ATTR}]:not([${GRANT_ATTR}="false"])`));
  const targets: HTMLElement[] = marked.length > 0 ? marked : [el];
  return targets.some((target) => {
    const r = target.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    return pointsOf(target, r).some(([x, y]) => {
      // Past the window's edge nothing can be seen to be clicked.
      if (x < 0 || y < 0 || x >= width || y >= height) return true;
      const hit = document.elementFromPoint(x, y);
      if (hit === null) return true;
      if (target.contains(hit)) return false;
      return !(hit === document.documentElement && pageInert());
    });
  });
}

/** Whether any of the element is in view, by an IntersectionObserver
 * (clipped by its scroller or scrolled away counts as out). Without one (a
 * test), it's in view. Whether each control that grants is wholly visible is
 * `covered()`'s to judge, so a box taller than its scroller still works once
 * its controls are scrolled into view. */
function useInView(el: HTMLElement | null): boolean {
  const [inView, setInView] = useState(typeof IntersectionObserver === "undefined");
  useEffect(() => {
    if (el === null || typeof IntersectionObserver === "undefined") return;
    // A new element starts out of view until the observer says otherwise.
    setInView(false);
    const io = new IntersectionObserver((entries) => {
      const last = entries[entries.length - 1];
      if (last) setInView(last.isIntersecting);
    });
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
    const began = pressBegan();
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
