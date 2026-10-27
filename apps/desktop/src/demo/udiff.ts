/** The lines of a text, without the empty string after a final newline. */
export function linesOf(text: string): string[] {
  if (text === "") return [];
  const lines = text.split("\n");
  if (lines.at(-1) === "") lines.pop();
  return lines;
}

/** One line of an edit, with its line numbers: `a` in the old text and `b`
 * in the new (for a removal, `b` is the new line it comes before; for an
 * addition, `a` is the old line it comes after). */
type Op = { t: " " | "-" | "+"; line: string; a: number; b: number };

/** A shortest edit from `a` to `b`, line by line (a longest common
 * subsequence), each run's removals before its additions, as git shows them. */
function ops(a: string[], b: string[]): Op[] {
  const n = a.length;
  const m = b.length;
  // lcs[i][j]: the longest common subsequence of a[i..] and b[j..].
  const lcs = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      lcs[i]![j] = a[i] === b[j] ? lcs[i + 1]![j + 1]! + 1 : Math.max(lcs[i + 1]![j]!, lcs[i]![j + 1]!);
  const out: Op[] = [];
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) {
      out.push({ t: " ", line: a[i]!, a: i + 1, b: j + 1 });
      i++;
      j++;
    } else if (j >= m || (i < n && lcs[i + 1]![j]! >= lcs[i]![j + 1]!)) {
      out.push({ t: "-", line: a[i]!, a: i + 1, b: j });
      i++;
    } else {
      out.push({ t: "+", line: b[j]!, a: i, b: j + 1 });
      j++;
    }
  }
  return out;
}

const CONTEXT = 3;

/** `@@ -a,n +b,m @@`'s halves, as git writes them. */
function range(first: number, count: number, before: number): string {
  if (count === 0) return `${before},0`;
  return count === 1 ? `${first}` : `${first},${count}`;
}

/** `before` → `after` as a unified diff with git's header and three lines
 * of context, and the lines it adds and removes. A null `before` is a new
 * file. */
export function unifiedDiff(
  path: string,
  before: string | null,
  after: string,
): { patch: string; additions: number; deletions: number } {
  const all = ops(linesOf(before ?? ""), linesOf(after));
  const body: string[] = [];
  let k = 0;
  while (k < all.length) {
    if (all[k]!.t === " ") {
      k++;
      continue;
    }
    // One hunk: CONTEXT lines either side of a run of changes, joined with
    // the next run when no more than 2 × CONTEXT unchanged lines part them.
    let last = k;
    for (let x = k + 1; x < all.length && x - last <= 2 * CONTEXT + 1; x++) if (all[x]!.t !== " ") last = x;
    const start = Math.max(0, k - CONTEXT);
    const stop = Math.min(all.length, last + CONTEXT + 1);
    const hunk = all.slice(start, stop);
    const old = hunk.filter((o) => o.t !== "+");
    const now = hunk.filter((o) => o.t !== "-");
    body.push(
      `@@ -${range(old[0]?.a ?? 0, old.length, hunk[0]!.a)} +${range(now[0]?.b ?? 0, now.length, hunk[0]!.b)} @@`,
      ...hunk.map((o) => `${o.t}${o.line}`),
    );
    k = stop;
  }
  const head =
    before === null
      ? [`diff --git a/${path} b/${path}`, "new file mode 100644", "--- /dev/null", `+++ b/${path}`]
      : [`diff --git a/${path} b/${path}`, `--- a/${path}`, `+++ b/${path}`];
  return {
    patch: [...head, ...body, ""].join("\n"),
    additions: all.filter((o) => o.t === "+").length,
    deletions: all.filter((o) => o.t === "-").length,
  };
}
