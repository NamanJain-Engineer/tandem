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
 * subsequence), as which lines each side changed. */
function changes(a: string[], b: string[]): { a: boolean[]; b: boolean[] } {
  const n = a.length;
  const m = b.length;
  // lcs[i][j]: the longest common subsequence of a[i..] and b[j..].
  const lcs = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      lcs[i]![j] = a[i] === b[j] ? lcs[i + 1]![j + 1]! + 1 : Math.max(lcs[i + 1]![j]!, lcs[i]![j + 1]!);
  const out = { a: new Array<boolean>(n).fill(false), b: new Array<boolean>(m).fill(false) };
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) {
      i++;
      j++;
    } else if (j >= m || (i < n && lcs[i + 1]![j]! >= lcs[i]![j + 1]!)) out.a[i++] = true;
    else out.b[j++] = true;
  }
  return out;
}

// Where a change sits when it could sit in several places (the same lines
// before and after it), as git's xdiff puts it (xdiffi.c, `xdl_change_compact`
// and its indent heuristic): a group of changes slides as far down as it can,
// merging with any it meets; then it moves back up to line up with a change
// on the other side, or else to where the lines around it read best.

/** One side of a diff: its lines, and which of them changed. */
type Side = { recs: string[]; chg: boolean[] };
/** A run of changed lines [start, end); an empty one sits above line `start`. */
type Group = { start: number; end: number };

const changed = (f: Side, i: number) => i >= 0 && i < f.recs.length && f.chg[i] === true;

function groupInit(f: Side): Group {
  let end = 0;
  while (changed(f, end)) end++;
  return { start: 0, end };
}

/** The next group; false at the end. */
function groupNext(f: Side, g: Group): boolean {
  if (g.end === f.recs.length) return false;
  g.start = g.end + 1;
  for (g.end = g.start; changed(f, g.end); g.end++);
  return true;
}

/** The group before; false at the start. */
function groupPrevious(f: Side, g: Group): boolean {
  if (g.start === 0) return false;
  g.end = g.start - 1;
  for (g.start = g.end; changed(f, g.start - 1); g.start--);
  return true;
}

/** Moves the group a line down when the line after it is its first line's
 * twin, joining any group it then touches; false when it can't. */
function slideDown(f: Side, g: Group): boolean {
  if (g.end >= f.recs.length || f.recs[g.start] !== f.recs[g.end]) return false;
  f.chg[g.start++] = false;
  f.chg[g.end++] = true;
  while (changed(f, g.end)) g.end++;
  return true;
}

/** Moves the group a line up, as `slideDown` moves it down. */
function slideUp(f: Side, g: Group): boolean {
  if (g.start <= 0 || f.recs[g.start - 1] !== f.recs[g.end - 1]) return false;
  f.chg[--g.start] = true;
  f.chg[--g.end] = false;
  while (changed(f, g.start - 1)) g.start--;
  return true;
}

/** C's `isspace`, which git's xdiff uses. */
const SPACE = " \t\n\v\f\r";
const MAX_INDENT = 200;
const MAX_BLANKS = 20;
const INDENT_HEURISTIC_MAX_SLIDING = 100;

/** A line's indent, a tab to the next multiple of 8; -1 for a blank line. */
function indentOf(line: string): number {
  let n = 0;
  for (const c of line) {
    if (c === " ") n += 1;
    else if (c === "\t") n += 8 - (n % 8);
    else if (!SPACE.includes(c)) return n;
    if (n >= MAX_INDENT) return MAX_INDENT;
  }
  return -1;
}

/** What surrounds a split before line `at`, as git measures it. */
function measureSplit(f: Side, at: number) {
  const m = { endOfFile: at >= f.recs.length, indent: -1, preBlank: 0, preIndent: -1, postBlank: 0, postIndent: -1 };
  if (!m.endOfFile) m.indent = indentOf(f.recs[at] ?? "");
  for (let i = at - 1; i >= 0; i--) {
    m.preIndent = indentOf(f.recs[i] ?? "");
    if (m.preIndent !== -1) break;
    m.preBlank += 1;
    if (m.preBlank === MAX_BLANKS) {
      m.preIndent = 0;
      break;
    }
  }
  for (let i = at + 1; i < f.recs.length; i++) {
    m.postIndent = indentOf(f.recs[i] ?? "");
    if (m.postIndent !== -1) break;
    m.postBlank += 1;
    if (m.postBlank === MAX_BLANKS) {
      m.postIndent = 0;
      break;
    }
  }
  return m;
}

type Score = { effectiveIndent: number; penalty: number };

/** git's penalties for a split (`score_add_split`). */
function scoreSplit(m: ReturnType<typeof measureSplit>, s: Score): void {
  if (m.preIndent === -1 && m.preBlank === 0) s.penalty += 1;
  if (m.endOfFile) s.penalty += 21;
  const postBlank = m.indent === -1 ? 1 + m.postBlank : 0;
  const totalBlank = m.preBlank + postBlank;
  s.penalty += -30 * totalBlank + 6 * postBlank;
  const indent = m.indent !== -1 ? m.indent : m.postIndent;
  const anyBlanks = totalBlank !== 0;
  s.effectiveIndent += indent;
  if (indent === -1 || m.preIndent === -1 || indent === m.preIndent) return;
  if (indent > m.preIndent) s.penalty += anyBlanks ? 10 : -4;
  else if (m.postIndent !== -1 && m.postIndent > indent) s.penalty += anyBlanks ? 17 : 24;
  else s.penalty += anyBlanks ? 17 : 23;
}

const scoreCmp = (a: Score, b: Score) =>
  60 * (Number(a.effectiveIndent > b.effectiveIndent) - Number(a.effectiveIndent < b.effectiveIndent)) +
  (a.penalty - b.penalty);

/** `xdl_change_compact` for side `x`, `o` being the other side. */
function compact(x: Side, o: Side): void {
  const g = groupInit(x);
  const go = groupInit(o);
  for (;;) {
    if (g.end !== g.start) {
      let size: number;
      let earliestEnd: number;
      let endMatchingOther: number;
      do {
        size = g.end - g.start;
        endMatchingOther = -1;
        // Up as far as it goes, then down as far as it goes.
        while (slideUp(x, g)) if (!groupPrevious(o, go)) throw new Error("udiff: groups out of step sliding up");
        earliestEnd = g.end;
        if (go.end > go.start) endMatchingOther = g.end;
        while (slideDown(x, g)) {
          if (!groupNext(o, go)) throw new Error("udiff: groups out of step sliding down");
          if (go.end > go.start) endMatchingOther = g.end;
        }
      } while (size !== g.end - g.start);
      if (g.end === earliestEnd) {
        // It couldn't move.
      } else if (endMatchingOther !== -1) {
        // Back up to line up with the other side's last change it met.
        while (go.end === go.start) {
          if (!slideUp(x, g)) throw new Error("udiff: lost the other side's change");
          if (!groupPrevious(o, go)) throw new Error("udiff: groups out of step lining up");
        }
      } else {
        // Where the splits above and below it read best.
        let shift = Math.max(earliestEnd, g.end - size - 1, g.end - INDENT_HEURISTIC_MAX_SLIDING);
        let bestShift = -1;
        let best: Score = { effectiveIndent: 0, penalty: 0 };
        for (; shift <= g.end; shift++) {
          const score: Score = { effectiveIndent: 0, penalty: 0 };
          scoreSplit(measureSplit(x, shift), score);
          scoreSplit(measureSplit(x, shift - size), score);
          if (bestShift === -1 || scoreCmp(score, best) <= 0) {
            best = score;
            bestShift = shift;
          }
        }
        while (g.end > bestShift) {
          if (!slideUp(x, g)) throw new Error("udiff: couldn't reach the best place");
          if (!groupPrevious(o, go)) throw new Error("udiff: groups out of step at the best place");
        }
      }
    }
    if (!groupNext(x, g)) break;
    if (!groupNext(o, go)) throw new Error("udiff: groups out of step moving on");
  }
}

/** The edit from `a` to `b` as git shows it: each run's removals before its
 * additions, every change where git puts it. */
function ops(a: string[], b: string[]): Op[] {
  const chg = changes(a, b);
  const x: Side = { recs: a, chg: chg.a };
  const y: Side = { recs: b, chg: chg.b };
  compact(x, y);
  compact(y, x);
  const out: Op[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && x.chg[i]) {
      out.push({ t: "-", line: a[i]!, a: i + 1, b: j });
      i++;
    } else if (j < b.length && y.chg[j]) {
      out.push({ t: "+", line: b[j]!, a: i, b: j + 1 });
      j++;
    } else {
      out.push({ t: " ", line: a[i]!, a: i + 1, b: j + 1 });
      i++;
      j++;
    }
  }
  return out;
}

const CONTEXT = 3;

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** git's default hunk name for a line (`def_ff`): the line if it starts with
 * a letter, `_` or `$`, cut to 80 bytes and its trailing spaces dropped. */
function funcName(line: string): string | null {
  if (!/^[A-Za-z_$]/.test(line)) return null;
  // git reads the line with its newline, then trims.
  const bytes = encoder.encode(`${line}\n`).slice(0, 80);
  return decoder.decode(bytes).replace(/[ \t\n\v\f\r]+$/, "");
}

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
  const old = linesOf(before ?? "");
  const all = ops(old, linesOf(after));
  const body: string[] = [];
  // git's default hunk name (xemit.c): the nearest line above the hunk that
  // starts with a letter, `_` or `$`, its first 80 bytes; looked for back to
  // the hunk before's first line, and that hunk's name kept when none is.
  let name = "";
  let searchedTo = -1;
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
    const was = hunk.filter((o) => o.t !== "+");
    const now = hunk.filter((o) => o.t !== "-");
    // The hunk's first old line, 0-based (for an addition, the line after).
    const first = hunk[0]!.t === "+" ? hunk[0]!.a : hunk[0]!.a - 1;
    for (let l = first - 1; l > searchedTo && l >= 0; l--) {
      const found = funcName(old[l] ?? "");
      if (found !== null) {
        name = found;
        break;
      }
    }
    searchedTo = first - 1;
    body.push(
      `@@ -${range(was[0]?.a ?? 0, was.length, hunk[0]!.a)} +${range(now[0]?.b ?? 0, now.length, hunk[0]!.b)} @@${name ? ` ${name}` : ""}`,
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
