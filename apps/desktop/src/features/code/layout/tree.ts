// The pane layout of a tab (F-CD-02, ARCHITECTURE §3.2): a binary split tree
// whose leaves are panes. Pure functions only; the UI store owns the tree and
// Rust validates and persists it through `layout_save`.

export type Dir = "row" | "col";
export type Side = "left" | "right" | "top" | "bottom";

export type LayoutNode =
  { type: "leaf"; paneId: string } | { type: "split"; dir: Dir; ratio: number; a: LayoutNode; b: LayoutNode };

export type Rect = { x: number; y: number; w: number; h: number };

export const MIN_RATIO = 0.05;
export const MAX_RATIO = 0.95;
export const MAX_DEPTH = 16;
/** Smallest pane a split may create (F-CD-02: 240 × 120 px). */
export const MIN_PANE = { w: 240, h: 120 };

export function leaf(paneId: string): LayoutNode {
  return { type: "leaf", paneId };
}

export function clampRatio(r: number): number {
  if (!Number.isFinite(r)) return 0.5;
  return Math.min(MAX_RATIO, Math.max(MIN_RATIO, r));
}

/** Pane ids in reading order (left-to-right, top-to-bottom). */
export function leaves(node: LayoutNode | null): string[] {
  if (!node) return [];
  return node.type === "leaf" ? [node.paneId] : [...leaves(node.a), ...leaves(node.b)];
}

export function depth(node: LayoutNode | null): number {
  if (!node) return 0;
  return node.type === "leaf" ? 1 : 1 + Math.max(depth(node.a), depth(node.b));
}

export function contains(node: LayoutNode | null, paneId: string): boolean {
  return leaves(node).includes(paneId);
}

function map(node: LayoutNode, f: (n: LayoutNode) => LayoutNode | null): LayoutNode | null {
  const mapped = f(node);
  if (mapped !== node) return mapped;
  if (node.type === "leaf") return node;
  const a = map(node.a, f);
  const b = map(node.b, f);
  if (a === node.a && b === node.b) return node;
  if (!a) return b;
  if (!b) return a;
  return { ...node, a, b };
}

/** Splits `target` and puts `newPane` on `side` of it. */
export function split(
  tree: LayoutNode,
  target: string,
  newPane: string,
  side: Side,
  ratio = 0.5,
): LayoutNode {
  if (contains(tree, newPane)) throw new Error("pane is already in the layout");
  if (!contains(tree, target)) throw new Error("no such pane");
  const dir: Dir = side === "left" || side === "right" ? "row" : "col";
  const first = side === "left" || side === "top";
  return (
    map(tree, (n) => {
      if (n.type !== "leaf" || n.paneId !== target) return n;
      const fresh = leaf(newPane);
      return { type: "split", dir, ratio: clampRatio(ratio), a: first ? fresh : n, b: first ? n : fresh };
    }) ?? tree
  );
}

/** Removes a pane; its sibling takes the parent's place. Null when empty. */
export function remove(tree: LayoutNode, paneId: string): LayoutNode | null {
  return map(tree, (n) => (n.type === "leaf" && n.paneId === paneId ? null : n));
}

/** Swaps two panes' positions (their processes don't move). */
export function swap(tree: LayoutNode, a: string, b: string): LayoutNode {
  if (a === b || !contains(tree, a) || !contains(tree, b)) return tree;
  return (
    map(tree, (n) => {
      if (n.type !== "leaf") return n;
      if (n.paneId === a) return leaf(b);
      if (n.paneId === b) return leaf(a);
      return n;
    }) ?? tree
  );
}

/** Moves `paneId` next to `target` on `side` (drag-to-split). */
export function move(tree: LayoutNode, paneId: string, target: string, side: Side): LayoutNode {
  if (paneId === target || !contains(tree, paneId) || !contains(tree, target)) return tree;
  const without = remove(tree, paneId);
  return without ? split(without, target, paneId, side) : tree;
}

/** The node at `path` ("a"/"b" steps from the root). */
export function at(tree: LayoutNode, path: string): LayoutNode | null {
  let n: LayoutNode = tree;
  for (const step of path) {
    if (n.type !== "split") return null;
    n = step === "a" ? n.a : n.b;
  }
  return n;
}

/** Sets the ratio of the split at `path` (a divider drag). */
export function setRatio(tree: LayoutNode, path: string, ratio: number): LayoutNode {
  const go = (n: LayoutNode, rest: string): LayoutNode => {
    if (n.type !== "split") return n;
    if (rest === "") return { ...n, ratio: clampRatio(ratio) };
    return rest[0] === "a" ? { ...n, a: go(n.a, rest.slice(1)) } : { ...n, b: go(n.b, rest.slice(1)) };
  };
  return go(tree, path);
}

/** Every split at 0.5 (double-clicking a gutter equalizes, UI_SPEC §2.21). */
export function equalize(tree: LayoutNode): LayoutNode {
  return tree.type === "leaf" ? tree : { ...tree, ratio: 0.5, a: equalize(tree.a), b: equalize(tree.b) };
}

/** Equalizes the run of same-direction splits at `path`, so every pane in
 * that row (or column) gets the same share (double-clicking a gutter). */
export function equalizeAt(tree: LayoutNode, path: string): LayoutNode {
  const target = at(tree, path);
  if (!target || target.type !== "split") return tree;
  const dir = target.dir;
  const count = (n: LayoutNode): number =>
    n.type === "split" && n.dir === dir ? count(n.a) + count(n.b) : 1;
  const balance = (n: LayoutNode): LayoutNode => {
    if (n.type !== "split" || n.dir !== dir) return n;
    const a = count(n.a);
    return { ...n, ratio: clampRatio(a / (a + count(n.b))), a: balance(n.a), b: balance(n.b) };
  };
  const go = (n: LayoutNode, rest: string): LayoutNode => {
    if (rest === "") return balance(n);
    if (n.type !== "split") return n;
    return rest[0] === "a" ? { ...n, a: go(n.a, rest.slice(1)) } : { ...n, b: go(n.b, rest.slice(1)) };
  };
  return go(tree, path);
}

/** The smallest box a subtree fits in, panes at `MIN_PANE` and `gutter` px between them. */
export function minExtent(node: LayoutNode, gutter = 0): { w: number; h: number } {
  if (node.type === "leaf") return { ...MIN_PANE };
  const a = minExtent(node.a, gutter);
  const b = minExtent(node.b, gutter);
  return node.dir === "row"
    ? { w: a.w + b.w + gutter, h: Math.max(a.h, b.h) }
    : { w: Math.max(a.w, b.w), h: a.h + b.h + gutter };
}

/** Pane rectangles for a container (gutters ignored). */
export function rects(tree: LayoutNode, box: Rect): Map<string, Rect> {
  const out = new Map<string, Rect>();
  const walk = (n: LayoutNode, r: Rect) => {
    if (n.type === "leaf") {
      out.set(n.paneId, r);
      return;
    }
    if (n.dir === "row") {
      const w = r.w * n.ratio;
      walk(n.a, { x: r.x, y: r.y, w, h: r.h });
      walk(n.b, { x: r.x + w, y: r.y, w: r.w - w, h: r.h });
    } else {
      const h = r.h * n.ratio;
      walk(n.a, { x: r.x, y: r.y, w: r.w, h });
      walk(n.b, { x: r.x, y: r.y + h, w: r.w, h: r.h - h });
    }
  };
  walk(tree, box);
  return out;
}

/** Whether splitting `target` on `side` keeps both halves ≥ the minimum. */
export function canSplit(tree: LayoutNode, target: string, side: Side, box: Rect): boolean {
  const r = rects(tree, box).get(target);
  if (!r || depth(tree) >= MAX_DEPTH) return false;
  return side === "left" || side === "right" ? r.w / 2 >= MIN_PANE.w : r.h / 2 >= MIN_PANE.h;
}

/** The pane spatially next to `from` in a direction (Mod+Alt+Arrow). */
export function neighbor(
  tree: LayoutNode,
  from: string,
  direction: Side,
  box: Rect = { x: 0, y: 0, w: 1000, h: 1000 },
): string | null {
  const all = rects(tree, box);
  const r = all.get(from);
  if (!r) return null;
  const cx = r.x + r.w / 2;
  const cy = r.y + r.h / 2;
  let best: { id: string; score: number } | null = null;
  for (const [id, o] of all) {
    if (id === from) continue;
    const ox = o.x + o.w / 2;
    const oy = o.y + o.h / 2;
    const ahead =
      direction === "left"
        ? o.x + o.w <= r.x + 0.5
        : direction === "right"
          ? o.x >= r.x + r.w - 0.5
          : direction === "top"
            ? o.y + o.h <= r.y + 0.5
            : o.y >= r.y + r.h - 0.5;
    if (!ahead) continue;
    const overlap =
      direction === "left" || direction === "right"
        ? Math.min(r.y + r.h, o.y + o.h) - Math.max(r.y, o.y)
        : Math.min(r.x + r.w, o.x + o.w) - Math.max(r.x, o.x);
    const distance = Math.abs(ox - cx) + Math.abs(oy - cy);
    // Prefer panes that share an edge span, then the nearest.
    const score = (overlap > 0 ? 0 : 1_000_000) + distance;
    if (!best || score < best.score) best = { id, score };
  }
  return best?.id ?? null;
}

/** Arrange presets (UI_SPEC §2.21). */
export type Preset = "1" | "2c" | "2r" | "2x2" | "1+2" | "3c" | "3x2" | "4x2";

function rowOf(ids: string[], dir: Dir): LayoutNode {
  if (ids.length === 1) return leaf(ids[0] ?? "");
  const [first, ...rest] = ids;
  return { type: "split", dir, ratio: 1 / ids.length, a: leaf(first ?? ""), b: rowOf(rest, dir) };
}

function grid(ids: string[], cols: number): LayoutNode {
  const rows: string[][] = [];
  for (let i = 0; i < ids.length; i += cols) rows.push(ids.slice(i, i + cols));
  const rowNodes = rows.map((r) => rowOf(r, "row"));
  const stack = (nodes: LayoutNode[]): LayoutNode =>
    nodes.length === 1
      ? (nodes[0] as LayoutNode)
      : {
          type: "split",
          dir: "col",
          ratio: 1 / nodes.length,
          a: nodes[0] as LayoutNode,
          b: stack(nodes.slice(1)),
        };
  return stack(rowNodes);
}

/** The preset that fits `n` panes ("Arrange" picks the nearest). */
export function presetFor(n: number): Preset {
  if (n <= 1) return "1";
  if (n === 2) return "2c";
  if (n === 3) return "1+2";
  if (n === 4) return "2x2";
  if (n <= 6) return "3x2";
  return "4x2";
}

/** Lays `ids` out by a preset; panes beyond the preset's slots fill more rows. */
export function arrange(ids: string[], preset: Preset = presetFor(ids.length)): LayoutNode | null {
  if (ids.length === 0) return null;
  if (ids.length === 1) return leaf(ids[0] ?? "");
  switch (preset) {
    case "2r":
      return rowOf(ids, "col");
    case "1+2": {
      const [main, ...rest] = ids;
      return { type: "split", dir: "row", ratio: 0.5, a: leaf(main ?? ""), b: rowOf(rest, "col") };
    }
    case "2x2":
      return grid(ids, 2);
    case "3c":
      return rowOf(ids, "row");
    case "3x2":
      return grid(ids, 3);
    case "4x2":
      return grid(ids, 4);
    case "2c":
    case "1":
    default:
      return rowOf(ids, "row");
  }
}

/** Throws unless the tree is well formed: unique leaves, ratios in range,
 * depth ≤ 16. Rust applies the same rules on `layout_save`. */
export function validate(tree: LayoutNode): LayoutNode {
  const ids = leaves(tree);
  if (new Set(ids).size !== ids.length) throw new Error("duplicate pane in layout");
  if (depth(tree) > MAX_DEPTH) throw new Error("layout is too deep");
  const check = (n: LayoutNode) => {
    if (n.type === "leaf") {
      if (typeof n.paneId !== "string" || n.paneId === "") throw new Error("bad pane id");
      return;
    }
    if (n.dir !== "row" && n.dir !== "col") throw new Error("bad split direction");
    if (!(n.ratio >= MIN_RATIO && n.ratio <= MAX_RATIO)) throw new Error("ratio out of range");
    check(n.a);
    check(n.b);
  };
  check(tree);
  return tree;
}

export function serialize(tree: LayoutNode): string {
  return JSON.stringify(tree);
}

export function deserialize(json: string): LayoutNode {
  const parsed: unknown = JSON.parse(json);
  const read = (v: unknown): LayoutNode => {
    if (!v || typeof v !== "object") throw new Error("bad layout node");
    const o = v as Record<string, unknown>;
    if (o.type === "leaf") return leaf(String(o.paneId ?? ""));
    if (o.type === "split")
      return { type: "split", dir: o.dir as Dir, ratio: Number(o.ratio), a: read(o.a), b: read(o.b) };
    throw new Error("bad layout node");
  };
  return validate(read(parsed));
}
