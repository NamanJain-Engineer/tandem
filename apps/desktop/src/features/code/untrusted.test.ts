import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { DEFANGED, defang, defangMarked, removable } from "./untrusted";

type Ranges = [number, number][];
type Vectors = { note: string; cases: { in: string; out: string }[]; removable: Ranges };

const fixture = (name: string): unknown =>
  JSON.parse(
    readFileSync(join(import.meta.dirname, "../../../../../crates/tandem-core/tests/fixtures", name), "utf8"),
  );
// The core reads the same file, so both make the same text (D-3023).
const vectors = fixture("defang.json") as Vectors;
// What Claude Code's prompt drops, from its own stripper (D-3030).
const engine = fixture("claude-code-strip.json") as { version: string; sha256: string; removed: Ranges };
const within = (rs: Ranges, cp: number) => rs.some(([lo, hi]) => cp >= lo && cp <= hi);
// A test over every code point makes about 1.1 million checks: more than
// vitest's default 5 s allows while the whole suite runs at once.
const EVERY_CODE_POINT_MS = 20_000;

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

  it(
    "F-WT-02.1 drops the core's list, which covers every code point Claude Code's prompt drops",
    () => {
      expect(engine.version).toMatch(/^\d+\.\d+\.\d+/);
      expect(engine.sha256).toMatch(/^[0-9a-f]{64}$/);
      for (let cp = 0; cp <= 0x10ffff; cp++) {
        if (cp >= 0xd800 && cp <= 0xdfff) continue;
        if (removable(cp) !== within(vectors.removable, cp)) {
          throw new Error(`U+${cp.toString(16).toUpperCase()} isn't the core's`);
        }
        if (within(engine.removed, cp) && !removable(cp)) {
          throw new Error(`Claude Code ${engine.version} drops U+${cp.toString(16).toUpperCase()}`);
        }
      }
    },
    EVERY_CODE_POINT_MS,
  );

  it(
    "F-WT-02.1 drops each removable code point wherever it is, and reads the Kelvin sign as K",
    () => {
      // Behaviour over every code point, not only the list it reads. Each
      // "a<c>b" stays apart from the next when they're joined (nothing a rule
      // reads spans "b" and "a"), so a chunk is one call; a chunk that differs
      // is read again one code point at a time, to name it.
      const want = (cp: number, c: string) =>
        removable(cp) ? "ab" : c === "@" ? "a\uFF20b" : cp === 0x212a ? "aKb" : `a${c}b`;
      for (let from = 0; from <= 0x10ffff; from += 0x1000) {
        const cps: number[] = [];
        for (let cp = from; cp < from + 0x1000 && cp <= 0x10ffff; cp++) {
          if (cp < 0xd800 || cp > 0xdfff) cps.push(cp);
        }
        const input = cps.map((cp) => `a${String.fromCodePoint(cp)}b`).join("");
        if (defang(input) === cps.map((cp) => want(cp, String.fromCodePoint(cp))).join("")) continue;
        for (const cp of cps) {
          const c = String.fromCodePoint(cp);
          const got = defang(`a${c}b`);
          if (got !== want(cp, c))
            throw new Error(`U+${cp.toString(16).toUpperCase()} gave ${JSON.stringify(got)}`);
        }
        throw new Error(`the chunk from U+${from.toString(16).toUpperCase()} differs only when joined`);
      }
      expect(defang("ultrathin\u212A")).toBe("ultra-thinK");
    },
    EVERY_CODE_POINT_MS,
  );

  it(
    "F-TK-08.1 writes a full-width slash before every code point but a space, * or /",
    () => {
      // What follows a `/` decides whether it opens a skill's name (D-3035).
      // The spaces are the core's too: JavaScript's \s and Rust's whitespace
      // agree once what's removable is gone, so a space outside ASCII opens no
      // word in either. Each "x /<c>y" ends its word at the line break that
      // joins it to the next, so a chunk is one call.
      const spaces: Ranges = [
        [0x09, 0x0a],
        [0x0d, 0x0d],
        [0x20, 0x20],
        [0xa0, 0xa0],
        [0x1680, 0x1680],
        [0x2000, 0x200a],
        [0x202f, 0x202f],
        [0x205f, 0x205f],
        [0x3000, 0x3000],
      ];
      const want = (cp: number, c: string) => {
        if (within(spaces, cp) || c === "*" || c === "/") return `x /${c}y`;
        const rest = removable(cp) ? "" : c === "@" ? "\uFF20" : cp === 0x212a ? "K" : c;
        return `x \uFF0F${rest}y`;
      };
      for (let from = 0; from <= 0x10ffff; from += 0x1000) {
        const cps: number[] = [];
        for (let cp = from; cp < from + 0x1000 && cp <= 0x10ffff; cp++) {
          if (cp < 0xd800 || cp > 0xdfff) cps.push(cp);
        }
        for (const cp of cps) {
          const c = String.fromCodePoint(cp);
          if (within(spaces, cp) !== (/\s/u.test(c) && !removable(cp))) {
            throw new Error(`U+${cp.toString(16).toUpperCase()} is a space to one of them only`);
          }
        }
        const input = cps.map((cp) => `x /${String.fromCodePoint(cp)}y`).join("\n");
        if (defang(input) === cps.map((cp) => want(cp, String.fromCodePoint(cp))).join("\n")) continue;
        for (const cp of cps) {
          const c = String.fromCodePoint(cp);
          const got = defang(`x /${c}y`);
          if (got !== want(cp, c))
            throw new Error(`U+${cp.toString(16).toUpperCase()} gave ${JSON.stringify(got)}`);
        }
        throw new Error(`the chunk from U+${from.toString(16).toUpperCase()} differs only when joined`);
      }
    },
    EVERY_CODE_POINT_MS,
  );

  it("F-WT-02.1 notes only an at sign it wrote", () => {
    expect(defangMarked("ultraplan [Image #1]")).toEqual({
      text: "ultra-plan \uFF3BImage #1]",
      wroteAt: false,
    });
    expect(defangMarked("ultraplan @x")).toEqual({ text: "ultra-plan \uFF20x", wroteAt: true });
    expect(defangMarked("\u200b@x")).toEqual({ text: "\uFF20x", wroteAt: true });
  });
});
