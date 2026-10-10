import { describe, expect, it } from "vitest";
import { addCell, columnsFor, GRID_MAX, neighbor, removeCell } from "./grid";

describe("thread grid (F-TH-14)", () => {
  it("F-TH-14.1 cells start from the open thread and stop at 6: Grid is full", () => {
    let grid: string[] = [];
    let r = addCell(grid, "t0", "t1", 6);
    expect(r).toEqual({ grid: ["t0", "t1"], result: "added" });
    grid = r.grid;
    for (let i = 2; i < 7; i++) {
      r = addCell(grid, "t0", `t${i}`, 6);
      grid = r.grid;
    }
    expect(grid).toHaveLength(GRID_MAX);
    expect(r.result).toBe("full");
    expect(addCell(grid, "t0", "t3", 6).result).toBe("present");
  });

  it("F-TH-14.1 Free (1) can't add a second cell", () => {
    expect(addCell([], "t0", "t1", 1)).toEqual({ grid: [], result: "limit" });
    // With nothing open, the first thread is still just one cell.
    expect(addCell([], null, "t1", 1)).toEqual({ grid: ["t1"], result: "added" });
    // A grid saved on Pro is still "one at a time" after a downgrade.
    const saved = ["a", "b", "c", "d", "e", "f"];
    expect(addCell(saved, "a", "g", 1).result).toBe("limit");
    expect(addCell(saved, "a", "g", 6).result).toBe("full");
  });

  it("closing a cell keeps the rest; one left is no grid, and the open thread moves over", () => {
    expect(removeCell(["a", "b", "c"], "b", "b")).toEqual({ grid: ["a", "c"], open: "c" });
    expect(removeCell(["a", "b", "c"], "a", "c")).toEqual({ grid: ["a", "b"], open: "a" });
    expect(removeCell(["a", "b"], "b", "b")).toEqual({ grid: [], open: "a" });
    expect(removeCell(["a", "b"], "a", "z")).toEqual({ grid: ["a", "b"], open: "a" });
  });

  it("columns follow the width; Mod+Alt+Arrow moves between cells", () => {
    expect(columnsFor(800, 6)).toBe(1);
    expect(columnsFor(1200, 6)).toBe(2);
    expect(columnsFor(1600, 6)).toBe(3);
    expect(columnsFor(1600, 2)).toBe(2);
    const g = ["a", "b", "c", "d", "e"];
    // a b c / d e
    expect(neighbor(g, "a", "right", 3)).toBe("b");
    expect(neighbor(g, "c", "right", 3)).toBe("c");
    expect(neighbor(g, "e", "right", 3)).toBe("e");
    expect(neighbor(g, "d", "left", 3)).toBe("d");
    expect(neighbor(g, "b", "down", 3)).toBe("e");
    expect(neighbor(g, "c", "down", 3)).toBe("c");
    expect(neighbor(g, "e", "up", 3)).toBe("b");
    expect(neighbor(g, "a", "up", 3)).toBe("a");
  });
});
