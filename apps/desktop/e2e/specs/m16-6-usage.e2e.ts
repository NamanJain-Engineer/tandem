import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { addFolder, freshOffline, home, invoke } from "../support/app";

const ts = (daysAgo: number) => new Date(Date.now() - daysAgo * 86_400_000).toISOString();
const line = (daysAgo: number, id: string, tokens: number, cwd: string) =>
  JSON.stringify({
    type: "assistant",
    timestamp: ts(daysAgo),
    cwd,
    sessionId: "e2e-session",
    message: { id, usage: { input_tokens: tokens, output_tokens: 0 } },
  });

// E2E-M16-6 Usage (F-AC-07): the Workspace menu's Usage… page reads the
// engines' local logs: three days of a Claude account's usage, a malformed
// line counted as skipped, and a share card saved without workspace names.
describe("E2E-M16-6 usage dashboard", () => {
  const root = `/tmp/tandem-e2e-usage-${Date.now()}`;
  let accountHome = "";

  before(async function () {
    this.timeout(60_000);
    await freshOffline();
    await addFolder(root);
    const account = await invoke<{ homePath: string }>("test_account_create", {
      engineId: "claude",
      label: `Usage ${Date.now()}`,
    });
    accountHome = account.homePath;
    const dir = join(accountHome, "projects", "e2e");
    mkdirSync(dir, { recursive: true });
    writeFileSync(
      join(dir, "s.jsonl"),
      [line(2, "m1", 1_000, root), line(1, "m2", 2_000, root), "{broken", line(0, "m3", 3_000, `${root}/web`)].join("\n"),
    );
  });

  after(() => {
    rmSync(join(accountHome, "projects"), { recursive: true, force: true });
  });

  it("F-AC-07.1 the page shows the days' totals, the engine, the account and the workspace", async function () {
    this.timeout(30_000);
    await $('button[aria-label^="Workspace menu"]').click();
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const el = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find((x) =>
            (x.textContent ?? "").trim().startsWith("Usage"),
          );
          el?.click();
          return el !== undefined;
        }),
      { timeout: 5_000, timeoutMsg: "no Usage menu item" },
    );
    await $('[data-testid="usage-page"]').waitForDisplayed({ timeout: 15_000 });
    expect(String(await $('[data-testid="usage-total"]').getProperty("textContent"))).toBe("6K tokens");
    const levels = await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-testid="heatmap"] [data-date]')).slice(-3).map((c) => c.dataset.level),
    );
    expect(levels).toEqual(["2", "3", "4"]);
    expect(String(await $('[data-testid="usage-engines"]').getProperty("textContent"))).toContain("Claude Code");
    expect(String(await $('[data-testid="usage-workspaces"]').getProperty("textContent"))).toContain(root.split("/").pop());
  });

  it("F-AC-07.2 the malformed line shows as skipped", async () => {
    expect(String(await $('[data-testid="usage-skipped"]').getProperty("textContent"))).toContain("1 line skipped");
  });

  it("F-AC-07 the share card saves a PNG", async function () {
    this.timeout(30_000);
    const out = join(home(), "usage-card.png");
    writeFileSync(join(home(), "save-path.txt"), out);
    await $('[data-testid="usage-share"]').click();
    await browser.waitUntil(() => existsSync(out), { timeout: 10_000, timeoutMsg: "no share card saved" });
    expect(readFileSync(out).subarray(0, 4).toString("binary")).toBe("\x89PNG");
    await browser.keys("Escape");
  });
});
