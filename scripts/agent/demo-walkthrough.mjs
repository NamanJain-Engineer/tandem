// Walks the browser demo (vibezeo.com/demo/app/) in Chromium and screenshots
// each step; prints console errors. Serve the built site first:
//   pnpm --filter desktop build:demo && (cd apps/web/public && python3 -m http.server 4321 &)
//   node scripts/agent/demo-walkthrough.mjs /tmp/demo-shots http://127.0.0.1:4321/demo/app/
import { createRequire } from "node:module";
const require = createRequire(new URL("../../apps/web/package.json", import.meta.url));
const { chromium } = require("@playwright/test");
const out = process.argv[2] ?? "/tmp/demo-shots";
const url = process.argv[3] ?? "http://127.0.0.1:4321/demo/app/";
import { mkdirSync } from "node:fs";
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium" });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const errors = [];
page.on("console", (m) => { if (m.type() === "error" || m.type() === "warning") errors.push(`${m.type()}: ${m.text()}`); });
page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
await page.goto(url);
await page.waitForSelector('[data-testid="demo-bar"]', { timeout: 15000 });
await page.waitForTimeout(2500);
await page.screenshot({ path: `${out}/1-start.png` });
const box = page.locator('[data-testid="composer"] textarea[aria-label="Message"]');
console.log("draft:", await box.inputValue().catch((e) => `ERR ${e.message}`));
await box.focus();
await page.keyboard.press("Enter");
await page.waitForTimeout(6000);
await page.screenshot({ path: `${out}/2-approval.png` });
const allow = page.getByRole("button", { name: "Allow once" }).first();
console.log("allow visible:", await allow.isVisible().catch(() => false));
await allow.click({ timeout: 5000 }).catch((e) => console.log("allow click failed", e.message));
await page.waitForTimeout(4000);
await page.screenshot({ path: `${out}/3-done.png` });
await page.locator('[data-testid="changes-toggle"]').click().catch((e) => console.log("changes", e.message));
await page.waitForTimeout(1000);
await page.locator('[data-testid="change-row"]').first().click().catch((e) => console.log("change-row", e.message));
await page.waitForTimeout(800);
await page.screenshot({ path: `${out}/4-changes.png` });
await page.keyboard.press("Control+2");
await page.waitForTimeout(2500);
await page.screenshot({ path: `${out}/5-code.png` });
const term = page.locator('.xterm-helper-textarea').last();
await term.focus().catch((e) => console.log("term", e.message));
await page.keyboard.type("git status");
await page.keyboard.press("Enter");
await page.waitForTimeout(800);
await page.screenshot({ path: `${out}/6-shell.png` });
await page.keyboard.press("Control+1");
await page.waitForTimeout(1500);
await page.screenshot({ path: `${out}/7-agent.png` });
// The screens a release adds (D-833): each must render, never blank the page.
const alive = async (step) => {
  const ok = await page.locator('[data-testid="demo-bar"]').isVisible().catch(() => false);
  console.log(`${step}: ${ok ? "page alive" : "PAGE BLANK"}`);
};
const command = async (title) => {
  await page.keyboard.press("Control+k");
  await page.waitForTimeout(400);
  await page.keyboard.type(title);
  await page.waitForTimeout(400);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1200);
};
await page.keyboard.press("Control+,");
await page.waitForTimeout(1000);
await page.getByRole("button", { name: "Remote", exact: true }).first().click().catch((e) => console.log("remote", e.message));
await page.waitForTimeout(1200);
await page.screenshot({ path: `${out}/8-remote.png` });
await alive("8 Settings › Remote");
await page.keyboard.press("Escape");
await page.waitForTimeout(600);
await page.keyboard.press("Control+2");
await page.waitForTimeout(1200);
for (const [n, title] of [
  [9, "Show checkpoints"],
  [10, "Show ports"],
  [11, "Insert prompt"],
  [12, "Recall a command"],
  [13, "Best of N"],
]) {
  await command(title);
  await page.screenshot({ path: `${out}/${n}-${title.toLowerCase().replace(/[^a-z]+/g, "-")}.png` });
  await alive(`${n} ${title}`);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(500);
}
console.log("errors:\n" + errors.slice(0, 30).join("\n"));
await browser.close();
