// A view waiting its turn in the mount queue starts its pane's process at
// once instead of when it opens, so six agents start together rather than one
// behind another's open (F-CD-03 AC2, D-141). Its xterm doesn't exist yet, so
// the grid comes from the last view that fit: the same grid for a host of the
// same size (a tab's panes usually are), scaled for another. The view lays
// the replay out at that grid and resizes the PTY once it has fit.

/** A host's size in CSS pixels and the grid xterm fit in it. */
export type Fitted = { width: number; height: number; cols: number; rows: number };

let last: (Fitted & { font: string }) | null = null;

/** Records what a view fit, with the font (family and size) it measured. */
export function rememberFit(font: string, fitted: Fitted): void {
  last = { ...fitted, font };
}

/** The grid a host of this size would fit with this font, from the last
 * view's fit; null before any view with this font has fit. */
export function guessSize(
  font: string,
  width: number,
  height: number,
): { cols: number; rows: number } | null {
  const f = last;
  if (!f || f.font !== font) return null;
  if (width === f.width && height === f.height) return { cols: f.cols, rows: f.rows };
  const scale = (n: number, to: number, from: number) =>
    Math.max(2, from > 0 ? Math.floor((n * to) / from) : n);
  return { cols: scale(f.cols, width, f.width), rows: scale(f.rows, height, f.height) };
}
