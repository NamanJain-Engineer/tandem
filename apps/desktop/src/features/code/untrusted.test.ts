import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { DEFANGED, defang, defangMarked } from "./untrusted";

type Vectors = { note: string; cases: { in: string; out: string }[] };

// The core reads the same file, so both make the same text (D-3023).
const vectors = JSON.parse(
  readFileSync(
    join(import.meta.dirname, "../../../../../crates/tandem-core/tests/fixtures/defang.json"),
    "utf8",
  ),
) as Vectors;

describe("untrusted text for an agent (D-3023)", () => {
  it("F-WT-02.1 makes the core's text of every shared vector, and its note names nothing", () => {
    expect(DEFANGED).toBe(vectors.note);
    expect(DEFANGED).not.toContain("@");
    expect(vectors.cases.length).toBeGreaterThanOrEqual(15);
    for (const c of vectors.cases) {
      expect(defang(c.in), JSON.stringify(c.in)).toBe(c.out);
      expect(defang(c.out), JSON.stringify(c.out)).toBe(c.out);
    }
  });

  it("F-WT-02.1 drops exactly the characters the core drops: every default-ignorable code point", () => {
    // The list is the core's, not this engine's tables; they must agree.
    const ignorable = /^\p{Default_Ignorable_Code_Point}$/u;
    for (let cp = 0; cp <= 0x10ffff; cp++) {
      if (cp >= 0xd800 && cp <= 0xdfff) continue;
      const ch = String.fromCodePoint(cp);
      if (ignorable.test(ch) !== (defang(`a${ch}b`) === "ab")) {
        throw new Error(`U+${cp.toString(16).toUpperCase()} is treated differently`);
      }
    }
  });

  it("F-WT-02.1 notes only an at sign it wrote", () => {
    expect(defangMarked("ultraplan [Image #1]")).toEqual({
      text: "ultra-plan \uFF3BImage #1]",
      wroteAt: false,
    });
    expect(defangMarked("ultraplan @x")).toEqual({ text: "ultra-plan \uFF20x", wroteAt: true });
    expect(defangMarked("\u200b@x")).toEqual({ text: "\uFF20x", wroteAt: true });
  });
});
