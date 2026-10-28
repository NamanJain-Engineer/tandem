import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { DEFANGED, defangMentions } from "./untrusted";

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
    for (const c of vectors.cases) expect(defangMentions(c.in), JSON.stringify(c.in)).toBe(c.out);
  });
});
