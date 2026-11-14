// DESIGN §4.5 and §5.4.3 (slice T2): the app's raised and floating surfaces
// take the brand's light, a lit top edge and e2/e3, from brand.css tokens
// only; §4.7 (slice T4): entrances arrive on the brand's expo-out; §5.4: a
// field whose frame shows focus doesn't also box itself in the global ring.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const base = readFileSync(join(import.meta.dirname, "base.css"), "utf8");
const source = (file: string) => readFileSync(join(import.meta.dirname, "..", "components", file), "utf8");

/** Every declaration block whose selector list names `selector`. */
function rules(selector: string): string[] {
  const out: string[] = [];
  const re = /([^{}]+)\{([^{}]*)\}/g;
  for (const m of base.matchAll(re)) {
    const selectors = (m[1] ?? "")
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .split(",")
      .map((s) => s.trim());
    if (selectors.includes(selector)) out.push(m[2] ?? "");
  }
  return out;
}

describe("DESIGN §5.4.3 raised surfaces take the brand (T2)", () => {
  // Toasts keep e2 without the lit edge for now: the site's scenes draw the
  // app's toast and pin its classes (apps/web scenes.test, D-2702).
  it("DESIGN-5.4.3 menus, selects, popovers, tooltips and the coach are raised: lit top edge and e2", () => {
    for (const file of ["Menu.tsx", "Select.tsx", "Popover.tsx", "Tooltip.tsx", "Coachmark.tsx"]) {
      const src = source(file);
      expect(src, file).toContain("tn-raised");
      expect(src, file).not.toMatch(/bg-surface-3[^"]*shadow-e2/);
    }
  });

  it("DESIGN-5.4.3 dialogs, sheets, the palette and the quick picker float: lit top edge and e3", () => {
    for (const file of ["Dialog.tsx", "CommandPalette.tsx", "QuickPicker.tsx"]) {
      const src = source(file);
      expect(src, file).toContain("tn-floating");
      expect(src, file).not.toContain("shadow-e3");
    }
    // The dialog, the confirm (same PANEL) and the sheet.
    expect(source("Dialog.tsx").match(/tn-floating/g)?.length).toBeGreaterThanOrEqual(2);
  });

  it("DESIGN-5.4.3 the edge is brighter on top and the depth is e2/e3, from brand.css tokens only", () => {
    const lit = rules(".tn-raised").join("\n") + rules(".tn-floating").join("\n");
    expect(lit).toContain("border-top-color: var(--tn-edge-top)");
    expect(lit).toContain("border-bottom-color: var(--tn-edge-bottom)");
    expect(rules(".tn-raised").join("\n")).toContain("var(--tn-elev-2)");
    expect(rules(".tn-floating").join("\n")).toContain("inset 0 1px 0 var(--tn-edge-hi), var(--tn-elev-3)");
    // No raw colour: every value is a token.
    expect(lit).not.toMatch(/#[0-9a-f]{3,8}\b|rgb\(|hsl\(|oklch\(/i);
  });

  it("DESIGN-5.4.3 high contrast keeps its strong border on lit surfaces", () => {
    const more = rules('[data-contrast="more"] .tn-raised').join("\n");
    expect(more).toContain("border-color: var(--tn-border)");
    expect(rules('[data-contrast="more"] .tn-floating').join("\n")).toContain(
      "border-color: var(--tn-border)",
    );
  });
});

describe("DESIGN §4.7 entrances (T4)", () => {
  it("DESIGN-4.7 fades, pops, dialogs and sheets enter on --tn-ease-out at the UI_SPEC durations", () => {
    for (const [cls, dur] of [
      [".tn-fade", "--dur-base"],
      [".tn-pop", "--dur-base"],
      [".tn-dialog", "--dur-slow"],
      [".tn-sheet", "--dur-slow"],
    ] as const) {
      const body = rules(cls).find((r) => r.includes("animation:")) ?? "";
      expect(body, cls).toContain(`var(${dur}) var(--tn-ease-out)`);
    }
  });

  it("F-SH-10 closing content still never animates", () => {
    for (const cls of [".tn-fade", ".tn-pop", ".tn-dialog", ".tn-sheet"])
      expect(rules(`${cls}[data-state="closed"]`).join(""), cls).toContain("animation: none");
  });
});

describe("DESIGN §5.4 focus drawn by the frame", () => {
  it("DESIGN-5.4.7 the palette's field and the composer's textarea leave the ring to their frame", () => {
    expect(rules(".tn-bare-field:focus-visible").join("")).toContain("box-shadow: none");
    expect(source("CommandPalette.tsx")).toContain("tn-bare-field");
    const composer = source("Composer.tsx");
    expect(composer).toContain("tn-bare-field");
    // The frame shows it instead, in the accent: a visible focus, not none.
    expect(composer).toContain("has-[textarea:focus-visible]:border-accent");
  });
});
