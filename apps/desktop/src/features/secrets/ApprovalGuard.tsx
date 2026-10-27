import { Component, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Button } from "@tandem/ui";
import { t } from "../../i18n";

/** How long Allow ignores clicks after what it would allow changed. */
export const SETTLE_MS = 500;

/** A new token on every change of `key`, even back to an earlier value
 * (A → B → A), so nothing settled or seen for the first A carries over. */
function useChange(key: string | null): object {
  // eslint-disable-next-line react-hooks/exhaustive-deps -- a new token per change is the point
  return useMemo(() => ({}), [key]);
}

/** False for {@link SETTLE_MS} after every change of `key`, so a click meant
 * for one approval (or one card's place) can't land on what replaced it. */
export function useSettled(key: string | null): boolean {
  const change = useChange(key);
  const [settled, setSettled] = useState<object | null>(null);
  useEffect(() => {
    if (key === null) return;
    const timer = setTimeout(() => setSettled(change), SETTLE_MS);
    return () => clearTimeout(timer);
  }, [key, change]);
  return key !== null && settled === change;
}

/** A ref for a marker after the run's line, and whether that marker has
 * been in view (not clipped by the pane or scrolled away) since `key`
 * changed: Allow waits for the person to have seen the line's end. */
export function useSeenEnd(key: string | null): [(el: HTMLElement | null) => void, boolean] {
  const change = useChange(key);
  const [el, setEl] = useState<HTMLElement | null>(null);
  const [seen, setSeen] = useState<object | null>(null);
  useEffect(() => {
    if (key === null || el === null) return;
    // Every webview Vibezeo ships in has it; a test without one sees all.
    if (typeof IntersectionObserver === "undefined") {
      setSeen(change);
      return;
    }
    const io = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) setSeen(change);
    });
    io.observe(el);
    return () => io.disconnect();
  }, [key, el, change]);
  return [setEl, key !== null && seen === change];
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

/** Something else (a menu, a dialog, a toast) covers the box's middle. */
function covered(el: HTMLElement): boolean {
  if (typeof document.elementFromPoint !== "function") return false;
  const r = el.getBoundingClientRect();
  const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
  return hit !== null && !el.contains(hit);
}

/** Whether Allow's own place on screen has held still for
 * {@link SETTLE_MS}: a ref for the element holding the buttons, `steady`
 * for their disabled state (sampled every frame), and `check()` for the
 * click itself, which measures again on the spot. Any move counts: a card
 * above growing or shrinking, a reorder, a scroll, the panel being shown or
 * resized. Each Allow waits only on its own box, so one agent's moving card
 * can't hold every other Allow disabled. */
export function useSteadyBox(active: boolean): {
  ref: (el: HTMLElement | null) => void;
  steady: boolean;
  check: () => boolean;
} {
  const [el, setEl] = useState<HTMLElement | null>(null);
  const last = useRef<{ box: string; since: number } | null>(null);
  const [steady, setSteady] = useState(false);
  const sample = useCallback((): boolean => {
    if (el === null) return false;
    const box = boxOf(el);
    const now = performance.now();
    if (last.current === null || last.current.box !== box) last.current = { box, since: now };
    return now - last.current.since >= SETTLE_MS;
  }, [el]);
  // At the click: still, and nothing (an overlay that just closed or is
  // closing) over it.
  const check = useCallback((): boolean => {
    if (el === null || hidden(el)) return false;
    return sample() && !covered(el);
  }, [el, sample]);
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

/** Why Allow is waiting, for its title: the first reason that applies. */
export function waitReason(w: { seenEnd: boolean; settled: boolean; steady: boolean }): string | undefined {
  if (!w.seenEnd) return t("approval.waitingEnd");
  if (!w.settled) return t("approval.waitingChanged");
  if (!w.steady) return t("approval.waiting");
  return undefined;
}

type BoundaryProps = { children: ReactNode; onDeny?: () => void };

/** An approval that can't be shown offers no Allow, and doesn't take the
 * rest of the window down with it. */
export class ApprovalBoundary extends Component<BoundaryProps, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  render(): ReactNode {
    if (!this.state.failed) return this.props.children;
    return (
      <div
        role="alert"
        className="flex flex-wrap items-center gap-2 text-sm text-fg"
        data-testid="approval-unreadable"
      >
        <span>{t("approval.unreadable")}</span>
        {this.props.onDeny && (
          <Button size="sm" variant="ghost" onClick={this.props.onDeny}>
            {t("dashboard.deny")}
          </Button>
        )}
      </div>
    );
  }
}
