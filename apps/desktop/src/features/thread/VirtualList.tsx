import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";

/** Below this many rows everything renders (no spacers). */
export const VIRTUAL_FROM = 80;
const OVERSCAN_PX = 800;
/** Within this of the bottom counts as "at the bottom". */
const STICK_PX = 32;

export type VirtualListProps<T> = {
  items: T[];
  getKey: (item: T) => string;
  /** Height guess for rows not measured yet. */
  estimate: number;
  render: (item: T) => ReactNode;
  /** Called when the top is near (load older pages). */
  onNearTop?: () => void;
  /** Rendered over the list when new rows arrive while scrolled up. */
  newActivity?: (jump: () => void) => ReactNode;
  ariaLabel: string;
  className?: string;
};

/** First index whose bottom is below `y`. */
function find(offsets: number[], y: number): number {
  let lo = 0;
  let hi = offsets.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((offsets[mid + 1] ?? Infinity) <= y) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** A list that renders only what's near the viewport (UI-21: 2,000 items at
 * 60 fps), sticks to the bottom while it's there, and keeps its place when
 * rows are added above. */
export function VirtualList<T>({ items, getKey, estimate, render, onNearTop, newActivity, ariaLabel, className }: VirtualListProps<T>) {
  const scroller = useRef<HTMLDivElement>(null);
  const heights = useRef(new Map<string, number>());
  const [version, setVersion] = useState(0);
  const [view, setView] = useState({ top: 0, height: 0 });
  const atBottom = useRef(true);
  const [behind, setBehind] = useState(false);
  const firstKey = useRef<string | null>(null);
  const pending = useRef(0);

  const keys = useMemo(() => items.map(getKey), [items, getKey]);
  const offsets = useMemo(() => {
    const out = new Array<number>(keys.length + 1);
    out[0] = 0;
    for (let i = 0; i < keys.length; i++) out[i + 1] = (out[i] ?? 0) + (heights.current.get(keys[i] ?? "") ?? estimate);
    return out;
    // `version` changes when measurements do.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [keys, estimate, version]);

  const virtual = items.length >= VIRTUAL_FROM;
  const total = offsets[offsets.length - 1] ?? 0;
  const start = virtual ? Math.max(0, find(offsets, view.top - OVERSCAN_PX)) : 0;
  const end = virtual ? Math.min(items.length, find(offsets, view.top + view.height + OVERSCAN_PX) + 1) : items.length;

  const measure = useCallback((key: string, el: HTMLElement | null) => {
    if (!el) return;
    const h = el.getBoundingClientRect().height;
    if (h > 0 && heights.current.get(key) !== h) {
      heights.current.set(key, h);
      if (!pending.current) {
        pending.current = requestAnimationFrame(() => {
          pending.current = 0;
          setVersion((v) => v + 1);
        });
      }
    }
  }, []);

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight <= STICK_PX;
    if (atBottom.current) setBehind(false);
    setView({ top: el.scrollTop, height: el.clientHeight });
    if (el.scrollTop < OVERSCAN_PX) onNearTop?.();
  };

  // Rows added above (an older page): keep the first old row where it was.
  useLayoutEffect(() => {
    const el = scroller.current;
    const prev = firstKey.current;
    firstKey.current = keys[0] ?? null;
    if (!el || !prev || prev === keys[0]) return;
    const at = keys.indexOf(prev);
    if (at > 0) el.scrollTop += offsets[at] ?? 0;
    // Only on a prepend.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [keys]);

  // Stick to the bottom while there; otherwise offer the jump.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    if (atBottom.current) el.scrollTop = el.scrollHeight;
    else setBehind(true);
  }, [items, version]);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    setView({ top: el.scrollTop, height: el.clientHeight });
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => setView({ top: el.scrollTop, height: el.clientHeight }));
    ro?.observe(el);
    return () => {
      ro?.disconnect();
      if (pending.current) cancelAnimationFrame(pending.current);
    };
  }, []);

  const jump = () => {
    const el = scroller.current;
    if (!el) return;
    atBottom.current = true;
    setBehind(false);
    el.scrollTop = el.scrollHeight;
  };

  return (
    <div className="relative min-h-0 flex-1">
      <div
        ref={scroller}
        role="log"
        aria-label={ariaLabel}
        aria-live="off"
        tabIndex={0}
        onScroll={onScroll}
        onKeyDown={(e) => {
          if (e.key === "End") jump();
        }}
        className={className ?? "h-full overflow-y-auto focus-visible:outline-none"}
      >
        {virtual && <div aria-hidden="true" style={{ height: offsets[start] ?? 0 }} />}
        {items.slice(start, end).map((item, n) => {
          const key = keys[start + n] ?? String(start + n);
          return (
            <div key={key} ref={(el) => measure(key, el)} data-row-key={key}>
              {render(item)}
            </div>
          );
        })}
        {virtual && <div aria-hidden="true" style={{ height: Math.max(0, total - (offsets[end] ?? total)) }} />}
      </div>
      {behind && newActivity?.(jump)}
    </div>
  );
}
