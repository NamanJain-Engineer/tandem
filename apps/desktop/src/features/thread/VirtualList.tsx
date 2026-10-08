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

export type Range = { start: number; end: number };
/** The range before the first measurement: the last rows (lists open at the bottom). */
export const UNSET: Range = { start: Number.MAX_SAFE_INTEGER, end: Number.MAX_SAFE_INTEGER };

/** The rows to render for a viewport. The current range is kept while it
 * still covers the viewport by half the overscan; otherwise it's replaced by
 * one covering it by 1.5 overscans. Rows then mount and unmount a screenful
 * at a time instead of one per frame while scrolling. Returns `r` itself
 * when nothing changes. */
export function nextRange(r: Range, offsets: number[], top: number, height: number): Range {
  const count = offsets.length - 1;
  const need = { start: find(offsets, top - OVERSCAN_PX / 2), end: Math.min(count, find(offsets, top + height + OVERSCAN_PX / 2) + 1) };
  if (r.start <= need.start && r.end >= need.end && r.end <= count) return r;
  const start = Math.max(0, find(offsets, top - OVERSCAN_PX * 1.5));
  const end = Math.min(count, find(offsets, top + height + OVERSCAN_PX * 1.5) + 1);
  return r.start === start && r.end === end ? r : { start, end };
}

/** A list that renders only what's near the viewport (UI-21: 2,000 items at
 * 60 fps), sticks to the bottom while it's there, and keeps its place when
 * rows are added above. Rows are measured by one ResizeObserver (never
 * during render), and scrolling re-renders only when the visible range
 * changes. */
export function VirtualList<T>({ items, getKey, estimate, render, onNearTop, newActivity, ariaLabel, className }: VirtualListProps<T>) {
  const scroller = useRef<HTMLDivElement>(null);
  const heights = useRef(new Map<string, number>());
  const [version, setVersion] = useState(0);
  const viewport = useRef({ top: 0, height: 0 });
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
  const [range, setRange] = useState(UNSET);

  const computeRange = useCallback(() => {
    const { top, height } = viewport.current;
    setRange((r) => nextRange(r, offsets, top, height));
  }, [offsets]);
  const computeRef = useRef(computeRange);
  computeRef.current = computeRange;
  useLayoutEffect(() => {
    if (virtual) computeRange();
  }, [computeRange, virtual]);

  const start = virtual ? (range === UNSET ? Math.max(0, keys.length - 40) : Math.min(range.start, keys.length)) : 0;
  const end = virtual ? (range === UNSET ? keys.length : Math.min(range.end, keys.length)) : keys.length;

  // One observer for every row; heights change without a render.
  const observer = useMemo(() => {
    if (typeof ResizeObserver === "undefined") return null;
    return new ResizeObserver((entries) => {
      let changed = false;
      for (const entry of entries) {
        const key = (entry.target as HTMLElement).dataset.rowKey;
        const h = entry.borderBoxSize?.[0]?.blockSize ?? entry.contentRect.height;
        if (key && h > 0 && heights.current.get(key) !== h) {
          heights.current.set(key, h);
          changed = true;
        }
      }
      if (changed && !pending.current) {
        pending.current = requestAnimationFrame(() => {
          pending.current = 0;
          setVersion((v) => v + 1);
        });
      }
    });
  }, []);
  const refs = useRef(new Map<string, (el: HTMLDivElement | null) => void>());
  const elements = useRef(new Map<string, HTMLDivElement>());
  const refFor = (key: string) => {
    let cb = refs.current.get(key);
    if (!cb) {
      cb = (el) => {
        const old = elements.current.get(key);
        if (old && old !== el) observer?.unobserve(old);
        if (el) {
          elements.current.set(key, el);
          observer?.observe(el);
        } else {
          elements.current.delete(key);
          refs.current.delete(key);
        }
      };
      refs.current.set(key, cb);
    }
    return cb;
  };

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight <= STICK_PX;
    if (atBottom.current && behind) setBehind(false);
    viewport.current = { top: el.scrollTop, height: el.clientHeight };
    computeRef.current();
    if (el.scrollTop < OVERSCAN_PX) onNearTop?.();
  };

  // Rows added above (an older page): keep the first old row where it was.
  useLayoutEffect(() => {
    const el = scroller.current;
    const prev = firstKey.current;
    firstKey.current = keys[0] ?? null;
    if (!el || !prev || prev === keys[0]) return;
    const at = keys.indexOf(prev);
    if (at <= 0) return;
    el.scrollTop += offsets[at] ?? 0;
    // Render the rows now in view before this frame paints.
    viewport.current = { top: el.scrollTop, height: el.clientHeight };
    computeRef.current();
    // Only on a prepend.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [keys]);

  // Stick to the bottom while there (and render what's there in the same
  // pass); otherwise offer the jump.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    if (atBottom.current) {
      el.scrollTop = el.scrollHeight;
      viewport.current = { top: el.scrollTop, height: el.clientHeight };
      computeRef.current();
    } else setBehind(true);
  }, [items, version]);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    viewport.current = { top: el.scrollTop, height: el.clientHeight };
    computeRef.current();
    const ro =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(() => {
            viewport.current = { top: el.scrollTop, height: el.clientHeight };
            computeRef.current();
          });
    ro?.observe(el);
    return () => {
      ro?.disconnect();
      observer?.disconnect();
      if (pending.current) cancelAnimationFrame(pending.current);
    };
  }, [observer]);

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
            <div key={key} ref={refFor(key)} data-row-key={key}>
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
