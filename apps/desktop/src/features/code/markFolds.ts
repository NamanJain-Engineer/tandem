import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import { flushSync } from "react-dom";

/** What a worktree's rail row folds while its contents are wider than the
 * row, in order (D-2700, D-2701): the preview's word (it says least), the
 * radar mark's symbol, the checks badge's words, then the status words to
 * their dot. */
export const FOLDS = ["preview", "radar", "checks", "status"] as const;
export type Fold = (typeof FOLDS)[number];

/** Whether `part` is folded once a row has folded `folds` parts. */
export function folded(folds: number, part: Fold): boolean {
  return folds > FOLDS.indexOf(part);
}

/** D-2700, D-2701: a rail row's parts show their words whole or not at all,
 * and the row never runs past the rail. While the row's contents are wider
 * than the row, one more part folds, up to `max`. `contents` names what the
 * row shows (its name, status, marks' words and count); when it changes, or
 * the row's width does, nothing is folded and the row is measured again, all
 * before it's drawn. `row` finds the row from the element `ref` is on, for a
 * part that folds inside a row it doesn't draw (a workspace row's status). */
export function useMarkFolds<T extends HTMLElement = HTMLDivElement>(
  contents: string,
  max: number = FOLDS.length,
  row?: (el: T) => HTMLElement | null,
): {
  ref: RefObject<T | null>;
  folds: number;
} {
  const ref = useRef<T | null>(null);
  const [folds, setFolds] = useState(0);
  const measured = useRef(contents);
  const [round, setRound] = useState(0);
  const target = (): HTMLElement | null => (ref.current && row ? row(ref.current) : ref.current);
  // Measured when the contents, the folds or the width change, not on
  // every render: each measure is a layout.
  useLayoutEffect(() => {
    if (measured.current !== contents) {
      measured.current = contents;
      if (folds > 0) {
        setFolds(0);
        return;
      }
    }
    const el = target();
    if (el && folds < max && el.scrollWidth > el.clientWidth + 1) setFolds(folds + 1);
    // `round` is a new width, measured even when nothing was folded.
  }, [contents, folds, round, max]);
  useLayoutEffect(() => {
    const el = target();
    if (!el || typeof ResizeObserver === "undefined") return;
    let width = el.clientWidth;
    const observer = new ResizeObserver(() => {
      if (el.clientWidth === width) return;
      width = el.clientWidth;
      flushSync(() => {
        setFolds(0);
        setRound((n) => n + 1);
      });
    });
    observer.observe(el);
    return () => observer.disconnect();
    // The row is the same element for the life of the part.
  }, []);
  return { ref, folds };
}
