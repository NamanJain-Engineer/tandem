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

/** How long all of the end marker must stay in view for the line's end to
 * count as seen. An animated jump shows the line for a frame or two on its
 * way past, which nobody can read: End or Page Down where scrolling is
 * smooth (WebView2's default), a wheel notch, a fast scrollbar drag (the
 * review of 8f35e5d0, S-1). */
export const DWELL_MS = 400;

/** A ref for a marker over the command's last line ([`RunLine`]'s
 * `endRef`), and whether all of that marker has stayed in view (not clipped
 * by the pane or scrolled away) for {@link DWELL_MS} since `key` changed:
 * Allow waits for the person to have seen the command's end.
 *
 * The third ref is for the scroller that holds the line and, below it, the
 * buttons (the pane's banner), and the fourth value says whether the view
 * has gone past the marker, so Allow can say to scroll back up. Only the
 * marker on screen counts: a scroll that reached the banner's end counted
 * too, and in a pane too short for the end and the buttons together the
 * end was never on screen (the review of ce0d5815, SF-1). In such a pane
 * the command's end is above the view at the banner's end, and Allow says
 * to scroll up a little (F-TL-12 AC 20). Each request starts at the top, so
 * a scroll left at the last one's end can't stand for this one's. */
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
    // The dwell starts when all of the marker comes into view and stops at
    // any report that it no longer is: one report used to be enough.
    let dwell: ReturnType<typeof setTimeout> | null = null;
    const io = new IntersectionObserver(
      (entries) => {
        // One target: a batch is its crossings in order, and the last one
        // is where it is now.
        const latest = entries[entries.length - 1];
        if (latest === undefined) return;
        if (latest.isIntersecting && latest.intersectionRatio >= WHOLE) {
          // Still whole (1 to 0.995 and back) keeps the dwell running.
          dwell ??= setTimeout(() => setSeen(change), DWELL_MS);
        } else if (dwell !== null) {
          clearTimeout(dwell);
          dwell = null;
        }
      },
      // Told as it crosses WHOLE too: rounding can keep a ratio just short of 1.
      { threshold: [0, WHOLE, 1] },
    );
    io.observe(el);
    return () => {
      if (dwell !== null) clearTimeout(dwell);
      io.disconnect();
    };
  }, [key, el, change]);
  useEffect(() => {
    if (key === null || scroller === null) return;
    scroller.scrollTop = 0;
    setPassed(false);
    const scrolled = () => {
      if (el === null) return;
      // Above the view, wholly or in part: a jump to the end (End, Page
      // Down, Tab to a button, the scrollbar) went past it. Only wholly,
      // and a marker cut by the view's top said to scroll down (the review
      // of b3ff3696, N-1).
      setPassed(el.getBoundingClientRect().top < scroller.getBoundingClientRect().top);
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
