// DESIGN §5.4 (the owner's 17 November ask, D-2653): the shell is lit. One
// bloom behind the work area, chrome that lets it through, panes that float
// with a lit edge, and the focused pane holding the light. Brand tokens only,
// with the plain surfaces back under high contrast and reduced transparency.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const css = readFileSync(join(import.meta.dirname, "shell.css"), "utf8");
const src = (file: string) => readFileSync(join(import.meta.dirname, file), "utf8");

/** The declarations of every block whose selector list names `selector`. */
function rules(selector: string): string {
  const out: string[] = [];
  for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selectors = (m[1] ?? "")
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .split(",")
      .map((s) => s.trim());
    if (selectors.includes(selector)) out.push(m[2] ?? "");
  }
  return out.join("\n");
}

describe("DESIGN-5.4 the lit shell (D-2653)", () => {
  it("DESIGN-5.4 the shell parts carry their classes", () => {
    expect(src("features/shell/AppShell.tsx")).toContain("tn-shell ");
    expect(src("features/shell/AppShell.tsx")).toContain("tn-main ");
    expect(src("features/shell/TitleBar.tsx")).toContain("tn-titlebar ");
    expect(src("features/shell/StatusBar.tsx")).toContain("tn-statusbar ");
    expect(src("features/code/CodeRail.tsx")).toContain("tn-rail ");
    expect(src("features/agents/AgentRail.tsx")).toContain("tn-rail ");
    expect(src("features/thread/ThreadRail.tsx")).toContain("tn-rail ");
    expect(src("features/code/PaneGrid.tsx")).toContain("tn-pane ");
    expect(src("features/code/CodeMain.tsx")).toContain("tn-tab ");
    expect(src("features/code/CodeMain.tsx")).toContain('data-selected={selected ? "true" : undefined}');
    expect(src("styles.css")).toContain('@import "./shell.css";');
  });

  it("DESIGN-5.4 one bloom of the brand's light behind the work area", () => {
    const shell = rules(".tn-shell");
    expect(shell).toMatch(/radial-gradient\([^;]*var\(--color-accent\)/);
    expect(shell).toContain("background-color: var(--color-bg)");
  });

  it("DESIGN-5.4 panes float with a lit edge; the focused pane holds the light", () => {
    const pane = rules(".tn-pane");
    expect(pane).toContain("var(--tn-elev-2)");
    expect(pane).toContain("var(--tn-edge-top)");
    expect(pane).toContain("var(--radius-lg)");
    const focused = rules('.tn-pane[data-focused="true"]');
    expect(focused).toContain("var(--tn-ring-lit)");
    expect(focused).toContain("var(--tn-accent-ring)");
  });

  it("DESIGN-5.4 the current row, mode and tab are lit", () => {
    expect(rules('.tn-rail [role="treeitem"][aria-selected="true"]')).toContain("var(--color-accent)");
    expect(rules('.tn-titlebar [role="radio"][aria-checked="true"]')).toContain("var(--tn-edge-hi)");
    expect(rules('.tn-tab[data-selected="true"]')).toContain("var(--tn-edge-hi)");
  });

  it("DESIGN-5.4 brand tokens only: no raw colours outside a shadow's black", () => {
    const body = css.replace(/\/\*[\s\S]*?\*\//g, "").replace(/rgb\(0 0 0 \/ [0-9.]+\)/g, "");
    expect(body).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(body).not.toMatch(/rgba?\(/);
  });

  it("DESIGN-5.4 high contrast and reduced transparency get the plain surfaces", () => {
    expect(rules('[data-contrast="more"] .tn-shell')).toContain("background-image: none");
    expect(rules('[data-transparency="reduced"] .tn-shell')).toContain("background-image: none");
    expect(rules('[data-contrast="more"] .tn-rail')).toContain("var(--color-surface-1)");
    expect(rules('[data-motion="reduced"] .tn-pane')).toContain("transition: none");
  });
});
