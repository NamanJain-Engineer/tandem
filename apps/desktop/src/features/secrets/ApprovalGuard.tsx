import { Component, useEffect, useState, type ReactNode } from "react";
import { Button } from "@tandem/ui";
import { t } from "../../i18n";

/** How long Allow ignores clicks after what it would allow changed. */
export const SETTLE_MS = 500;

/** False for {@link SETTLE_MS} after `key` changes, so a click meant for
 * one approval can't land on the one that replaced it. */
export function useSettled(key: string | null): boolean {
  const [settled, setSettled] = useState<string | null>(null);
  useEffect(() => {
    if (key === null) return;
    const timer = setTimeout(() => setSettled(key), SETTLE_MS);
    return () => clearTimeout(timer);
  }, [key]);
  return key !== null && settled === key;
}

/** A ref for a marker after the run's line, and whether that marker has
 * been in view (not clipped by the pane or scrolled away) since `key`
 * changed: Allow waits for the person to have seen the line's end. */
export function useSeenEnd(key: string | null): [(el: HTMLElement | null) => void, boolean] {
  const [el, setEl] = useState<HTMLElement | null>(null);
  const [seen, setSeen] = useState<string | null>(null);
  useEffect(() => {
    if (key === null || el === null) return;
    // Every webview Vibezeo ships in has it; a test without one sees all.
    if (typeof IntersectionObserver === "undefined") {
      setSeen(key);
      return;
    }
    const io = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) setSeen(key);
    });
    io.observe(el);
    return () => io.disconnect();
  }, [key, el]);
  return [setEl, key !== null && seen === key];
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
