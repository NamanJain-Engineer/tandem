// The browser demo's Scratchpad (F-SH-12): notes kept in the page, with the
// app's rules. The newest is listed first, each is titled by its first line
// with words, and a save that lands after a delete is refused. Nothing is
// saved anywhere.
import type { ScratchNote } from "@tandem/protocol";

const NOT_FOUND = { code: "not_found", message: "That note doesn't exist" };
const LEADS = ["- [ ] ", "- [x] ", "- [X] ", "* [ ] ", "* [x] ", "* [X] ", "[ ] ", "[x] ", "[X] ", "- ", "* ", "+ ", "> "];
const MARKS = ["<u>", "</u>", "**", "__", "~~", "`", "*"];

/** The line without one leading list, task, quote or heading mark. */
function stripLead(line: string): string | null {
  const s = line.trimStart();
  const lead = LEADS.find((m) => s.startsWith(m));
  if (lead) return s.slice(lead.length);
  const mark = /^#+(?= |$)/.exec(s) ?? /^\d+[.)] /.exec(s);
  return mark ? s.slice(mark[0].length) : null;
}

/** A note's title, as the app's store makes it (`tandem_store::scratchpad::title_of`). */
export function noteTitle(markdown: string): string {
  for (const raw of markdown.split(/\r?\n/)) {
    let line = raw;
    for (let rest = stripLead(line); rest !== null; rest = stripLead(line)) line = rest;
    for (const m of MARKS) line = line.split(m).join("");
    const words = line.split(/\s+/).filter(Boolean).join(" ");
    if (words) return [...words].slice(0, 80).join("");
  }
  return "";
}

export class DemoNotes {
  /** Oldest saved first. */
  private notes = new Map<string, { text: string; updatedAt: number }>();
  private next = 0;

  private meta(id: string): ScratchNote {
    const note = this.notes.get(id);
    const text = note?.text ?? "";
    return { id, title: noteTitle(text), updatedAt: note?.updatedAt ?? 0, bytes: new TextEncoder().encode(text).length };
  }

  list(): ScratchNote[] {
    return [...this.notes.keys()].reverse().map((id) => this.meta(id));
  }

  read(id: string): string {
    const note = this.notes.get(id);
    if (!note) throw NOT_FOUND;
    return note.text;
  }

  write(id: string | null, markdown: string): ScratchNote {
    if (id !== null && !this.notes.has(id)) throw NOT_FOUND;
    const key = id ?? `note_demo${++this.next}`;
    this.notes.delete(key);
    this.notes.set(key, { text: markdown, updatedAt: Date.now() });
    return this.meta(key);
  }

  delete(id: string): void {
    if (!this.notes.delete(id)) throw NOT_FOUND;
  }
}
