import { browser } from "@wdio/globals";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const AXE_SOURCE = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

type Violation = { id: string; impact: string | null; nodes: unknown[] };

/** Runs axe-core inside the app webview and fails on serious or critical
 * violations (PRODUCT_SPEC §5.4). The webview can't open helper windows, so
 * axe is injected directly instead of through @axe-core/webdriverio. */
export async function expectNoSeriousA11yViolations(): Promise<void> {
  await browser.execute(AXE_SOURCE);
  const violations = (await browser.executeAsync((done: (v: Violation[]) => void) => {
    const w = window as unknown as { axe: { run: (ctx: Document) => Promise<{ violations: Violation[] }> } };
    w.axe
      .run(document)
      .then((r) => done(r.violations.map((v) => ({ id: v.id, impact: v.impact, nodes: v.nodes }))))
      .catch(() => done([{ id: "axe-failed", impact: "critical", nodes: [] }]));
  })) as Violation[];
  const serious = violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  if (serious.length > 0) {
    throw new Error(`axe: ${serious.map((v) => `${v.id} (${v.nodes.length})`).join(", ")}`);
  }
}
