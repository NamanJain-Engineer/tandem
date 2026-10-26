import { Component, useEffect, useMemo, useReducer, useState, type ReactNode } from "react";
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

/** A ref for a list of cards, and a count that moves whenever the list,
 * any of its direct children (the header, the groups) or any card changes
 * size: a card growing or shrinking moves the ones below it, and a panel
 * shown from `display: none` or a full-view toggle resizes it. Feed it into
 * a settle key. `cards` re-observes when the set of cards changes. */
export function useLayoutChanges(cards: string): [(el: HTMLElement | null) => void, number] {
  const [el, setEl] = useState<HTMLElement | null>(null);
  const [changes, changed] = useReducer((n: number) => n + 1, 0);
  useEffect(() => {
    if (el === null || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => changed());
    ro.observe(el);
    for (const child of Array.from(el.children)) ro.observe(child);
    el.querySelectorAll('[data-testid="agent-card"]').forEach((card) => ro.observe(card));
    return () => ro.disconnect();
  }, [el, cards]);
  return [setEl, changes];
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
