import { describe, expect, it } from "vitest";
import { addNote, deleteNote, notesOf, parseNotes, serializeNotes } from "./notes";

const SAMPLES = [
  "",
  "- one\n- two\n",
  "# Notes\n\nIntro line.\n- one\n  continued here\n- two\n\nTrailing text\nno newline at end",
  "- crlf one\r\n- crlf two\r\n\r\n",
  "  - indented is not a top-level bullet\n* star bullet\n-not a bullet\n- real\n",
  "﻿- with a BOM\n- émoji 🎉 and tabs\t\n\n\n",
];

describe("NOTES.md as a list", () => {
  it("F-AG-07.3 List → Raw → List preserves every byte", () => {
    for (const md of SAMPLES) {
      const segs = parseNotes(md);
      expect(serializeNotes(segs)).toBe(md);
      expect(serializeNotes(parseNotes(serializeNotes(segs)))).toBe(md);
    }
  });

  it("F-AG-07 one bullet is one note, with its indented lines", () => {
    const notes = notesOf(parseNotes(SAMPLES[2] ?? ""));
    expect(notes.map((n) => n.text)).toEqual(["one continued here", "two"]);
    expect(notesOf(parseNotes(SAMPLES[4] ?? "")).map((n) => n.text)).toEqual(["real"]);
  });

  it("F-AG-07 adding and deleting a note touches nothing else", () => {
    const md = "# Notes\n- one\n\nfooter";
    const added = addNote(md, "two\nlines");
    expect(added).toBe("# Notes\n- one\n\nfooter\n- two lines\n");
    const segs = parseNotes(added);
    const one = notesOf(segs).find((n) => n.text === "one");
    expect(deleteNote(added, one?.index ?? -1)).toBe("# Notes\n\nfooter\n- two lines\n");
    expect(addNote("", "first")).toBe("- first\n");
    expect(addNote("- a\r\n", "b")).toBe("- a\r\n- b\r\n");
    // Not a note: unchanged.
    expect(deleteNote(md, 0)).toBe(md);
  });
});
