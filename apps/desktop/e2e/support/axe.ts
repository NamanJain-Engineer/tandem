import { browser } from "@wdio/globals";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const AXE_SOURCE = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

type Violation = { id: string; impact: string | null; nodes: Array<{ target: string[] }> };

/** Runs axe-core inside the app webview and fails on serious or critical
 * violations (PRODUCT_SPEC §5.4). The webview can't open helper windows, so
 * axe is injected directly instead of through @axe-core/webdriverio. */
export async function expectNoSeriousA11yViolations(): Promise<void> {
  await browser.execute(AXE_SOURCE);
  const violations = (await browser.executeAsync((done: (v: Violation[]) => void) => {
    const w = window as unknown as { axe: { run: (ctx: Document) => Promise<{ violations: Violation[] }> } };
    w.axe
      .run(document)
      .then((r) => done(r.violations.map((v) => ({ id: v.id, impact: v.impact, nodes: v.nodes.map((n) => ({ target: n.target })) }))))
      .catch(() => done([{ id: "axe-failed", impact: "critical", nodes: [] }]));
  })) as Violation[];
  const serious = violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  if (serious.length > 0) {
    throw new Error(`axe: ${serious.map((v) => `${v.id} (${v.nodes.map((n) => n.target.join(" ")).join("; ")})`).join(", ")}`);
  }
}

/** Runs the check in the dark and the light theme (M2 exit checklist). */
export async function expectNoSeriousA11yViolationsInBothThemes(): Promise<void> {
  const setTheme = (value: string) =>
    browser.executeAsync((v: string, done: () => void) => {
      const w = window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a: unknown) => Promise<unknown> } };
      void w.__TAURI_INTERNALS__.invoke("settings_set", { key: "theme", value: v }).then(() => setTimeout(done, 300));
    }, value);
  try {
    for (const theme of ["dark", "light"]) {
      await setTheme(theme);
      await expectNoSeriousA11yViolations();
    }
  } finally {
    await setTheme("dark");
  }
}
