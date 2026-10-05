import fc from "fast-check";
import { describe, expect, it } from "vitest";
import {
  arrange,
  canSplit,
  depth,
  deserialize,
  equalize,
  leaf,
  leaves,
  move,
  neighbor,
  presetFor,
  rects,
  remove,
  serialize,
  setRatio,
  split,
  swap,
  validate,
  type LayoutNode,
  type Side,
} from "./tree";

const SIDES: Side[] = ["left", "right", "top", "bottom"];
type Op = { kind: "split"; target: number; side: Side; ratio: number } | { kind: "remove"; target: number } | { kind: "swap"; a: number; b: number } | { kind: "move"; a: number; b: number; side: Side };

const opArb: fc.Arbitrary<Op> = fc.oneof(
  fc.record({ kind: fc.constant("split" as const), target: fc.nat(40), side: fc.constantFrom(...SIDES), ratio: fc.double({ min: 0, max: 1, noNaN: true }) }),
  fc.record({ kind: fc.constant("remove" as const), target: fc.nat(40) }),
  fc.record({ kind: fc.constant("swap" as const), a: fc.nat(40), b: fc.nat(40) }),
  fc.record({ kind: fc.constant("move" as const), a: fc.nat(40), b: fc.nat(40), side: fc.constantFrom(...SIDES) }),
);

/** Applies random operations, tracking the expected set of panes. */
function run(ops: Op[]): { tree: LayoutNode | null; expected: Set<string> } {
  let tree: LayoutNode | null = leaf("p0");
  const expected = new Set(["p0"]);
  let next = 1;
  for (const op of ops) {
    if (!tree) {
      tree = leaf(`p${next}`);
      expected.add(`p${next++}`);
      continue;
    }
    const ids = leaves(tree);
    const pick = (i: number) => ids[i % ids.length] ?? "";
    if (op.kind === "split" && depth(tree) < 15) {
      const id = `p${next++}`;
      tree = split(tree, pick(op.target), id, op.side, op.ratio);
      expected.add(id);
    } else if (op.kind === "remove") {
      const id = pick(op.target);
      tree = remove(tree, id);
      expected.delete(id);
    } else if (op.kind === "swap") {
      tree = swap(tree, pick(op.a), pick(op.b));
    } else if (op.kind === "move" && depth(tree) < 15) {
      tree = move(tree, pick(op.a), pick(op.b), op.side);
    }
  }
  return { tree, expected };
}

describe("layout tree (F-CD-02)", () => {
  it("F-CD-02 random edits keep exactly the expected panes, and the layout round-trips", () => {
    fc.assert(
      fc.property(fc.array(opArb, { maxLength: 60 }), (ops) => {
        const { tree, expected } = run(ops);
        expect(new Set(leaves(tree))).toEqual(expected);
        expect(leaves(tree)).toHaveLength(expected.size);
        if (tree) {
          validate(tree);
          expect(deserialize(serialize(tree))).toEqual(tree);
        }
      }),
      { numRuns: 300 },
    );
  });

  it("F-CD-02 pane rectangles tile the container exactly", () => {
    fc.assert(
      fc.property(fc.array(opArb, { maxLength: 40 }), (ops) => {
        const { tree } = run(ops);
        if (!tree) return;
        const box = { x: 0, y: 0, w: 1600, h: 900 };
        const area = [...rects(tree, box).values()].reduce((s, r) => s + r.w * r.h, 0);
        expect(Math.abs(area - box.w * box.h)).toBeLessThan(1e-6 * box.w * box.h);
      }),
      { numRuns: 200 },
    );
  });

  it("split places the new pane on the requested side", () => {
    const t = split(leaf("a"), "a", "b", "left");
    expect(leaves(t)).toEqual(["b", "a"]);
    expect(t).toMatchObject({ type: "split", dir: "row" });
    const t2 = split(t, "a", "c", "bottom");
    expect(leaves(t2)).toEqual(["b", "a", "c"]);
    expect(() => split(t2, "a", "b", "right")).toThrow();
  });

  it("swap keeps the shape and exchanges the panes", () => {
    const t = split(split(leaf("a"), "a", "b", "right"), "b", "c", "bottom");
    const s = swap(t, "a", "c");
    expect(leaves(s)).toEqual(["c", "b", "a"]);
    expect(depth(s)).toBe(depth(t));
  });

  it("ratios are clamped and equalize resets them", () => {
    const t = split(leaf("a"), "a", "b", "right", 0.99);
    expect(t).toMatchObject({ ratio: 0.95 });
    expect(setRatio(t, "", 0.01)).toMatchObject({ ratio: 0.05 });
    expect(equalize(setRatio(t, "", 0.3))).toMatchObject({ ratio: 0.5 });
  });

  it("refuses splits below 240 × 120", () => {
    const t = leaf("a");
    expect(canSplit(t, "a", "right", { x: 0, y: 0, w: 479, h: 900 })).toBe(false);
    expect(canSplit(t, "a", "right", { x: 0, y: 0, w: 480, h: 900 })).toBe(true);
    expect(canSplit(t, "a", "bottom", { x: 0, y: 0, w: 900, h: 239 })).toBe(false);
  });

  it("finds spatial neighbours for Mod+Alt+Arrow", () => {
    // a | b
    // --+--
    // c | d
    const t = arrange(["a", "b", "c", "d"], "2x2");
    if (!t) throw new Error("no tree");
    expect(neighbor(t, "a", "right")).toBe("b");
    expect(neighbor(t, "a", "bottom")).toBe("c");
    expect(neighbor(t, "d", "left")).toBe("c");
    expect(neighbor(t, "d", "top")).toBe("b");
    expect(neighbor(t, "a", "left")).toBeNull();
  });

  it("arrange presets give equal panes", () => {
    for (const n of [1, 2, 3, 4, 5, 6, 8, 16]) {
      const ids = Array.from({ length: n }, (_, i) => `p${i}`);
      const t = arrange(ids, presetFor(n));
      expect(leaves(t)).toEqual(ids);
    }
    const t = arrange(["a", "b", "c", "d"], "2x2");
    if (!t) throw new Error("no tree");
    const sizes = [...rects(t, { x: 0, y: 0, w: 1000, h: 1000 }).values()].map((r) => Math.round(r.w * r.h));
    expect(new Set(sizes)).toEqual(new Set([250000]));
  });

  it("rejects malformed layouts", () => {
    expect(() => deserialize('{"type":"split","dir":"row","ratio":2,"a":{"type":"leaf","paneId":"a"},"b":{"type":"leaf","paneId":"b"}}')).toThrow();
    expect(() => deserialize('{"type":"split","dir":"row","ratio":0.5,"a":{"type":"leaf","paneId":"a"},"b":{"type":"leaf","paneId":"a"}}')).toThrow();
    expect(() => deserialize('{"type":"pane"}')).toThrow();
  });
});
