import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import { flushSync } from "react-dom";

/** How many marks a rail row can fold, in the order they fold: the radar
 * mark (its symbol), then the checks badge (CI's state). */
export const MARK_FOLDS = 2;

/** D-2700: a rail row's marks show their words whole or not at all. While the
 * row's contents are wider than the row, one more mark folds to its icon:
 * the radar mark first, then the checks badge. `contents` names what the row
 * shows (its name, status, marks' words and count); when it changes, or the
 * row's width does (the rail is 180–360 px), nothing is folded and the row is
 * measured again, all before it's drawn. */
export function useMarkFolds(contents: string): {
  ref: RefObject<HTMLDivElement | null>;
  folds: number;
} {
  const ref = useRef<HTMLDivElement | null>(null);
  const [folds, setFolds] = useState(0);
  // A new width is measured even when nothing was folded: a render forces it.
  const [, setRound] = useState(0);
  const measured = useRef(contents);
  useLayoutEffect(() => {
    if (measured.current !== contents) {
      measured.current = contents;
      if (folds > 0) {
        setFolds(0);
        return;
      }
    }
    const row = ref.current;
    if (row && folds < MARK_FOLDS && row.scrollWidth > row.clientWidth + 1) setFolds(folds + 1);
  });
  useLayoutEffect(() => {
    const row = ref.current;
    if (!row || typeof ResizeObserver === "undefined") return;
    let width = row.clientWidth;
    const observer = new ResizeObserver(() => {
      if (row.clientWidth === width) return;
      width = row.clientWidth;
      flushSync(() => {
        setFolds(0);
        setRound((n) => n + 1);
      });
    });
    observer.observe(row);
    return () => observer.disconnect();
  }, []);
  return { ref, folds };
}
