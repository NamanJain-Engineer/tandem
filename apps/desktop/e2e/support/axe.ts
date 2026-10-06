import { browser } from "@wdio/globals";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const AXE_SOURCE = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

type Violation = { id: string; impact: string | null; nodes: Array<{ target: string[]; why?: string }> };

/** Runs axe-core inside the app webview and fails on serious or critical
 * violations (PRODUCT_SPEC §5.4). The webview can't open helper windows, so
 * axe is injected directly instead of through @axe-core/webdriverio. */
export async function expectNoSeriousA11yViolations(): Promise<void> {
  // Colour contrast is measured on settled pixels: a dialog mid-fade has
  // partial opacity and would read as low contrast.
  await browser.waitUntil(
    async () =>
      await browser.execute(() =>
        // Spinners loop forever; only finite transitions have to settle.
        document
          .getAnimations()
          .every((a) => a.playState !== "running" || a.effect?.getTiming().iterations === Infinity),
      ),
    { timeout: 5_000, timeoutMsg: "animations still running before axe" },
  );
  // …and dialogs fully opaque: a fade that WebKit doesn't list as an
  // animation leaves text blended toward the background (measured once as
  // light text reading #595f68 on the upgrade sheet).
  await browser.waitUntil(
    async () =>
      browser.execute(() =>
        Array.from(document.querySelectorAll("[role=dialog], [role=alertdialog]")).every((el) => {
          for (let n: Element | null = el; n; n = n.parentElement) if (Number(getComputedStyle(n).opacity) < 1) return false;
          return true;
        }),
      ),
    { timeout: 5_000, timeoutMsg: "a dialog never became fully opaque before axe" },
  );
  await browser.execute(AXE_SOURCE);
  const violations = (await browser.executeAsync((done: (v: Violation[]) => void) => {
    const w = window as unknown as { axe: { run: (ctx: Document) => Promise<{ violations: Violation[] }> } };
    w.axe
      .run(document)
      .then((r) =>
        done(
          r.violations.map((v) => ({
            id: v.id,
            impact: v.impact,
            // Contrast failures say what was measured.
            nodes: v.nodes.map((n) => {
              const d = (n as unknown as { any?: Array<{ data?: { fgColor?: string; bgColor?: string; contrastRatio?: number } }> }).any?.[0]?.data;
              return { target: n.target, why: d?.contrastRatio ? `${d.fgColor} on ${d.bgColor} = ${d.contrastRatio}` : undefined };
            }),
          })),
        ),
      )
      .catch(() => done([{ id: "axe-failed", impact: "critical", nodes: [] }]));
  })) as Violation[];
  const serious = violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  if (serious.length > 0) {
    throw new Error(
      `axe (${await browser.execute(() => document.documentElement.dataset.theme)}): ${serious
        .map((v) => `${v.id} (${v.nodes.map((n) => `${n.target.join(" ")}${n.why ? ` [${n.why}]` : ""}`).join("; ")})`)
        .join(", ")}`,
    );
  }
}

/** Runs the check in the dark and the light theme (M2 exit checklist). */
export async function expectNoSeriousA11yViolationsInBothThemes(): Promise<void> {
  // The theme applies when the setting's event reaches the webview, which
  // under load can take longer than any fixed pause: wait until the page is
  // on the new theme, so axe never measures a half-switched page.
  const setTheme = async (value: string) => {
    await browser.execute((v: string) => {
      const w = window as unknown as {
        __TAURI_INTERNALS__: { invoke: (c: string, a: unknown) => Promise<unknown> };
      };
      void w.__TAURI_INTERNALS__.invoke("settings_set", { key: "theme", value: v });
    }, value);
    await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === value, {
      timeout: 10_000,
      timeoutMsg: `theme never became ${value}`,
    });
    // Colours then transition (WebKit doesn't list CSS transitions in
    // getAnimations): wait until the computed colours stop changing.
    const sample = () =>
      browser.execute(() =>
        Array.from(document.querySelectorAll("body, [role=dialog], [role=dialog] *"))
          .slice(0, 200)
          .map((el) => {
            const cs = getComputedStyle(el);
            return `${cs.color}|${cs.backgroundColor}`;
          })
          .join(";"),
      );
    let last = await sample();
    await browser.waitUntil(
      async () => {
        await browser.pause(120);
        const now = await sample();
        const stable = now === last;
        last = now;
        return stable;
      },
      { timeout: 5_000, timeoutMsg: "colours kept changing after the theme switch" },
    );
  };
  try {
    for (const theme of ["dark", "light"]) {
      await setTheme(theme);
      await expectNoSeriousA11yViolations();
    }
  } finally {
    await setTheme("dark");
  }
}
