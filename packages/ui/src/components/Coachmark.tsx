import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cx } from "../cx";

export type CoachmarkProps = {
  /** The thing the hint describes; nothing shows until it's on screen. */
  anchor: Element | null;
  /** Accessible name, e.g. "Tip". */
  label: string;
  children: ReactNode;
  /** Where the hint sits: `right` of a rail row keeps the rail's content clear. */
  side?: "top" | "right" | "bottom";
  /** Below its anchor: centred, or from its left edge (to keep clear of
   * what's to the left, such as the rail). */
  align?: "center" | "start";
  /** Under the text: a step count, a Close button. */
  footer?: ReactNode;
  className?: string;
};

const GAP = 8;
const EDGE = 8;

type Place = { top: number; left: number };

/** Where a `w`×`h` hint goes beside `r`, kept inside a `vw`×`vh` viewport. */
export function placeCoachmark(
  r: DOMRect,
  w: number,
  h: number,
  side: "top" | "right" | "bottom",
  vw: number,
  vh: number,
  align: "center" | "start" = "center",
): Place {
  const clamp = (v: number, max: number) => Math.max(EDGE, Math.min(v, max - EDGE));
  if (side === "right") {
    return { left: clamp(r.right + GAP, vw - w), top: clamp(r.top + r.height / 2 - h / 2, vh - h) };
  }
  if (side === "top") {
    return { left: clamp(r.right - w, vw - w), top: clamp(r.top - GAP - h, vh - h) };
  }
  const left = align === "start" ? r.left : r.left + r.width / 2 - w / 2;
  return { left: clamp(left, vw - w), top: clamp(r.bottom + GAP, vh - h) };
}

/** A small hint beside the thing it describes (DESIGN §5.2.2). Unlike a
 * popover it never takes focus, an outside click or Esc: the visitor
 * dismisses it by doing what it says. It follows its anchor as the layout
 * moves, and announces itself politely. */
export function Coachmark({
  anchor,
  label,
  children,
  side = "bottom",
  align = "center",
  footer,
  className,
}: CoachmarkProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<Place | null>(null);

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
    setPlace((p) => (p && p.top === next.top && p.left === next.left ? p : next));
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
  return createPortal(
    <div
      ref={ref}
      role="status"
      aria-label={label}
      data-testid="coachmark"
      data-side={side}
      style={place ? { top: place.top, left: place.left } : { top: 0, left: 0, visibility: "hidden" }}
      className={cx(
        "tn-pop fixed z-[var(--z-popover)] flex w-max max-w-[260px] flex-col gap-2 rounded-md border border-border bg-surface-3 p-3 text-base text-fg shadow-e2",
        className,
      )}
    >
      <div>{children}</div>
      {footer && (
        <div className="flex items-center justify-between gap-3 text-sm text-fg-muted">{footer}</div>
      )}
    </div>,
    document.body,
  );
}
