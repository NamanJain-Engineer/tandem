/**
 * NOTES.md as a list (F-AG-07, ARCHITECTURE §8.3): every top-level "- "
 * bullet is a note (with the indented lines under it); everything else is
 * kept as it is, byte for byte, so List → Raw → List never changes the file.
 */
export type Segment = { kind: "note"; raw: string; text: string } | { kind: "other"; raw: string };

const BULLET = /^- /;

/** Splits keeping each line's ending ("\n" or "\r\n"). */
function lines(md: string): string[] {
  const out: string[] = [];
  let start = 0;
  for (let i = 0; i < md.length; i++) {
    if (md[i] === "\n") {
      out.push(md.slice(start, i + 1));
      start = i + 1;
    }
  }
  if (start < md.length) out.push(md.slice(start));
  return out;
}

function stripEol(line: string): string {
  return line.replace(/\r?\n$/, "");
}

export function parseNotes(md: string): Segment[] {
  const segs: Segment[] = [];
  let note: string[] | null = null;
  const flush = () => {
    if (!note) return;
    const raw = note.join("");
    const text = note
      .map((l, i) => (i === 0 ? stripEol(l).replace(BULLET, "") : stripEol(l).trim()))
      .filter((l) => l.length > 0)
      .join(" ");
    segs.push({ kind: "note", raw, text });
    note = null;
  };
  for (const line of lines(md)) {
    if (BULLET.test(line)) {
      flush();
      note = [line];
    } else if (note && /^[ \t]+\S/.test(line)) {
      note.push(line);
    } else {
      flush();
      const last = segs[segs.length - 1];
      if (last?.kind === "other") last.raw += line;
      else segs.push({ kind: "other", raw: line });
    }
  }
  flush();
  return segs;
}

export function serializeNotes(segs: Segment[]): string {
  return segs.map((s) => s.raw).join("");
}

export function notesOf(segs: Segment[]): Array<{ index: number; text: string }> {
  return segs.flatMap((s, index) => (s.kind === "note" ? [{ index, text: s.text }] : []));
}

/** Adds a note at the end, on its own line. */
export function addNote(md: string, text: string): string {
  const line = `- ${text.replace(/\s*\n\s*/g, " ").trim()}\n`;
  if (md === "") return line;
  const eol = md.includes("\r\n") ? "\r\n" : "\n";
  return md.endsWith("\n") ? md + line.replace(/\n$/, eol) : md + eol + line.replace(/\n$/, eol);
}

/** Removes the note at segment `index`; every other byte stays. */
export function deleteNote(md: string, index: number): string {
  const segs = parseNotes(md);
  if (segs[index]?.kind !== "note") return md;
  return serializeNotes(segs.filter((_, i) => i !== index));
}
