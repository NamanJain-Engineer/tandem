import { Component, useEffect, useMemo, useState, type ReactNode } from "react";
import { Button, SETTLE_MS, useSteadyBox } from "@tandem/ui";
import { t } from "../../i18n";

// The place-on-screen guard lives with the shared ApprovalCard, which
// needs it too; the app's guards keep importing it from here.
export { SETTLE_MS, useSteadyBox };

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

/** How much of the end marker must show at once for the line's end to
 * count as seen: all of it, give or take a rounding. */
const WHOLE = 0.99;

/** A ref for a marker over the run's last line, and whether all of that
 * marker has been in view (not clipped by the pane or scrolled away) since
 * `key` changed: Allow waits for the person to have seen the line's end.
 *
 * The third ref is for the scroller that holds the line and, below it, the
 * buttons (the pane's banner), and the fourth value says whether the view
 * has gone past the marker, so Allow can say to scroll back up. Only the
 * marker on screen counts: a scroll that reached the banner's end counted
 * too, and in a pane too short for the end and wrapped buttons together
 * the end was never on screen (the review of ce0d5815, SF-1). The banner's
 * buttons stay on one row instead, so at its end the line's last line
 * shows above them (F-TL-12 AC 20). Each request starts at the top, so a
 * scroll left at the last one's end can't stand for this one's. */
export function useSeenEnd(
  key: string | null,
): [(el: HTMLElement | null) => void, boolean, (el: HTMLElement | null) => void, boolean] {
  const change = useChange(key);
  const [el, setEl] = useState<HTMLElement | null>(null);
  const [scroller, setScroller] = useState<HTMLElement | null>(null);
  const [seen, setSeen] = useState<object | null>(null);
  const [passed, setPassed] = useState(false);
  useEffect(() => {
    if (key === null || el === null) return;
    // Every webview Vibezeo ships in has it; a test without one sees all.
    if (typeof IntersectionObserver === "undefined") {
      setSeen(change);
      return;
    }
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting && e.intersectionRatio >= WHOLE)) setSeen(change);
      },
      // Told as it crosses WHOLE too: rounding can keep a ratio just short of 1.
      { threshold: [0, WHOLE, 1] },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [key, el, change]);
  useEffect(() => {
    if (key === null || scroller === null) return;
    scroller.scrollTop = 0;
    setPassed(false);
    const scrolled = () => {
      if (el === null) return;
      // Above the view: a jump to the end (End, Page Down, the scrollbar)
      // went past it.
      setPassed(el.getBoundingClientRect().bottom <= scroller.getBoundingClientRect().top + 1);
    };
    scroller.addEventListener("scroll", scrolled, { passive: true });
    return () => scroller.removeEventListener("scroll", scrolled);
  }, [key, scroller, el, change]);
  return [setEl, key !== null && seen === change, setScroller, passed];
}

/** Why Allow is waiting, for its title: the first reason that applies.
 * `passed`: the view has gone past the line's end without showing it. */
export function waitReason(w: {
  seenEnd: boolean;
  settled: boolean;
  steady: boolean;
  passed?: boolean;
}): string | undefined {
  if (!w.seenEnd) return t(w.passed ? "approval.waitingEndAbove" : "approval.waitingEnd");
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
