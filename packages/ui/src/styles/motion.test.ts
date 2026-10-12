// Entry animations (UI_SPEC §1.8) leave a panel where its layout puts it.
// Panels centre themselves with Tailwind's translate utilities, which set
// the `translate` property; a keyframe `transform: translate(…)` adds to it,
// so a dialog sat half its size up and to the left while it opened and then
// jumped to the centre (D-842).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const base = readFileSync(join(import.meta.dirname, "base.css"), "utf8");
const source = (file: string) => readFileSync(join(import.meta.dirname, "..", "components", file), "utf8");

/** The body of one `@keyframes` rule, braces balanced. */
function keyframes(name: string): string {
  const start = base.indexOf(`@keyframes ${name} `);
  if (start < 0) throw new Error(`no @keyframes ${name}`);
  let depth = 0;
  for (let i = base.indexOf("{", start); i < base.length; i++) {
    if (base[i] === "{") depth++;
    else if (base[i] === "}" && --depth === 0) return base.slice(start, i + 1);
  }
  throw new Error(`@keyframes ${name} never closes`);
}

describe("entry animations (UI_SPEC §1.8)", () => {
  it("UI_SPEC §1.8 a dialog opens with fade and scale 0.97→1 only, where it stays (D-842)", () => {
    const dialog = keyframes("tn-dialog-in");
    expect(dialog).toContain("scale(0.97)");
    expect(dialog).toContain("opacity: 0");
    expect(dialog).not.toMatch(/translate/);
  });

  it("UI_SPEC §2.10 the panels that open with it centre with the translate utilities", () => {
    for (const file of ["Dialog.tsx", "CommandPalette.tsx", "QuickPicker.tsx"]) {
      const panel = source(file)
        .split("\n")
        .find((l) => l.includes("tn-dialog "));
      expect(panel, file).toContain("-translate-x-1/2");
    }
  });
});
