import { browser, $ } from "@wdio/globals";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { expectNoSeriousA11yViolations } from "../support/axe";

// E2E-M0-1 Boot smoke (MILESTONES M0).
describe("E2E-M0-1 boot smoke", () => {
  it("shows the Tandem window with the UI-02 splash from the catalog", async () => {
    await browser.waitUntil(async () => (await browser.getTitle()) === "Tandem", { timeout: 20_000 });
    const heading = await $("h1");
    await heading.waitForDisplayed({ timeout: 20_000 });
    expect(await heading.getText()).toBe("Tandem");
    expect(await $('[role="status"]').getText()).toBe("Starting…");
  });

  it("has no serious accessibility violations", async () => {
    await expectNoSeriousA11yViolations();
  });

  it("writes a log file with no secret patterns", async () => {
    const logs = join(process.env.TANDEM_HOME ?? "", "logs");
    await browser.waitUntil(() => existsSync(logs) && readdirSync(logs).length > 0, { timeout: 10_000 });
    for (const file of readdirSync(logs)) {
      const body = readFileSync(join(logs, file), "utf8");
      expect(body).not.toMatch(
        /sk-[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9]{30,}|eyJ[A-Za-z0-9_-]{8,}\.eyJ|BEGIN [A-Z ]*PRIVATE KEY/,
      );
    }
  });
});
