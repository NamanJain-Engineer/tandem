import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cx } from "../cx";

type Side = "top" | "right" | "bottom" | "left";

export type CoachmarkProps = {
  /** The thing the hint describes; nothing shows until it's on screen. */
  anchor: Element | null;
  /** Accessible name, e.g. "Tip". */
  label: string;
  children: ReactNode;
  /** Where the hint sits: `right` of a rail row keeps the rail's content clear. */
  side?: Side;
  /** Below its anchor: centred, or from its left edge (to keep clear of
   * what's to the left, such as the rail). */
  align?: "center" | "start";
  /** One line, text and footer side by side: fits a header band. */
  compact?: boolean;
  /** Beside the text: a step count, a Close button. */
  footer?: ReactNode;
  className?: string;
};

/** Room for the notch between the hint and its anchor. */
const GAP = 10;
const EDGE = 8;
/** The notch keeps this far from the hint's corners. */
const CORNER = 14;

/** Where the hint goes, and how far along its facing edge the notch sits. */
export type CoachmarkPlace = { top: number; left: number; notch: number };

/** Where a `w`×`h` hint goes beside `r`, kept inside a `vw`×`vh` viewport,
 * with its notch pointing at the anchor's centre. */
export function placeCoachmark(
  r: DOMRect,
  w: number,
  h: number,
  side: Side,
  vw: number,
  vh: number,
  align: "center" | "start" = "center",
): CoachmarkPlace {
  const clamp = (v: number, max: number) => Math.max(EDGE, Math.min(v, max - EDGE));
  const notchAt = (centre: number, start: number, size: number) =>
    Math.max(CORNER, Math.min(centre - start, size - CORNER));
  const midX = r.left + r.width / 2;
  const midY = r.top + r.height / 2;
  if (side === "right" || side === "left") {
    const top = clamp(midY - h / 2, vh - h);
    const left = clamp(side === "right" ? r.right + GAP : r.left - GAP - w, vw - w);
    return { top, left, notch: notchAt(midY, top, h) };
  }
  const top = clamp(side === "top" ? r.top - GAP - h : r.bottom + GAP, vh - h);
  const left = clamp(side === "top" ? r.right - w : align === "start" ? r.left : midX - w / 2, vw - w);
  return { top, left, notch: notchAt(midX, left, w) };
}

/** The notch on the edge facing the anchor: a square turned 45°, bordered
 * on its two outer sides so it reads as part of the hint's outline. */
const NOTCH: Record<Side, string> = {
  right: "-left-[5px] -translate-y-1/2 border-b border-l",
  left: "-right-[5px] -translate-y-1/2 border-r border-t",
  bottom: "-top-[5px] -translate-x-1/2 border-l border-t",
  top: "-bottom-[5px] -translate-x-1/2 border-b border-r",
};

/** A small hint beside the thing it describes (DESIGN §5.2.2). Unlike a
 * popover it never takes focus, an outside click or Esc: the visitor
 * dismisses it by doing what it says. It follows its anchor as the layout
 * moves, points at it with a notch, and announces itself politely. */
export function Coachmark({
  anchor,
  label,
  children,
  side = "bottom",
  align = "center",
  compact = false,
  footer,
  className,
}: CoachmarkProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<CoachmarkPlace | null>(null);

  const measure = useCallback(() => {
    const el = ref.current;
    if (!anchor || !el) return;
    const next = placeCoachmark(
      anchor.getBoundingClientRect(),
      el.offsetWidth,
      el.offsetHeight,
      side,
      window.innerWidth,
      window.innerHeight,
      align,
    );
    setPlace((p) => (p && p.top === next.top && p.left === next.left && p.notch === next.notch ? p : next));
  }, [anchor, side, align]);

  useLayoutEffect(() => {
    if (!anchor) return;
    measure();
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    ro?.observe(anchor);
    ro?.observe(document.body);
    if (ref.current) ro?.observe(ref.current);
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => {
      ro?.disconnect();
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure, true);
    };
  }, [anchor, measure]);

  if (!anchor) return null;
  const across = side === "right" || side === "left";
  return createPortal(
    <div
      ref={ref}
      role="status"
      aria-label={label}
      data-testid="coachmark"
      data-side={side}
      style={place ? { top: place.top, left: place.left } : { top: 0, left: 0, visibility: "hidden" }}
      className={cx(
        "tn-pop fixed z-[var(--z-popover)] flex w-max rounded-md border border-border bg-surface-3 text-base text-fg shadow-e2",
        compact
          ? "max-w-[calc(100vw-1rem)] items-center gap-3 py-1.5 pl-3 pr-1.5"
          : "max-w-[240px] flex-col gap-2 p-3",
        className,
      )}
    >
      <span
        aria-hidden="true"
        data-testid="coachmark-notch"
        style={place ? (across ? { top: place.notch } : { left: place.notch }) : undefined}
        className={cx("absolute size-2.5 rotate-45 border-border bg-surface-3", NOTCH[side])}
      />
      {/* One line where there's room; a narrow window wraps it. */}
      <div className={compact ? "sm:whitespace-nowrap" : undefined}>{children}</div>
      {footer && (
        <div className={cx("flex items-center gap-3 text-sm text-fg-muted", !compact && "justify-between")}>
          {footer}
        </div>
      )}
    </div>,
    document.body,
  );
}
