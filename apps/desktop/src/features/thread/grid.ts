/** The thread grid (F-TH-14): up to six threads side by side. Pure, so the
 * store and its tests share it. */

export const GRID_MAX = 6;

export type GridAdd = "added" | "present" | "full" | "limit";

/** `id` added to `grid` (which starts from the open thread). */
export function addCell(
  grid: string[],
  open: string | null,
  id: string,
  limit: number,
): { grid: string[]; result: GridAdd } {
  const current = grid.length > 0 ? grid : open ? [open] : [];
  if (current.includes(id)) return { grid, result: "present" };
  // The plan's limit speaks first: on Free even a grid saved on Pro is "one at a time".
  if (limit < GRID_MAX && current.length >= limit) return { grid, result: "limit" };
  if (current.length >= GRID_MAX) return { grid, result: "full" };
  return { grid: [...current, id], result: "added" };
}

/** `id` taken out; one cell left is no grid. Returns the thread to show
 * next when `id` was the open one. */
export function removeCell(
  grid: string[],
  open: string | null,
  id: string,
): { grid: string[]; open: string | null } {
  const at = grid.indexOf(id);
  if (at < 0) return { grid, open };
  const next = grid.filter((g) => g !== id);
  const nextOpen = open === id ? (next[Math.min(at, next.length - 1)] ?? null) : open;
  return { grid: next.length > 1 ? next : [], open: nextOpen };
}

/** Columns by width: 1 below 900 px, 2 below 1400 px, else 3. */
export function columnsFor(width: number, cells: number): number {
  const fit = width < 900 ? 1 : width < 1400 ? 2 : 3;
  return Math.max(1, Math.min(fit, cells));
}

/** The cell `dir` of `id` in a grid of `columns`, or `id` at an edge. */
export function neighbor(
  grid: string[],
  id: string,
  dir: "left" | "right" | "up" | "down",
  columns: number,
): string {
  const at = grid.indexOf(id);
  if (at < 0) return grid[0] ?? id;
  const row = Math.floor(at / columns);
  const to =
    dir === "left"
      ? at % columns === 0
        ? at
        : at - 1
      : dir === "right"
        ? at % columns === columns - 1 || at + 1 >= grid.length
          ? at
          : at + 1
        : dir === "up"
          ? row === 0
            ? at
            : at - columns
          : at + columns < grid.length
            ? at + columns
            : at;
  return grid[to] ?? id;
}
