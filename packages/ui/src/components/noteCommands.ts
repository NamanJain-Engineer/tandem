// Editing commands for Markdown notes (the scratchpad, F-SH-12): bold,
// italic and underline that wrap and unwrap the selection, list items that
// continue on Enter, and task boxes that tick.
import { EditorSelection, type ChangeSpec, type EditorState, type SelectionRange, type StateCommand } from "@codemirror/state";

export type NoteFormat = "bold" | "italic" | "underline" | "bullet" | "task";

/** `- `, `* `, `+ `, `1. ` or `1) `, then maybe a task box. */
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])( \[[ xX]\])?(\s+)/;
const TASK = /^(\s*)([-*+]|\d{1,9}[.)]) \[([ xX])\]/;

/** How many `ch` in a row end just before `pos`. */
function runBefore(state: EditorState, pos: number, ch: string): number {
  let n = 0;
  while (pos - n > 0 && state.sliceDoc(pos - n - 1, pos - n) === ch) n++;
  return n;
}

/** How many `ch` in a row start at `pos`. */
function runAfter(state: EditorState, pos: number, ch: string): number {
  let n = 0;
  while (pos + n < state.doc.length && state.sliceDoc(pos + n, pos + n + 1) === ch) n++;
  return n;
}

/** Whether a run of `*` holds this emphasis: bold needs 2 (or 3, bold and
 * italic), italic an odd count (1, or 3). */
function holds(run: number, stars: number): boolean {
  return stars === 2 ? run === 2 || run === 3 : run === 1 || run === 3;
}

function wrapRange(state: EditorState, range: SelectionRange, open: string, close: string): { changes: ChangeSpec[]; range: SelectionRange } {
  const { from, to } = range;
  const stars = /^\*+$/.test(open) ? open.length : 0;
  // Already wrapped just outside the selection: **[text]**.
  const outside = stars
    ? holds(runBefore(state, from, "*"), stars) && holds(runAfter(state, to, "*"), stars)
    : from >= open.length && state.sliceDoc(from - open.length, from) === open && state.sliceDoc(to, to + close.length) === close;
  if (outside) {
    return {
      changes: [
        { from: from - open.length, to: from, insert: "" },
        { from: to, to: to + close.length, insert: "" },
      ],
      range: EditorSelection.range(from - open.length, to - open.length),
    };
  }
  // The selection takes in the marks: [**text**].
  const text = state.sliceDoc(from, to);
  const inside =
    text.length >= open.length + close.length &&
    (stars
      ? holds(runAfter(state, from, "*"), stars) && holds(runBefore(state, to, "*"), stars) && runAfter(state, from, "*") < text.length
      : text.startsWith(open) && text.endsWith(close));
  if (inside) {
    return {
      changes: [
        { from, to: from + open.length, insert: "" },
        { from: to - close.length, to, insert: "" },
      ],
      range: EditorSelection.range(from, to - open.length - close.length),
    };
  }
  return {
    changes: [
      { from, insert: open },
      { from: to, insert: close },
    ],
    range: EditorSelection.range(from + open.length, to + open.length),
  };
}

/** Wraps each selection in `open`…`close`, or unwraps it when it's wrapped
 * already. An empty selection gets both marks with the cursor between. */
export function toggleWrap(open: string, close = open): StateCommand {
  return ({ state, dispatch }) => {
    if (state.readOnly) return false;
    dispatch(state.update(state.changeByRange((range) => wrapRange(state, range, open, close)), { scrollIntoView: true, userEvent: "input" }));
    return true;
  };
}

export const toggleBold = toggleWrap("**");
export const toggleItalic = toggleWrap("*");
export const toggleUnderline = toggleWrap("<u>", "</u>");

/** The lines the selection touches, each once. */
function selectedLines(state: EditorState) {
  const seen = new Set<number>();
  const lines = [];
  for (const r of state.selection.ranges) {
    for (let n = state.doc.lineAt(r.from).number; n <= state.doc.lineAt(r.to).number; n++) {
      if (!seen.has(n)) {
        seen.add(n);
        lines.push(state.doc.line(n));
      }
    }
  }
  return lines;
}

/** Makes the selected lines bullet items, or plain lines when they all are. */
export const toggleBullet: StateCommand = ({ state, dispatch }) => {
  if (state.readOnly) return false;
  const lines = selectedLines(state);
  const all = lines.every((l) => ITEM.test(l.text) && !TASK.test(l.text));
  const changes: ChangeSpec[] = lines.map((l) => {
    const m = ITEM.exec(l.text);
    if (all && m) return { from: l.from + (m[1]?.length ?? 0), to: l.from + m[0].length, insert: "" };
    if (m) return { from: l.from + (m[1]?.length ?? 0), to: l.from + m[0].length, insert: "- " };
    const indent = /^\s*/.exec(l.text)?.[0].length ?? 0;
    return { from: l.from + indent, insert: "- " };
  });
  dispatch(state.update({ changes, scrollIntoView: true, userEvent: "input" }));
  return true;
};

/** Makes the selected lines task items; on task items, ticks or unticks them. */
export const toggleTask: StateCommand = ({ state, dispatch }) => {
  if (state.readOnly) return false;
  const lines = selectedLines(state);
  const tasks = lines.map((l) => TASK.exec(l.text));
  const changes: ChangeSpec[] = [];
  if (tasks.every((t) => t !== null)) {
    // Tick them all, or untick them all when every one is ticked.
    const allDone = tasks.every((t) => t?.[3] !== " ");
    lines.forEach((l, i) => {
      const t = tasks[i];
      if (!t) return;
      const box = l.from + t[0].length - 2;
      changes.push({ from: box, to: box + 1, insert: allDone ? " " : "x" });
    });
  } else {
    lines.forEach((l, i) => {
      if (tasks[i]) return;
      const m = ITEM.exec(l.text);
      const indent = /^\s*/.exec(l.text)?.[0].length ?? 0;
      if (m) changes.push({ from: l.from + (m[1]?.length ?? 0), to: l.from + m[0].length, insert: "- [ ] " });
      else changes.push({ from: l.from + indent, insert: "- [ ] " });
    });
  }
  dispatch(state.update({ changes, scrollIntoView: true, userEvent: "input" }));
  return true;
};

/** Ticks or unticks the task box that starts at `pos` (a click on its box). */
export function tickAt(state: EditorState, pos: number) {
  const line = state.doc.lineAt(pos);
  const t = TASK.exec(line.text);
  if (!t) return null;
  const box = line.from + t[0].length - 2;
  return state.update({ changes: { from: box, to: box + 1, insert: t[3] === " " ? "x" : " " }, userEvent: "input" });
}

/** Enter in a list item starts the next one (an unticked box for tasks, the
 * next number for numbered items); Enter on an empty item ends the list. */
export const continueList: StateCommand = ({ state, dispatch }) => {
  if (state.readOnly || state.selection.ranges.length !== 1 || !state.selection.main.empty) return false;
  const pos = state.selection.main.head;
  const line = state.doc.lineAt(pos);
  const m = ITEM.exec(line.text);
  if (!m || pos < line.from + m[0].length) return false;
  const [, indent = "", bullet = "-", box, space = " "] = m;
  if (line.text.slice(m[0].length).trim() === "" && pos === line.to) {
    dispatch(state.update({ changes: { from: line.from, to: line.to, insert: "" }, selection: { anchor: line.from }, userEvent: "input" }));
    return true;
  }
  const next = /^\d/.test(bullet) ? `${Number.parseInt(bullet, 10) + 1}${bullet.slice(-1)}` : bullet;
  const insert = `\n${indent}${next}${box ? " [ ]" : ""}${space}`;
  dispatch(state.update({ changes: { from: pos, insert }, selection: { anchor: pos + insert.length }, scrollIntoView: true, userEvent: "input" }));
  return true;
};

/** The command behind each toolbar button. */
export const NOTE_FORMATS: Record<NoteFormat, StateCommand> = {
  bold: toggleBold,
  italic: toggleItalic,
  underline: toggleUnderline,
  bullet: toggleBullet,
  task: toggleTask,
};

/** Where task boxes are in `text` (their `[` positions), for the editor's
 * checkboxes. */
export function taskBoxes(text: string, offset = 0): Array<{ from: number; checked: boolean }> {
  const out: Array<{ from: number; checked: boolean }> = [];
  let at = offset;
  for (const line of text.split("\n")) {
    const t = TASK.exec(line);
    if (t) out.push({ from: at + t[0].length - 3, checked: t[3] !== " " });
    at += line.length + 1;
  }
  return out;
}
